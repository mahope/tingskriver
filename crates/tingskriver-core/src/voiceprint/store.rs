//! Where voiceprints are kept: Windows Credential Manager (DPAPI) and
//! nowhere else. The database holds ids and dates, never the numbers.

use std::collections::BTreeMap;
use std::sync::Mutex;

use super::embedder::{Embedding, MAX_DIMENSIONS};

/// Every voiceprint target starts with this, which is under
/// [`crate::install::user_data::CREDENTIAL_PREFIX`], so uninstalling with
/// "delete my data" removes them too.
pub const TARGET_PREFIX: &str = "Tingskriver/voiceprint/";

pub fn target_for(exemplar_id: &str) -> String {
    format!("{TARGET_PREFIX}{exemplar_id}")
}

/// Raw secret storage. [`CredentialManagerBackend`] in the app,
/// [`MemoryBackend`] (a fake) in tests.
pub trait SecretBackend: Send + Sync {
    fn read(&self, target: &str) -> Result<Option<String>, String>;
    fn write(&self, target: &str, secret: &str) -> Result<(), String>;
    fn delete(&self, target: &str) -> Result<(), String>;
    /// The target of every entry under `prefix`, including entries the
    /// database does not know about. Used to find leftovers.
    fn list_prefix(&self, prefix: &str) -> Result<Vec<String>, String>;
    /// Delete everything under `prefix`, including entries the database no
    /// longer knows about. Returns how many were removed.
    fn delete_prefix(&self, prefix: &str) -> Result<usize, String>;
}

/// Windows Credential Manager. Off Windows nothing can be stored.
pub struct CredentialManagerBackend;

impl SecretBackend for CredentialManagerBackend {
    fn read(&self, target: &str) -> Result<Option<String>, String> {
        crate::dictation::secrets::read(target)
    }
    fn write(&self, target: &str, secret: &str) -> Result<(), String> {
        crate::dictation::secrets::write(target, secret)
    }
    fn delete(&self, target: &str) -> Result<(), String> {
        crate::dictation::secrets::delete(target)
    }
    fn list_prefix(&self, prefix: &str) -> Result<Vec<String>, String> {
        crate::install::list_credentials_with_prefix(prefix)
    }
    fn delete_prefix(&self, prefix: &str) -> Result<usize, String> {
        crate::install::delete_credentials_with_prefix(prefix)
    }
}

/// In-memory fake for tests.
#[derive(Default)]
pub struct MemoryBackend {
    entries: Mutex<BTreeMap<String, String>>,
    fail_deletes: Mutex<bool>,
}

impl MemoryBackend {
    pub fn targets(&self) -> Vec<String> {
        self.entries
            .lock()
            .map(|e| e.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Make deletes fail, to test that a failed delete is reported and retried.
    pub fn set_fail_deletes(&self, fail: bool) {
        if let Ok(mut f) = self.fail_deletes.lock() {
            *f = fail;
        }
    }

    /// Put an entry the database does not know about (a leftover).
    pub fn plant(&self, target: &str, secret: &str) {
        if let Ok(mut e) = self.entries.lock() {
            e.insert(target.to_string(), secret.to_string());
        }
    }

    fn deletes_fail(&self) -> bool {
        self.fail_deletes.lock().map(|f| *f).unwrap_or(false)
    }
}

impl SecretBackend for MemoryBackend {
    fn read(&self, target: &str) -> Result<Option<String>, String> {
        Ok(self
            .entries
            .lock()
            .map_err(|e| e.to_string())?
            .get(target)
            .cloned())
    }
    fn write(&self, target: &str, secret: &str) -> Result<(), String> {
        self.entries
            .lock()
            .map_err(|e| e.to_string())?
            .insert(target.into(), secret.into());
        Ok(())
    }
    fn delete(&self, target: &str) -> Result<(), String> {
        if self.deletes_fail() {
            return Err("delete refused".to_string());
        }
        self.entries
            .lock()
            .map_err(|e| e.to_string())?
            .remove(target);
        Ok(())
    }
    fn list_prefix(&self, prefix: &str) -> Result<Vec<String>, String> {
        Ok(self
            .entries
            .lock()
            .map_err(|e| e.to_string())?
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect())
    }
    fn delete_prefix(&self, prefix: &str) -> Result<usize, String> {
        if self.deletes_fail() {
            return Err("delete refused".to_string());
        }
        let mut entries = self.entries.lock().map_err(|e| e.to_string())?;
        let before = entries.len();
        entries.retain(|k, _| !k.starts_with(prefix));
        Ok(before - entries.len())
    }
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Little-endian f32 bytes as padded base64.
pub(crate) fn encode(embedding: &Embedding) -> String {
    let bytes: Vec<u8> = embedding
        .values()
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub(crate) fn decode(text: &str) -> Option<Embedding> {
    let mut bytes = Vec::with_capacity(text.len() / 4 * 3);
    let symbols: Vec<u8> = text.bytes().filter(|b| *b != b'=').collect();
    for chunk in symbols.chunks(4) {
        let mut n = 0u32;
        for (i, b) in chunk.iter().enumerate() {
            let v = ALPHABET.iter().position(|a| a == b)? as u32;
            n |= v << (18 - 6 * i);
        }
        let produced = (chunk.len() * 6) / 8;
        for i in 0..produced {
            bytes.push((n >> (16 - 8 * i)) as u8);
        }
    }
    if bytes.is_empty() || bytes.len() % 4 != 0 || bytes.len() / 4 > MAX_DIMENSIONS {
        return None;
    }
    let values = bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    Embedding::new(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoding_roundtrips_for_any_length() {
        for len in [1usize, 2, 3, 5, 8, 192, MAX_DIMENSIONS] {
            let values: Vec<f32> = (0..len).map(|i| (i as f32 + 1.0) * 0.37 - 4.0).collect();
            let e = Embedding::new(values).unwrap();
            let text = encode(&e);
            assert!(text.len() <= 1400, "fits in a credential blob");
            assert_eq!(decode(&text), Some(e), "length {len}");
        }
    }

    #[test]
    fn garbage_does_not_decode() {
        assert_eq!(decode(""), None);
        assert_eq!(decode("!!!!"), None);
        assert_eq!(decode("AAA"), None);
    }

    #[test]
    fn targets_are_under_the_uninstall_prefix() {
        assert!(target_for("x").starts_with(crate::install::user_data::CREDENTIAL_PREFIX));
    }
}
