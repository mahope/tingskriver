//! API keys. In the app they live only in Windows Credential Manager; the
//! provider configuration, the database and logs never hold them.

use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;

/// An API key. `Debug` never shows the value, and there is no `Serialize`,
/// so a key cannot end up in a log line or a config file by accident.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiKey(String);

impl ApiKey {
    /// `None` for an empty or blank key.
    pub fn new(value: impl Into<String>) -> Option<ApiKey> {
        let value = value.into().trim().to_string();
        (!value.is_empty()).then_some(ApiKey(value))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey(***)")
    }
}

/// Where keys are kept. [`CredentialManager`] in the app, [`MemoryKeyStore`]
/// in tests.
pub trait KeyStore: Send + Sync {
    fn get(&self, target: &str) -> Result<Option<ApiKey>, String>;
    fn set(&self, target: &str, key: &ApiKey) -> Result<(), String>;
    fn delete(&self, target: &str) -> Result<(), String>;
}

/// Windows Credential Manager (generic credentials, local machine). On other
/// systems nothing can be stored and every lookup finds no key.
pub struct CredentialManager;

impl KeyStore for CredentialManager {
    fn get(&self, target: &str) -> Result<Option<ApiKey>, String> {
        Ok(crate::dictation::secrets::read(target)?.and_then(ApiKey::new))
    }

    fn set(&self, target: &str, key: &ApiKey) -> Result<(), String> {
        crate::dictation::secrets::write(target, key.expose())
    }

    fn delete(&self, target: &str) -> Result<(), String> {
        crate::dictation::secrets::delete(target)
    }
}

/// In-memory key store for tests and for hosts without Credential Manager.
#[derive(Default)]
pub struct MemoryKeyStore {
    keys: Mutex<HashMap<String, ApiKey>>,
    /// Make `set` fail, to test what happens when Credential Manager refuses.
    fail_writes: bool,
}

impl MemoryKeyStore {
    /// A store whose writes always fail.
    pub fn failing() -> MemoryKeyStore {
        MemoryKeyStore {
            fail_writes: true,
            ..MemoryKeyStore::default()
        }
    }

    pub fn targets(&self) -> Vec<String> {
        let mut targets: Vec<String> = self
            .keys
            .lock()
            .map(|k| k.keys().cloned().collect())
            .unwrap_or_default();
        targets.sort();
        targets
    }
}

impl KeyStore for MemoryKeyStore {
    fn get(&self, target: &str) -> Result<Option<ApiKey>, String> {
        let keys = self.keys.lock().map_err(|e| e.to_string())?;
        Ok(keys.get(target).cloned())
    }

    fn set(&self, target: &str, key: &ApiKey) -> Result<(), String> {
        if self.fail_writes {
            return Err("Nøglen kunne ikke gemmes".to_string());
        }
        let mut keys = self.keys.lock().map_err(|e| e.to_string())?;
        keys.insert(target.to_string(), key.clone());
        Ok(())
    }

    fn delete(&self, target: &str) -> Result<(), String> {
        let mut keys = self.keys.lock().map_err(|e| e.to_string())?;
        keys.remove(target);
        Ok(())
    }
}

/// Replace every occurrence of `key` in `text`. Used on any text that came
/// from outside (error details) before it is shown or logged.
pub fn redact(text: &str, key: Option<&ApiKey>) -> String {
    match key {
        Some(key) if key.expose().len() >= 4 => text.replace(key.expose(), "***"),
        _ => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_hides_the_value() {
        let key = ApiKey::new("sk-hemmelig").unwrap();
        assert_eq!(format!("{key:?}"), "ApiKey(***)");
        assert_eq!(format!("{:?}", Some(key)), "Some(ApiKey(***))");
    }

    #[test]
    fn blank_keys_are_no_keys() {
        assert!(ApiKey::new("  ").is_none());
        assert_eq!(ApiKey::new(" sk-1 \n").unwrap().expose(), "sk-1");
    }

    #[test]
    fn redact_removes_the_key() {
        let key = ApiKey::new("sk-hemmelig").unwrap();
        assert_eq!(
            redact("fejl: sk-hemmelig afvist", Some(&key)),
            "fejl: *** afvist"
        );
        assert_eq!(redact("intet", None), "intet");
    }

    #[test]
    fn memory_store_roundtrip() {
        let store = MemoryKeyStore::default();
        let key = ApiKey::new("k1").unwrap();
        store.set("a", &key).unwrap();
        assert_eq!(store.get("a").unwrap(), Some(key));
        store.delete("a").unwrap();
        assert_eq!(store.get("a").unwrap(), None);
    }
}
