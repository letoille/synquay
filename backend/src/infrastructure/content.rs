//! Private local blob store. SQL reserves quota before any payload bytes are read.
use super::storage::SqliteStore;
use crate::{
    application::ports::{
        OperationContext, PortError, PortFuture,
        content::{ContentReader, ContentStore},
    },
    domain::{connector::ConnectionScope, content::ContentRef},
};
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::Semaphore,
};
use uuid::Uuid;

#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ContentConfig {
    pub directory: PathBuf,
    pub max_object_bytes: u64,
    pub max_total_bytes: u64,
    pub max_objects: u64,
    pub concurrent_uploads: usize,
}
impl Default for ContentConfig {
    fn default() -> Self {
        Self {
            directory: "content".into(),
            max_object_bytes: 64 * 1024 * 1024,
            max_total_bytes: 1024 * 1024 * 1024,
            max_objects: 10000,
            concurrent_uploads: 4,
        }
    }
}
impl ContentConfig {
    pub fn validate(&self) -> Result<(), PortError> {
        if self.directory.as_os_str().is_empty()
            || self.max_object_bytes == 0
            || self.max_object_bytes > 1024 * 1024 * 1024
            || self.max_total_bytes < self.max_object_bytes
            || self.max_total_bytes > i64::MAX as u64
            || self.max_objects == 0
            || self.concurrent_uploads == 0
            || self.concurrent_uploads > 16
        {
            Err(PortError::InvalidInput {
                code: "content_config",
            })
        } else {
            Ok(())
        }
    }
}
pub struct FileContentStore {
    store: Arc<SqliteStore>,
    config: ContentConfig,
    source: ConnectionScope,
    principal: Uuid,
    allowed_connections: Vec<Uuid>,
    uploads: Arc<Semaphore>,
}
impl FileContentStore {
    pub async fn open(
        store: Arc<SqliteStore>,
        config: ContentConfig,
        source: ConnectionScope,
        principal: Uuid,
        targets: Vec<Uuid>,
    ) -> Result<Self, PortError> {
        config.validate()?;
        let mut builder = tokio::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        builder.mode(0o700);
        builder.create(&config.directory).await.map_err(io_error)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let m = tokio::fs::metadata(&config.directory)
                .await
                .map_err(io_error)?;
            if m.permissions().mode() & 0o077 != 0 {
                return Err(PortError::InvalidInput {
                    code: "content_directory_not_private",
                });
            }
        }
        let mut config = config;
        config.directory = tokio::fs::canonicalize(&config.directory)
            .await
            .map_err(io_error)?;
        store
            .bind_content_directory(config.directory.to_string_lossy().into_owned())
            .await?;
        let uploads = Arc::new(Semaphore::new(config.concurrent_uploads));
        let mut allowed_connections = targets;
        allowed_connections.push(source.connection_id);
        Ok(Self {
            store,
            config,
            source,
            principal,
            allowed_connections,
            uploads,
        })
    }
    pub async fn download(
        &self,
        ctx: OperationContext,
        id: Uuid,
    ) -> Result<(ContentRef, ContentReader), PortError> {
        self.authorize(&ctx, false)?;
        let reference = self.store.content_reference(self.source, id).await?;
        let input = self.read(ctx, reference.clone()).await?;
        Ok((reference, input))
    }
    fn authorize(&self, ctx: &OperationContext, upload: bool) -> Result<(), PortError> {
        ctx.check_active()?;
        if ctx.scope.workspace_id != self.source.workspace_id
            || ctx.principal_id != self.principal
            || !self.allowed_connections.contains(&ctx.scope.connection_id)
            || (upload && ctx.scope != self.source)
        {
            Err(PortError::PermissionDenied)
        } else {
            Ok(())
        }
    }
    fn path(&self, id: Uuid) -> PathBuf {
        self.config.directory.join(format!("{id}.blob"))
    }
    /// Bounded, retryable orphan cleanup; linked content survives until payload retention releases it.
    pub async fn collect(&self) -> Result<usize, PortError> {
        let before = SystemTime::now()
            .checked_sub(Duration::from_secs(3600))
            .ok_or(PortError::Unavailable)?
            .duration_since(UNIX_EPOCH)
            .map_err(|_| PortError::Unavailable)?
            .as_millis() as i64;
        let ids = self.store.content_garbage(before, 100).await?;
        let mut removed = 0;
        for id in ids {
            remove_file(&self.path(id)).await?;
            self.store.forget_content(id).await?;
            removed += 1;
        }
        Ok(removed)
    }
}
fn io_error(_: std::io::Error) -> PortError {
    PortError::Unavailable
}
async fn remove_file(path: &Path) -> Result<(), PortError> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io_error(e)),
    }
}
struct UploadGuard {
    store: Arc<SqliteStore>,
    path: PathBuf,
    id: Uuid,
    armed: bool,
}
impl Drop for UploadGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let store = self.store.clone();
        let path = self.path.clone();
        let id = self.id;
        // Best effort immediately; persisted reservation is retried by collect after a crash.
        tokio::spawn(async move {
            if store.abort_content(id).await == Ok(true) && remove_file(&path).await.is_ok() {
                let _ = store.forget_content(id).await;
            }
        });
    }
}
impl ContentStore for FileContentStore {
    fn upload(
        &self,
        mut ctx: OperationContext,
        size: u64,
        media_type: String,
        mut input: ContentReader,
    ) -> PortFuture<'_, ContentRef> {
        Box::pin(async move {
            let max_deadline = std::time::Instant::now() + Duration::from_secs(300);
            ctx.deadline = Some(ctx.deadline.map_or(max_deadline, |d| d.min(max_deadline)));
            self.authorize(&ctx, true)?;
            if size > self.config.max_object_bytes {
                return Err(PortError::PayloadTooLarge);
            }
            if media_type.is_empty()
                || media_type.len() > 128
                || !media_type.bytes().all(|b| b.is_ascii_graphic())
            {
                return Err(PortError::InvalidInput {
                    code: "content_media_type",
                });
            }
            let _permit =
                self.uploads
                    .clone()
                    .try_acquire_owned()
                    .map_err(|_| PortError::RateLimited {
                        retry_after: Duration::from_secs(1),
                    })?;
            let id = Uuid::new_v4();
            let path = self.path(id);
            // Create guard before reservation: cancellation may race the SQL commit.
            let mut guard = UploadGuard {
                store: self.store.clone(),
                path: path.clone(),
                id,
                armed: true,
            };
            self.store
                .reserve_content(
                    ContentRef {
                        id,
                        scope: self.source,
                        size,
                        media_type: media_type.clone(),
                        sha256: String::new(),
                    },
                    self.config.max_total_bytes,
                    self.config.max_objects,
                    self.config.directory.clone(),
                )
                .await?;
            let mut options = tokio::fs::OpenOptions::new();
            options.create_new(true).write(true);
            #[cfg(unix)]
            options.mode(0o600);
            let mut file = options.open(&path).await.map_err(io_error)?;
            let mut hash = ring::digest::Context::new(&ring::digest::SHA256);
            let mut total = 0u64;
            let mut buffer = vec![0u8; 64 * 1024];
            loop {
                let count = tokio::select! {biased;error=ctx.stopped()=>return Err(error),result=input.read(&mut buffer)=>result.map_err(io_error)?};
                if count == 0 {
                    break;
                }
                total = total
                    .checked_add(count as u64)
                    .ok_or(PortError::PayloadTooLarge)?;
                if total > size {
                    return Err(PortError::PayloadTooLarge);
                }
                hash.update(&buffer[..count]);
                tokio::select! {biased;error=ctx.stopped()=>return Err(error),result=file.write_all(&buffer[..count])=>result.map_err(io_error)?};
            }
            if total != size {
                return Err(PortError::InvalidInput {
                    code: "content_length_mismatch",
                });
            }
            file.flush().await.map_err(io_error)?;
            file.sync_all().await.map_err(io_error)?;
            drop(file);
            // Persist directory entry before publishing a durable reference.
            let directory = self.config.directory.clone();
            tokio::task::spawn_blocking(move || std::fs::File::open(directory)?.sync_all())
                .await
                .map_err(|_| PortError::Unavailable)?
                .map_err(io_error)?;
            let sha256 = hash
                .finish()
                .as_ref()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            let reference = ContentRef {
                id,
                scope: self.source,
                size,
                sha256,
                media_type,
            };
            self.store.stage_content(reference.clone()).await?;
            guard.armed = false;
            Ok(reference)
        })
    }
    fn read(&self, ctx: OperationContext, reference: ContentRef) -> PortFuture<'_, ContentReader> {
        Box::pin(async move {
            self.authorize(&ctx, false)?;
            if reference.scope != self.source {
                return Err(PortError::ScopeMismatch);
            }
            self.store.check_content(reference.clone()).await?;
            let file = tokio::fs::File::open(self.path(reference.id))
                .await
                .map_err(io_error)?;
            if file.metadata().await.map_err(io_error)?.len() != reference.size {
                return Err(PortError::Failed {
                    code: "content_size_corrupt",
                });
            }
            Ok(Box::pin(VerifiedReader {
                file,
                remaining: reference.size,
                digest: Some(ring::digest::Context::new(&ring::digest::SHA256)),
                expected: reference.sha256,
                failed: false,
            }) as ContentReader)
        })
    }
}

