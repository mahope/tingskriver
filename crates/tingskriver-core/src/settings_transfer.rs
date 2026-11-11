//! Portable settings snapshots. Persistence and confirmation belong to the caller.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::dictation::settings::DictationSettings;
use crate::providers::model::{key_target, SYV_ID};
use crate::providers::model::{ProvidersConfig, CONFIG_VERSION};
use crate::providers::secrets::{CredentialManager, KeyStore};
use crate::shortcuts::{ShortcutAction, ShortcutSettings};

pub const TRANSFER_VERSION: u32 = 1;

/// No credentials, calendar feed address, history, or machine-local row ids.
/// The caller gathers this data and persists the confirmed preview's result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsSnapshot {
    pub version: u32,
    pub settings: MeetingSettings,
    pub dictation: DictationSettings,
    pub dictionary: Vec<Word>,
    pub snippets: Vec<Snippet>,
    pub shortcuts: ShortcutSettings,
    pub providers: ProvidersConfig,
}

impl Default for SettingsSnapshot {
    fn default() -> Self {
        Self {
            version: TRANSFER_VERSION,
            settings: MeetingSettings::default(),
            dictation: DictationSettings::default(),
            dictionary: Vec::new(),
            snippets: Vec::new(),
            shortcuts: ShortcutSettings::default(),
            providers: ProvidersConfig::default(),
        }
    }
}

/// Legacy meeting preferences, deliberately projected instead of serializing
/// database rows (which can still contain API keys and custom JSON).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MeetingSettings {
    pub provider: String,
    pub model: String,
    pub whisper_model: String,
    pub transcript_provider: String,
    pub transcript_model: String,
}

