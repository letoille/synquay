//! Managed connection credentials. SQLite contains authenticated ciphertext only.
//! The adjacent owner-only key file must be backed up with the database.
use super::*;
use crate::infrastructure::pipeline::message_connections::ConnectionInput;
use base64::{Engine, engine::general_purpose::STANDARD};
use ring::{
    aead, hmac,
    rand::{SecureRandom, SystemRandom},
};
use std::io::{Read, Write};

fn key_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".connection-secrets.key");
    PathBuf::from(name)
}
pub(super) struct Key([u8; 32]);
impl Drop for Key {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}
impl Key {
    pub fn load(path: &Path, create: bool) -> Result<Self, PortError> {
        let key_path = key_path(path);
        if !key_path.exists() && create {
            let mut bytes = [0u8; 32];
            SystemRandom::new()
                .fill(&mut bytes)
                .map_err(|_| PortError::Unavailable)?;
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&key_path) {
                Ok(mut file) => {
                    file.write_all(&bytes)
                        .and_then(|_| file.sync_all())
                        .map_err(|_| PortError::Unavailable)?;
                    std::fs::File::open(key_path.parent().ok_or(PortError::Unavailable)?)
                        .and_then(|dir| dir.sync_all())
                        .map_err(|_| PortError::Unavailable)?;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return Err(PortError::Unavailable),
            }
            bytes.fill(0);
        }
        let meta = std::fs::symlink_metadata(&key_path).map_err(|_| PortError::Unavailable)?;
        if !meta.is_file() || meta.len() != 32 {
            return Err(PortError::Unavailable);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if meta.permissions().mode() & 0o077 != 0 {
                return Err(PortError::Unavailable);
            }
        }
        let mut bytes = [0u8; 32];
        #[cfg(unix)]
        let mut file = {
            let fd = rustix::fs::open(
                &key_path,
                rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::NOFOLLOW,
                rustix::fs::Mode::empty(),
            )
            .map_err(|_| PortError::Unavailable)?;
            std::fs::File::from(fd)
        };
        #[cfg(not(unix))]
        let mut file = std::fs::File::open(key_path).map_err(|_| PortError::Unavailable)?;
        let opened = file.metadata().map_err(|_| PortError::Unavailable)?;
        if !opened.is_file() || opened.len() != 32 {
            return Err(PortError::Unavailable);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if opened.permissions().mode() & 0o077 != 0 {
                return Err(PortError::Unavailable);
            }
        }
        file.read_exact(&mut bytes)
            .map_err(|_| PortError::Unavailable)?;
        Ok(Self(bytes))
    }
    fn aead(&self) -> Result<aead::LessSafeKey, PortError> {
        aead::UnboundKey::new(&aead::AES_256_GCM, &self.0)
            .map(aead::LessSafeKey::new)
            .map_err(|_| PortError::Unavailable)
    }
    pub fn seal(&self, aad: &[u8], value: &str) -> Result<Vec<u8>, PortError> {
        let mut nonce = [0u8; 12];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| PortError::Unavailable)?;
        let mut data = value.as_bytes().to_vec();
        self.aead()?
            .seal_in_place_append_tag(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::from(aad),
                &mut data,
            )
            .map_err(|_| PortError::Unavailable)?;
        let mut output = nonce.to_vec();
        output.extend(data);
        Ok(output)
    }
    pub fn open(&self, aad: &[u8], mut data: Vec<u8>) -> Result<String, PortError> {
        if data.len() < 28 || data.len() > 4124 {
            return Err(PortError::Unavailable);
        }
        let nonce: [u8; 12] = data[..12].try_into().map_err(|_| PortError::Unavailable)?;
        let plain = self
            .aead()?
            .open_in_place(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::from(aad),
                &mut data[12..],
            )
            .map_err(|_| PortError::Unavailable)?;
        String::from_utf8(plain.to_vec()).map_err(|_| PortError::Unavailable)
    }
    fn fingerprint(&self, input: &ConnectionInput) -> Result<String, PortError> {
        let root = hmac::Key::new(hmac::HMAC_SHA256, &self.0);
        let derived = hmac::sign(&root, b"synquay.connection-request-fingerprint.v1");
        let key = hmac::Key::new(hmac::HMAC_SHA256, derived.as_ref());
        Ok(format!(
            "hmac-v1:{}",
            STANDARD.encode(hmac::sign(&key, encode(input)?.as_bytes()).as_ref())
        ))
    }
}
pub(super) fn aad(scope: crate::domain::connector::ConnectionScope, id: Uuid) -> Vec<u8> {
    format!(
        "synquay.qq-secret.v1:{}:{}:{}",
        scope.workspace_id, scope.connection_id, id
    )
    .into_bytes()
}
pub(super) fn request_record(
    db: &Connection,
    path: &Path,
    input: &ConnectionInput,
) -> Result<String, PortError> {
    if input.secret.is_none() {
        return encode(input);
    }
    // Never silently replace a lost key: old ciphertext and operation fingerprints depend on it.
    let exists:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM connection_secrets) OR EXISTS(SELECT 1 FROM message_connection_operations WHERE request LIKE 'hmac-v1:%')",[],|r|r.get(0)).map_err(db_error)?;
    Key::load(path, !exists)?.fingerprint(input)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ciphertext_is_scoped_and_requires_the_original_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let key = Key::load(&path, true).unwrap();
        let encrypted = key.seal(b"scope-a", "test-secret").unwrap();
        assert_eq!(
            Key::load(&path, false)
                .unwrap()
                .open(b"scope-a", encrypted.clone())
                .unwrap(),
            "test-secret"
        );
        assert!(key.open(b"scope-b", encrypted.clone()).is_err());
        let mut changed = encrypted;
        changed[15] ^= 1;
        assert!(key.open(b"scope-a", changed).is_err());
        std::fs::remove_file(key_path(&path)).unwrap();
        assert!(Key::load(&path, false).is_err());
    }
}
