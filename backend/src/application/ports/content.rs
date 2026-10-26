use super::{OperationContext, PortFuture};
use crate::domain::content::ContentRef;
use std::pin::Pin;
use tokio::io::AsyncRead;
/// Streaming boundary: adapters do not require one payload-sized allocation.
pub type ContentReader = Pin<Box<dyn AsyncRead + Send + Unpin>>;
pub trait ContentStore: Send + Sync {
    fn upload(
        &self,
        context: OperationContext,
        size: u64,
        media_type: String,
        input: ContentReader,
    ) -> PortFuture<'_, ContentRef>;
    fn read(
        &self,
        context: OperationContext,
        reference: ContentRef,
    ) -> PortFuture<'_, ContentReader>;
}