impl MeetingSettings {
    /// Endpoints and custom model options belong to the shared provider config.
    pub fn from_legacy(
        settings: &crate::database::models::Setting,
        transcript: &crate::database::models::TranscriptSetting,
    ) -> Self {
        // Exhaustive patterns force every newly added database field to be
        // reviewed here before it can accidentally become portable.
        let crate::database::models::Setting {
            id: _,
            provider,
            model,
            whisper_model,
            groq_api_key: _,
            openai_api_key: _,
            anthropic_api_key: _,
            ollama_api_key: _,
            open_router_api_key: _,
            ollama_endpoint: _,
            custom_openai_config: _,
        } = settings;
        let crate::database::models::TranscriptSetting {
            id: _,
            provider: transcript_provider,
            model: transcript_model,
            whisper_api_key: _,
            deepgram_api_key: _,
            eleven_labs_api_key: _,
            groq_api_key: _,
            openai_api_key: _,
        } = transcript;
        Self {
            provider: provider.clone(),
            model: model.clone(),
            whisper_model: whisper_model.clone(),
            transcript_provider: transcript_provider.clone(),
            transcript_model: transcript_model.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Word {
    pub word: String,
    pub aliases: String,
    pub english: bool,
}

impl From<&crate::dictation::store::DictionaryWord> for Word {
    fn from(row: &crate::dictation::store::DictionaryWord) -> Self {
        Self {
            word: row.word.clone(),
            aliases: row.aliases.clone(),
            english: row.english,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snippet {
    pub phrase: String,
    pub text: String,
}

impl From<&crate::dictation::store::SnippetRow> for Snippet {
    fn from(row: &crate::dictation::store::SnippetRow) -> Self {
        Self {
            phrase: row.phrase.clone(),
            text: row.text.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportMode {
    /// Incoming preferences win; collections merge by word, phrase, provider
    /// id and function. An incoming function replaces its entire ordered chain.
    Merge,
    /// Replace portable data only. The caller must retain credentials and ICS.
    Replace,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Change {
    /// Keyed path with JSON Pointer escaping; object arrays use ids, not indices.
    pub path: String,
    pub before: Option<Value>,
    pub after: Option<Value>,
    pub warning: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImportPreview {
    pub changes: Vec<Change>,
    pub result: SettingsSnapshot,
    pending_endpoints: Vec<(String, String, String, String)>,
}

impl ImportPreview {
    /// Delete credentials before exposing any explicitly accepted new address.
    /// A failed deletion leaves the safe result unchanged.
    pub fn accept_endpoint_changes(&mut self, keys: &dyn KeyStore) -> Result<(), String> {
        for (_, _, target, _) in &self.pending_endpoints {
            keys.delete(target)?;
        }
        for (path, endpoint, _, _) in self.pending_endpoints.drain(..) {
            if path == "/dictation/syvEndpoint" {
                self.result.dictation.syv_endpoint = endpoint;
            } else {
                let id = path
                    .strip_prefix("/providers/providers/")
                    .unwrap()
                    .strip_suffix("/baseUrl")
                    .unwrap()
                    .replace("~1", "/")
                    .replace("~0", "~");
                if let Some(provider) = self
                    .result
                    .providers
                    .providers
                    .iter_mut()
                    .find(|p| p.id == id)
                {
                    provider.base_url = endpoint;
                }
            }
        }
        Ok(())
    }
}

impl SettingsSnapshot {
    pub fn export_json(&self) -> Result<String, String> {
        self.validate()?;
        serde_json::to_string_pretty(self)
            .map_err(|_| "Indstillingerne kunne ikke eksporteres".into())
    }

    pub fn import_json(json: &str) -> Result<Self, String> {
        // Never echo input (it may contain credentials from another format).
        let snapshot: Self =
            serde_json::from_str(json).map_err(|_| "Ugyldig indstillingsfil".to_string())?;
        snapshot.validate()?;
        Ok(snapshot)
    }

    fn validate(&self) -> Result<(), String> {
        if self.version != TRANSFER_VERSION || self.providers.version != CONFIG_VERSION {
            return Err("Ukendt version af indstillingsfilen".into());
        }
        for provider in &self.providers.providers {
            safe_endpoint(&provider.base_url)
                .map_err(|_| format!("Udbyderen \"{}\" har en ugyldig adresse", provider.id))?;
        }
        for (provider, endpoint) in [
            ("Syv.ai", &self.dictation.syv_endpoint),
            ("tekstforbedring", &self.dictation.polish_endpoint),
        ] {
            safe_endpoint(endpoint)
                .map_err(|_| format!("Udbyderen {provider} har en ugyldig adresse"))?;
        }
        self.providers.validate()?;
        let normalized = self
            .dictation
            .clone()
            .normalized()
            .map_err(|_| "Ugyldige dikteringsindstillinger".to_string())?;
        if normalized != self.dictation {
            return Err("Dikteringsindstillingerne skal være normaliserede før overførsel".into());
        }
        if let Some(conflict) = crate::shortcuts::check(
            self.dictation
                .enabled
                .then_some(self.dictation.shortcut.as_str()),
            &self.shortcuts,
        )
        .iter()
        .find(|c| c.blocks())
        {
            return Err(format!(
                "Ugyldig eller sammenfaldende tastaturgenvej: {}",
                conflict.shortcut
            ));
        }
        unique(self.dictionary.iter().map(|w| w.word.as_str()))?;
        unique(self.snippets.iter().map(|s| s.phrase.as_str()))?;
        if self.snippets.iter().any(|s| s.text.trim().is_empty()) {
            return Err("En tekstgenvej mangler tekst".into());
        }
        Ok(())
    }

    /// Read-only operation: inspect `changes`, ask for confirmation in the UI, then
    /// persist `result`. Neither preview nor parsing touches the live settings.
    pub fn preview(&self, incoming: &Self, mode: ImportMode) -> Result<ImportPreview, String> {
        self.preview_with_keys(incoming, mode, &CredentialManager)
    }

    pub fn preview_with_keys(
        &self,
        incoming: &Self,
        mode: ImportMode,
        keys: &dyn KeyStore,
    ) -> Result<ImportPreview, String> {
        self.validate()?;
        incoming.validate()?;
        let mut result = incoming.clone();
        if mode == ImportMode::Merge {
            result.dictation.microphone = self.dictation.microphone.clone();
            result.dictation.channel = self.dictation.channel.clone();
            result.dictation.modes.modes = merge_by(
                &self.dictation.modes.modes,
                &incoming.dictation.modes.modes,
                |m| &m.id,
            );
            result.dictation.app_styles = merge_by(
                &self.dictation.app_styles,
                &incoming.dictation.app_styles,
                |s| &s.process,
            );
            result.dictionary = merge_by(&self.dictionary, &incoming.dictionary, |w| &w.word);
            result.snippets = merge_by(&self.snippets, &incoming.snippets, |s| &s.phrase);
            for action in ShortcutAction::REGISTERED {
                if incoming.shortcuts.get(action).is_none() {
                    result
                        .shortcuts
                        .set(action, self.shortcuts.get(action).map(str::to_string));
                }
            }
            result.providers = self.providers.clone();
            for provider in &incoming.providers.providers {
                result.providers.upsert_provider(provider.clone());
            }
            result
                .providers
                .functions
                .extend(incoming.providers.functions.clone());
        }
        let mut pending_endpoints = Vec::new();
        for provider in &mut result.providers.providers {
            if let Some(local) = self.providers.provider(&provider.id) {
                protect_endpoint(
                    &format!("/providers/providers/{}/baseUrl", escape(&provider.id)),
                    &local.base_url,
                    &mut provider.base_url,
                    &key_target(&provider.id),
                    keys,
                    &mut pending_endpoints,
                )?;
            } else if keys.get(&key_target(&provider.id))?.is_some() {
                // Without a local endpoint there is no trusted origin to retain.
                return Err(format!(
                    "Udbyderen \"{}\" kræver ny nøgle: slet den gemte nøgle før import",
                    provider.id
                ));
            }
        }
        protect_endpoint(
            "/dictation/syvEndpoint",
            &self.dictation.syv_endpoint,
            &mut result.dictation.syv_endpoint,
            &key_target(SYV_ID),
            keys,
            &mut pending_endpoints,
        )?;
        result.validate()?;
        let before = serde_json::to_value(self).map_err(|_| "Diff kunne ikke beregnes")?;
        let after = serde_json::to_value(&result).map_err(|_| "Diff kunne ikke beregnes")?;
        let mut changes = Vec::new();
        diff("", Some(&before), Some(&after), &mut changes);
        for (path, endpoint, _, local) in &pending_endpoints {
            changes.push(Change {
                path: path.clone(),
                before: Some(Value::String(local.clone())),
                after: Some(Value::String(endpoint.clone())),
                warning: Some("kræver ny nøgle".into()),
            });
        }
        Ok(ImportPreview {
            changes,
            result,
            pending_endpoints,
        })
    }
}

fn escape(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

fn protect_endpoint(
    path: &str,
    local: &str,
    incoming: &mut String,
    target: &str,
    keys: &dyn KeyStore,
    pending: &mut Vec<(String, String, String, String)>,
) -> Result<(), String> {
    // Origins include scheme and port: neither downgrades nor another service
    // on the same host may inherit an existing credential.
    let origin = |url: &str| reqwest::Url::parse(url).ok().map(|u| u.origin());
    if origin(local) != origin(incoming) && keys.get(target)?.is_some() {
        pending.push((path.into(), incoming.clone(), target.into(), local.into()));
        *incoming = local.into();
    }
    Ok(())
}

fn safe_endpoint(endpoint: &str) -> Result<(), String> {
    if endpoint.is_empty() {
        return Ok(());
    }
    let url = reqwest::Url::parse(endpoint).map_err(|_| "Ugyldig udbyderadresse")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("Udbyderadresser må ikke indeholde login, query eller fragment".into());
    }
    Ok(())
}

fn unique<'a>(keys: impl Iterator<Item = &'a str>) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for key in keys {
        if key.trim().is_empty() || !seen.insert(key.trim().to_lowercase()) {
            return Err("Tomme eller gentagne poster i indstillingsfilen".into());
        }
    }
    Ok(())
}

fn merge_by<T: Clone>(current: &[T], incoming: &[T], key: impl Fn(&T) -> &str) -> Vec<T> {
    let mut merged = current.to_vec();
    for item in incoming {
        if let Some(existing) = merged
            .iter_mut()
            .find(|v| key(v).trim().to_lowercase() == key(item).trim().to_lowercase())
        {
            *existing = item.clone();
        } else {
            merged.push(item.clone());
        }
    }
    merged
}

fn diff(path: &str, before: Option<&Value>, after: Option<&Value>, changes: &mut Vec<Change>) {
    if before == after {
        return;
    }
    if let (Some(Value::Object(a)), Some(Value::Object(b))) = (before, after) {
        let keys: BTreeSet<_> = a.keys().chain(b.keys()).collect();
        for key in keys {
            let escaped = key.replace('~', "~0").replace('/', "~1");
            diff(
                &format!("{path}/{escaped}"),
                a.get(key),
                b.get(key),
                changes,
            );
        }
    } else if let (Some(Value::Array(a)), Some(Value::Array(b))) = (before, after) {
        let key = if path == "/dictionary" { "word" } else { "id" };
        if a.iter()
            .chain(b)
            .all(|v| v.get(key).and_then(Value::as_str).is_some())
        {
            let order_before: Vec<_> = a.iter().map(|v| v[key].clone()).collect();
            let order_after: Vec<_> = b.iter().map(|v| v[key].clone()).collect();
            if order_before != order_after {
                changes.push(Change {
                    path: path.into(),
                    before: Some(Value::Array(order_before)),
                    after: Some(Value::Array(order_after)),
                    warning: None,
                });
            }
            let ids: BTreeSet<_> = a.iter().chain(b).filter_map(|v| v[key].as_str()).collect();
            for id in ids {
                diff(
                    &format!("{path}/{}", escape(id)),
                    a.iter().find(|v| v[key] == id),
                    b.iter().find(|v| v[key] == id),
                    changes,
                );
            }
        } else {
            changes.push(Change {
                path: path.into(),
                before: before.cloned(),
                after: after.cloned(),
                warning: None,
            });
        }
    } else {
        changes.push(Change {
            path: path.into(),
            before: before.cloned(),
            after: after.cloned(),
            warning: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reintroduced_provider_requires_removing_orphaned_key() {
        use crate::providers::secrets::{ApiKey, MemoryKeyStore};
        for mode in [ImportMode::Merge, ImportMode::Replace] {
            let incoming = SettingsSnapshot::default();
            let id = incoming.providers.providers[0].id.clone();
            let mut current = incoming.clone();
            current.providers.providers.retain(|p| p.id != id);
            current.providers.functions.clear();
            let store = MemoryKeyStore::default();
            store
                .set(&key_target(&id), &ApiKey::new("test-key").unwrap())
                .unwrap();
            let mut incoming = incoming;
            incoming.providers.providers[0].base_url = "https://example.com/v1".into();
            let error = current
                .preview_with_keys(&incoming, mode, &store)
                .unwrap_err();
            assert!(error.contains(&id));
            assert!(error.contains("kræver ny nøgle"));
            assert!(store.get(&key_target(&id)).unwrap().is_some());
            store.delete(&key_target(&id)).unwrap();
            let preview = current.preview_with_keys(&incoming, mode, &store).unwrap();
            assert_eq!(
                preview.result.providers.provider(&id).unwrap().base_url,
                "https://example.com/v1"
            );
        }
    }

    #[test]
    fn preview_reports_mode_order_changes() {
        let current = SettingsSnapshot::default();
        let mut incoming = current.clone();
        incoming.dictation.modes.modes.swap(0, 1);
        let store = crate::providers::secrets::MemoryKeyStore::default();
        let preview = current
            .preview_with_keys(&incoming, ImportMode::Replace, &store)
            .unwrap();
        let change = preview
            .changes
            .iter()
            .find(|c| c.path == "/dictation/modes/modes")
            .unwrap();
        assert_eq!(
            change.before.as_ref().unwrap()[0],
            current.dictation.modes.modes[0].id
        );
        assert_eq!(
            change.after.as_ref().unwrap()[0],
            incoming.dictation.modes.modes[0].id
        );
        assert!(current
            .preview_with_keys(&incoming, ImportMode::Merge, &store)
            .unwrap()
            .changes
            .is_empty());
    }

    #[test]
    fn endpoint_acceptance_fails_closed_when_key_deletion_fails() {
        use crate::providers::secrets::ApiKey;
        struct RefusingStore;
        impl KeyStore for RefusingStore {
            fn get(&self, _: &str) -> Result<Option<ApiKey>, String> {
                Ok(ApiKey::new("test-key"))
            }
            fn set(&self, _: &str, _: &ApiKey) -> Result<(), String> {
                unreachable!()
            }
            fn delete(&self, _: &str) -> Result<(), String> {
                Err("Nøglen kunne ikke slettes".into())
            }
        }
        let current = SettingsSnapshot::default();
        let mut incoming = current.clone();
        incoming.dictation.syv_endpoint = "https://example.com/v1".into();
        let mut preview = current
            .preview_with_keys(&incoming, ImportMode::Replace, &RefusingStore)
            .unwrap();
        assert!(preview.accept_endpoint_changes(&RefusingStore).is_err());
        assert_eq!(preview.result, current);
        assert_eq!(
            preview.changes[0].before,
            Some(Value::String(current.dictation.syv_endpoint.clone()))
        );
        incoming.dictation.syv_endpoint = format!("{}/other", current.dictation.syv_endpoint);
        let preview = current
            .preview_with_keys(&incoming, ImportMode::Replace, &RefusingStore)
            .unwrap();
        assert_eq!(preview.result, incoming);
        assert!(preview.changes.iter().all(|c| c.warning.is_none()));
    }

    #[test]
    fn endpoint_changes_keep_keys_local_until_explicit_acceptance() {
        use crate::providers::secrets::{ApiKey, MemoryKeyStore};
        for mode in [ImportMode::Merge, ImportMode::Replace] {
            let current = SettingsSnapshot::default();
            let mut incoming = current.clone();
            let id = current.providers.providers[0].id.clone();
            incoming.providers.providers[0].base_url = "https://example.com/v1".into();
            incoming.dictation.syv_endpoint = "https://example.org/v1".into();
            let store = MemoryKeyStore::default();
            for target in [key_target(&id), key_target(SYV_ID)] {
                store
                    .set(&target, &ApiKey::new("test-key").unwrap())
                    .unwrap();
            }
            let mut preview = current.preview_with_keys(&incoming, mode, &store).unwrap();
            assert_eq!(
                preview.result.providers.providers[0].base_url,
                current.providers.providers[0].base_url
            );
            assert_eq!(
                preview.result.dictation.syv_endpoint,
                current.dictation.syv_endpoint
            );
            assert_eq!(
                preview
                    .changes
                    .iter()
                    .filter(|c| c.warning.as_deref() == Some("kræver ny nøgle"))
                    .count(),
                2
            );
            assert!(store.get(&key_target(&id)).unwrap().is_some());
            assert!(store.get(&key_target(SYV_ID)).unwrap().is_some());
            preview.accept_endpoint_changes(&store).unwrap();
            assert!(store.get(&key_target(&id)).unwrap().is_none());
            assert!(store.get(&key_target(SYV_ID)).unwrap().is_none());
            assert_eq!(
                preview.result.providers.providers[0].base_url,
                incoming.providers.providers[0].base_url
            );
            assert_eq!(
                preview.result.dictation.syv_endpoint,
                incoming.dictation.syv_endpoint
            );
            let without_keys = current.preview_with_keys(&incoming, mode, &store).unwrap();
            assert_eq!(without_keys.result, incoming);
        }
    }

    #[test]
    fn merge_retains_mail_mode_styles_and_local_audio() {
        let mut current = SettingsSnapshot::default();
        let mut mail = current.dictation.modes.modes[0].clone();
        mail.id = "mail".into();
        mail.name = "Mail".into();
        current.dictation.modes.modes.push(mail.clone());
        let style = crate::dictation::settings::AppStyleSetting {
            process: "mail.exe".into(),
            style: "raw".into(),
        };
        current.dictation.app_styles.push(style.clone());
        current.dictation.microphone = "local microphone".into();
        current.dictation.channel = "left".into();
        let incoming = SettingsSnapshot::default();
        let merged = current
            .preview(&incoming, ImportMode::Merge)
            .unwrap()
            .result;
        assert!(merged.dictation.modes.modes.contains(&mail));
        assert!(merged.dictation.app_styles.contains(&style));
        assert_eq!(merged.dictation.microphone, current.dictation.microphone);
        assert_eq!(merged.dictation.channel, "left");
        let replaced = current
            .preview(&incoming, ImportMode::Replace)
            .unwrap()
            .result;
        assert!(!replaced.dictation.modes.modes.contains(&mail));
    }

    #[test]
    fn preview_identifies_provider_fields_and_individual_words() {
        let mut current = SettingsSnapshot::default();
        current.dictionary.push(Word {
            word: "a/b~c".into(),
            aliases: "old".into(),
            english: false,
        });
        let mut incoming = current.clone();
        incoming.dictionary[0].aliases = "new".into();
        incoming.providers.providers[0].base_url = "https://example.com/v1".into();
        let store = crate::providers::secrets::MemoryKeyStore::default();
        let preview = current
            .preview_with_keys(&incoming, ImportMode::Replace, &store)
            .unwrap();
        assert!(preview
            .changes
            .iter()
            .any(|c| c.path == "/dictionary/a~1b~0c/aliases"));
        assert!(preview.changes.iter().any(|c| c.path
            == format!(
                "/providers/providers/{}/baseUrl",
                current.providers.providers[0].id
            )));
        incoming.dictionary.clear();
        assert!(current
            .preview(&incoming, ImportMode::Replace)
            .unwrap()
            .changes
            .iter()
            .any(|c| c.path == "/dictionary/a~1b~0c" && c.after.is_none()));
    }

    #[test]
    fn disabled_dictation_and_merge_conflicts_follow_app_rules() {
        let mut current = SettingsSnapshot::default();
        current.dictation.enabled = false;
        current.shortcuts.ask = Some(current.dictation.shortcut.clone());
        // Use an ordinary key because registered shortcuts cannot be modifiers only.
        current.dictation.shortcut = "Ctrl+Alt+Q".into();
        current.shortcuts.ask = Some("Ctrl+Alt+Q".into());
        assert!(current.export_json().is_ok());
        current.shortcuts.ask = None;
        current.shortcuts.open_window = Some("Ctrl+Alt+Q".into());
        let mut incoming = SettingsSnapshot::default();
        incoming.dictation.enabled = false;
        incoming.shortcuts.ask = Some("Ctrl+Alt+Q".into());
        let error = current.preview(&incoming, ImportMode::Merge).unwrap_err();
        assert!(error.contains("Ctrl+Alt+Q"), "{error}");
        incoming.providers.providers[0].base_url = "https://user:secret@example.com".into();
        let error = incoming.export_json().unwrap_err();
        assert!(error.contains(&incoming.providers.providers[0].id));
        assert!(!error.contains("secret"));
    }

    #[test]
    fn non_normalized_dictation_is_rejected_before_transfer() {
        let current = SettingsSnapshot::default();
        for (field, value) in [
            ("soundVolume", Value::from(500)),
            ("fallbackAfterMs", Value::from(0)),
            ("language", Value::from(" EN ")),
        ] {
            let mut document = serde_json::to_value(&current).unwrap();
            document["dictation"][field] = value;
            let snapshot: SettingsSnapshot = serde_json::from_value(document.clone()).unwrap();
            assert!(snapshot.export_json().is_err(), "{field}");
            assert!(
                SettingsSnapshot::import_json(&document.to_string()).is_err(),
                "{field}"
            );
            for mode in [ImportMode::Merge, ImportMode::Replace] {
                assert!(current.preview(&snapshot, mode).is_err(), "{field}");
                assert!(snapshot.preview(&current, mode).is_err(), "{field}");
            }
        }
    }

    #[test]
    fn preview_dictation_matches_the_values_used_by_save_settings() {
        let current = SettingsSnapshot::default();
        let mut incoming = current.clone();
        incoming.dictation.sound_volume = 500;
        incoming.dictation.fallback_after_ms = 0;
        incoming.dictation = incoming.dictation.normalized().unwrap();
        for mode in [ImportMode::Merge, ImportMode::Replace] {
            let preview = current.preview(&incoming, mode).unwrap();
            let saved = preview.result.dictation.clone().normalized().unwrap();
            assert_eq!(preview.result.dictation, saved);
            let saved = serde_json::to_value(saved).unwrap();
            for field in ["soundVolume", "fallbackAfterMs"] {
                let change = preview
                    .changes
                    .iter()
                    .find(|change| change.path == format!("/dictation/{field}"))
                    .unwrap();
                assert_eq!(change.after.as_ref(), Some(&saved[field]));
            }
        }
    }

    fn keys(value: &Value, expected: &str) {
        let actual: BTreeSet<_> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(actual, expected.split_whitespace().collect());
    }

    #[test]
    fn every_legacy_secret_field_is_excluded() {
        // Populate every column, including all keys and the nested custom key.
        let settings = serde_json::from_value(serde_json::json!({
            "id": "1", "provider": "local", "model": "model", "whisperModel": "tiny",
            "groqApiKey": "SECRET-groq", "openaiApiKey": "SECRET-openai",
            "anthropicApiKey": "SECRET-anthropic", "ollamaApiKey": "SECRET-ollama",
            "openRouterApiKey": "SECRET-router", "ollamaEndpoint": "http://localhost:11434",
            "customOpenAIConfig": "{\"apiKey\":\"SECRET-custom\",\"endpoint\":\"https://example.com/v1\",\"model\":\"model\"}"
        })).unwrap();
        let transcript = serde_json::from_value(serde_json::json!({
            "id": "1", "provider": "local", "model": "tiny",
            "whisperApiKey": "SECRET-whisper", "deepgramApiKey": "SECRET-deepgram",
            "elevenLabsApiKey": "SECRET-eleven", "groqApiKey": "SECRET-groq-transcript",
            "openaiApiKey": "SECRET-openai-transcript"
        }))
        .unwrap();
        let snapshot = SettingsSnapshot {
            settings: MeetingSettings::from_legacy(&settings, &transcript),
            ..SettingsSnapshot::default()
        };
        let json = snapshot.export_json().unwrap();
        assert!(!json.contains("SECRET"));
        assert!(!json.to_lowercase().contains("apikey"));
        let mut document: Value = serde_json::from_str(&json).unwrap();
        document["icsUrl"] = "https://example.com/SECRET-feed.ics".into();
        assert!(SettingsSnapshot::import_json(&document.to_string()).is_err());
    }

    #[test]
    fn portable_fields_are_explicitly_audited() {
        let mut snapshot = SettingsSnapshot::default();
        snapshot.dictionary.push(Word {
            word: "word".into(),
            aliases: "alias".into(),
            english: true,
        });
        snapshot.snippets.push(Snippet {
            phrase: "phrase".into(),
            text: "text".into(),
        });
        snapshot
            .dictation
            .app_styles
            .push(crate::dictation::settings::AppStyleSetting {
                process: "app.exe".into(),
                style: "raw".into(),
            });
        snapshot.providers.providers[0].chat_options = crate::providers::model::ChatOptions {
            max_tokens: Some(100),
            temperature: Some(0.5),
            top_p: Some(0.9),
        };
        let v: Value = serde_json::from_str(&snapshot.export_json().unwrap()).unwrap();
        keys(
            &v,
            "version settings dictation dictionary snippets shortcuts providers",
        );
        keys(
            &v["settings"],
            "provider model whisperModel transcriptProvider transcriptModel",
        );
        keys(&v["dictation"], "enabled shortcut shortcutMode hybridThresholdMs engine syvEndpoint syvModel fallbackAfterMs polishEnabled polishProvider polishModel polishEndpoint polishBudgetMs historyEnabled keepAudio restoreClipboardMs pasteTimeoutMs appStyles modelIdleMinutes keepWarmModels language formatNumbers formatAddresses collapseStutters microphone channel keepMicOpen soundEnabled soundVolume textHookEnabled textHookScript modes");
        keys(&v["dictation"]["appStyles"][0], "process style");
        keys(&v["dictation"]["modes"], "version modes");
        for mode in v["dictation"]["modes"]["modes"].as_array().unwrap() {
            keys(
                mode,
                "id name provider model language style commaStyle polish prompt formatting output rules",
            );
            keys(&mode["formatting"], "numberFormat codeSwitching");
        }
        keys(&v["dictionary"][0], "word aliases english");
        keys(&v["snippets"][0], "phrase text");
        keys(
            &v["shortcuts"],
            "newRecording ask openWindow insertLast rerunLast",
        );
        keys(&v["providers"], "version providers functions");
        for provider in v["providers"]["providers"].as_array().unwrap() {
            let mut fields =
                "id name kind baseUrl transcriptionModel chatModel diarization".to_string();
            if provider.get("chatOptions").is_some() {
                fields.push_str(" chatOptions");
            }
            keys(provider, &fields);
        }
        keys(
            &v["providers"]["providers"][0]["chatOptions"],
            "maxTokens temperature topP",
        );
        let link = crate::providers::model::ChainLink::new("local")
            .with_model("model")
            .with_timeout_ms(1000);
        keys(
            &serde_json::to_value(link).unwrap(),
            "provider model timeoutMs",
        );
        use crate::dictation::modes::{Output, Rule};
        keys(&serde_json::to_value(Output::Insert).unwrap(), "kind");
        keys(
            &serde_json::to_value(Output::InsertAndEnter {
                trigger: "send".into(),
            })
            .unwrap(),
            "kind trigger",
        );
        for rule in [
            Rule::Program("app.exe".into()),
            Rule::WindowClass("editor".into()),
            Rule::Website("example.com".into()),
            Rule::TriggerWord("write".into()),
            Rule::Shortcut("Ctrl+Alt+A".into()),
        ] {
            keys(&serde_json::to_value(rule).unwrap(), "kind value");
        }
    }

    #[test]
    fn merge_preserves_local_entries_while_replace_removes_them() {
        use crate::providers::model::{local_provider, ChainLink, Function};
        let mut current = SettingsSnapshot::default();
        let mut local = local_provider();
        local.id = "extra".into();
        current.providers.providers.push(local);
        current
            .providers
            .functions
            .insert(Function::Summary, vec![ChainLink::new("extra")]);
        current.shortcuts.open_window = Some("Ctrl+Alt+O".into());
        current.snippets.push(Snippet {
            phrase: "old".into(),
            text: "old text".into(),
        });
        let mut incoming = SettingsSnapshot::default();
        incoming.providers.functions.remove(&Function::Summary);
        incoming.snippets.push(Snippet {
            phrase: "new".into(),
            text: "new text".into(),
        });
        incoming.dictation.sound_enabled = true;
        let merged = current
            .preview(&incoming, ImportMode::Merge)
            .unwrap()
            .result;
        assert!(merged.providers.provider("extra").is_some());
        assert_eq!(
            merged.providers.chain(Function::Summary)[0].provider,
            "extra"
        );
        assert_eq!(merged.snippets.len(), 2);
        assert_eq!(merged.shortcuts.open_window, current.shortcuts.open_window);
        assert!(merged.dictation.sound_enabled);
        let replaced = current
            .preview(&incoming, ImportMode::Replace)
            .unwrap()
            .result;
        assert_eq!(replaced, incoming);
        assert!(replaced.providers.provider("extra").is_none());
        assert!(replaced.shortcuts.open_window.is_none());
    }

    #[test]
    fn nested_injected_keys_do_not_survive_import() {
        let mut value = serde_json::to_value(SettingsSnapshot::default()).unwrap();
        value["dictation"]["apiKey"] = "SECRET-dictation".into();
        value["providers"]["providers"][0]["apiKey"] = "SECRET-provider".into();
        value["providers"]["providers"][0]["icsUrl"] = "SECRET-feed".into();
        let imported = SettingsSnapshot::import_json(&value.to_string()).unwrap();
        assert!(!imported.export_json().unwrap().contains("SECRET"));
    }

    #[test]
    fn conflicts_use_incoming_values_and_diff_escapes_keys() {
        let mut current = SettingsSnapshot::default();
        current.shortcuts.ask = Some("Ctrl+A".into());
        current.dictionary.push(Word {
            word: "Word".into(),
            aliases: "old".into(),
            english: false,
        });
        let mut incoming = current.clone();
        incoming.shortcuts.ask = Some("Ctrl+B".into());
        incoming.dictionary[0].word = "word".into();
        incoming.dictionary[0].aliases = "new".into();
        incoming.providers.providers[0].name = "Renamed".into();
        let preview = current.preview(&incoming, ImportMode::Merge).unwrap();
        assert_eq!(preview.result.dictionary.len(), 1);
        assert_eq!(preview.result.dictionary[0].aliases, "new");
        assert_eq!(preview.result.providers.providers[0].name, "Renamed");
        assert!(preview.changes.iter().any(|c| c.path == "/shortcuts/ask"
            && c.before == Some(Value::from("Ctrl+A"))
            && c.after == Some(Value::from("Ctrl+B"))));
        let mut changes = Vec::new();
        diff(
            "",
            Some(&serde_json::json!({"a/b~c": 1})),
            Some(&serde_json::json!({"a/b~c": 2})),
            &mut changes,
        );
        assert_eq!(changes[0].path, "/a~1b~0c");
        assert!(current
            .preview(&current, ImportMode::Merge)
            .unwrap()
            .changes
            .is_empty());
        incoming.dictionary.push(incoming.dictionary[0].clone());
        assert!(incoming.export_json().is_err());
    }

    #[test]
    fn round_trip_and_unknown_versions() {
        let snapshot = SettingsSnapshot::default();
        let json = snapshot.export_json().unwrap();
        assert_eq!(SettingsSnapshot::import_json(&json).unwrap(), snapshot);
        let json = json.replace("\"version\": 1", "\"version\": 999");
        assert!(SettingsSnapshot::import_json(&json).is_err());
        for nested in ["providers", "modes"] {
            let mut snapshot = SettingsSnapshot::default();
            if nested == "providers" {
                snapshot.providers.version = 999;
            } else {
                snapshot.dictation.modes.version = 999;
            }
            assert!(snapshot.export_json().is_err());
            let json = serde_json::to_string(&snapshot).unwrap();
            assert!(SettingsSnapshot::import_json(&json).is_err());
        }
    }

    #[test]
    fn invalid_shortcuts_and_secret_inputs_are_rejected() {
        let mut snapshot = SettingsSnapshot::default();
        snapshot.shortcuts.ask = Some("invalid shortcut".into());
        assert!(snapshot.export_json().is_err());
        assert!(!SettingsSnapshot::import_json("SECRET malformed input")
            .unwrap_err()
            .contains("SECRET"));
    }

    #[test]
    fn merge_replace_and_preview_are_consistent() {
        let mut current = SettingsSnapshot::default();
        current.shortcuts.new_recording = Some("Ctrl+R".into());
        current.dictionary.push(Word {
            word: "old".into(),
            aliases: "".into(),
            english: false,
        });
        let mut incoming = SettingsSnapshot::default();
        incoming.shortcuts.ask = Some("Ctrl+D".into());
        incoming.dictionary.push(Word {
            word: "new".into(),
            aliases: "".into(),
            english: true,
        });
        for mode in [ImportMode::Merge, ImportMode::Replace] {
            let preview = current.preview(&incoming, mode).unwrap();
            assert!(!preview.changes.is_empty());
            assert_eq!(
                preview.result.dictionary.len(),
                if mode == ImportMode::Merge { 2 } else { 1 }
            );
            assert_eq!(current.dictionary[0].word, "old");
        }
    }

    #[test]
    fn endpoint_credentials_are_never_exported_or_previewed() {
        for url in [
            "https://user:secret@example.com/v1",
            "https://example.com/v1?token=secret",
            "https://example.com/v1#secret",
        ] {
            let mut snapshot = SettingsSnapshot::default();
            snapshot.providers.providers[0].base_url = url.into();
            assert!(snapshot.export_json().is_err());
            assert!(SettingsSnapshot::default()
                .preview(&snapshot, ImportMode::Replace)
                .is_err());
        }
    }
}
