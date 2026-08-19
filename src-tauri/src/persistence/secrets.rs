use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, Mutex},
};

#[derive(Clone, Eq, PartialEq, serde::Deserialize)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.trim().is_empty()
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SecretStoreError {
    #[error("invalid secret reference")]
    InvalidReference,
    #[error("secret store is unavailable")]
    Unavailable,
    #[error("secret store operation failed")]
    Backend,
}

pub trait SecretStore: Send + Sync {
    fn write(&self, reference: &str, secret: SecretString) -> Result<(), SecretStoreError>;
    fn read(&self, reference: &str) -> Result<Option<SecretString>, SecretStoreError>;
    fn delete(&self, reference: &str) -> Result<(), SecretStoreError>;
}

#[derive(Clone, Default)]
pub struct MemorySecretStore {
    values: Arc<Mutex<HashMap<String, SecretString>>>,
}

impl SecretStore for MemorySecretStore {
    fn write(&self, reference: &str, secret: SecretString) -> Result<(), SecretStoreError> {
        validate_reference(reference)?;
        self.values
            .lock()
            .map_err(|_| SecretStoreError::Unavailable)?
            .insert(reference.to_string(), secret);
        Ok(())
    }

    fn read(&self, reference: &str) -> Result<Option<SecretString>, SecretStoreError> {
        validate_reference(reference)?;
        Ok(self
            .values
            .lock()
            .map_err(|_| SecretStoreError::Unavailable)?
            .get(reference)
            .cloned())
    }

    fn delete(&self, reference: &str) -> Result<(), SecretStoreError> {
        validate_reference(reference)?;
        self.values
            .lock()
            .map_err(|_| SecretStoreError::Unavailable)?
            .remove(reference);
        Ok(())
    }
}

fn validate_reference(reference: &str) -> Result<(), SecretStoreError> {
    let valid = !reference.trim().is_empty()
        && reference.len() <= 240
        && reference.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '/' | '-' | '_' | '.')
        });
    if valid {
        Ok(())
    } else {
        Err(SecretStoreError::InvalidReference)
    }
}

#[cfg(target_os = "windows")]
pub trait WindowsCredentialBackend: Send + Sync {
    fn write_credential(&self, target: &str, secret: &str) -> Result<(), SecretStoreError>;
    fn read_credential(&self, target: &str) -> Result<Option<SecretString>, SecretStoreError>;
    fn delete_credential(&self, target: &str) -> Result<(), SecretStoreError>;
}

#[cfg(target_os = "windows")]
#[derive(Clone)]
pub struct WindowsCredentialStore<B> {
    backend: B,
}

#[cfg(target_os = "windows")]
impl<B: Default> Default for WindowsCredentialStore<B> {
    fn default() -> Self {
        Self::new(B::default())
    }
}

#[cfg(target_os = "windows")]
impl<B> WindowsCredentialStore<B> {
    pub fn new(backend: B) -> Self {
        Self { backend }
    }
}

#[cfg(target_os = "windows")]
impl<B: WindowsCredentialBackend> SecretStore for WindowsCredentialStore<B> {
    fn write(&self, reference: &str, secret: SecretString) -> Result<(), SecretStoreError> {
        validate_reference(reference)?;
        self.backend.write_credential(reference, secret.expose())
    }

    fn read(&self, reference: &str) -> Result<Option<SecretString>, SecretStoreError> {
        validate_reference(reference)?;
        self.backend.read_credential(reference)
    }

    fn delete(&self, reference: &str) -> Result<(), SecretStoreError> {
        validate_reference(reference)?;
        self.backend.delete_credential(reference)
    }
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Default)]
pub struct NativeWindowsCredentialBackend;

#[cfg(target_os = "windows")]
impl NativeWindowsCredentialBackend {
    fn entry(target: &str) -> Result<keyring::Entry, SecretStoreError> {
        keyring::Entry::new("com.aimeeting.app", target).map_err(|_| SecretStoreError::Backend)
    }
}

#[cfg(target_os = "windows")]
impl WindowsCredentialBackend for NativeWindowsCredentialBackend {
    fn write_credential(&self, target: &str, secret: &str) -> Result<(), SecretStoreError> {
        Self::entry(target)?
            .set_password(secret)
            .map_err(|_| SecretStoreError::Backend)
    }

    fn read_credential(&self, target: &str) -> Result<Option<SecretString>, SecretStoreError> {
        match Self::entry(target)?.get_password() {
            Ok(secret) => Ok(Some(SecretString::new(secret))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(SecretStoreError::Backend),
        }
    }

    fn delete_credential(&self, target: &str) -> Result<(), SecretStoreError> {
        match Self::entry(target)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(SecretStoreError::Backend),
        }
    }
}

#[cfg(target_os = "windows")]
pub type PlatformSecretStore = WindowsCredentialStore<NativeWindowsCredentialBackend>;

#[cfg(not(target_os = "windows"))]
pub type PlatformSecretStore = MemorySecretStore;