/// Hash as bytes are consumed. Corruption fails before the final chunk is handed off.
struct VerifiedReader {
    file: tokio::fs::File,
    remaining: u64,
    digest: Option<ring::digest::Context>,
    expected: String,
    failed: bool,
}
impl tokio::io::AsyncRead for VerifiedReader {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buffer: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        use std::task::Poll;
        if self.failed {
            return Poll::Ready(Err(std::io::Error::other("content integrity failure")));
        }
        if buffer.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        let before = buffer.filled().len();
        match std::pin::Pin::new(&mut self.file).poll_read(cx, buffer) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
            Poll::Ready(Ok(())) => {}
        }
        let bytes = &buffer.filled()[before..];
        if bytes.len() as u64 > self.remaining || (bytes.is_empty() && self.remaining != 0) {
            buffer.set_filled(before);
            self.failed = true;
            return Poll::Ready(Err(std::io::Error::other("content size mismatch")));
        }
        self.remaining -= bytes.len() as u64;
        if let Some(digest) = &mut self.digest {
            digest.update(bytes);
        }
        if self.remaining == 0
            && let Some(digest) = self.digest.take()
        {
            let actual: String = digest
                .finish()
                .as_ref()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            if actual != self.expected {
                buffer.set_filled(before);
                self.failed = true;
                return Poll::Ready(Err(std::io::Error::other("content digest mismatch")));
            }
        }
        Poll::Ready(Ok(()))
    }
}
