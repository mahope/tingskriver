//! Load and save the provider configuration (`providers.json` in the app data
//! folder), migrate the old settings the first time, and turn a function's
//! chain into callable links with their keys.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use sqlx::SqlitePool;

use super::client::{self, ResolvedLink, TestReport};
use super::error::{ProviderError, ProviderFailure};
use super::migrate;
use super::model::{key_target, Capability, Function, Provider, ProvidersConfig, CONFIG_VERSION};
use super::secrets::{ApiKey, CredentialManager, KeyStore};
use crate::dictation::settings::DictationSettings;
use crate::host::HostHandle;

pub const CONFIG_FILE: &str = "providers.json";
const TEST_TIMEOUT: Duration = Duration::from_secs(15);

/// One migration at a time, so two first calls do not both migrate.
static MIGRATION_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// A provider as the settings UI shows it.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    #[serde(flatten)]
    pub provider: Provider,
    pub has_key: bool,
    pub requires_key: bool,
}

#[derive(Clone)]
pub struct ProviderService {
    dir: PathBuf,
    keys: Arc<dyn KeyStore>,
}

impl ProviderService {
    pub fn new(app_data_dir: &Path, keys: Arc<dyn KeyStore>) -> ProviderService {
        ProviderService {
            dir: app_data_dir.to_path_buf(),
            keys,
        }
    }

    /// The service for the running app: keys in Credential Manager.
    pub fn for_host(host: &HostHandle) -> Result<ProviderService, String> {
        Ok(ProviderService::new(
            &host.app_data_dir()?,
            Arc::new(CredentialManager),
        ))
    }

    pub fn path(&self) -> PathBuf {
        self.dir.join(CONFIG_FILE)
    }

    /// `Ok(None)` when there is no file yet. A file that cannot be read is an
    /// error, never silently replaced by defaults.
    pub fn load(&self) -> Result<Option<ProvidersConfig>, String> {
        let path = self.path();
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("Udbyderindstillingerne kunne ikke læses: {e}")),
        };
        let config: ProvidersConfig = serde_json::from_str(&text).map_err(|e| {
            format!(
                "Udbyderindstillingerne i {} er ødelagte: {e}",
                path.display()
            )
        })?;
        if config.version > CONFIG_VERSION {
            return Err(format!(
                "Udbyderindstillingerne er fra en nyere version af Tingskriver (version {})",
                config.version
            ));
        }
        Ok(Some(config))
    }

    /// Validate and write atomically.
    pub fn save(&self, config: &ProvidersConfig) -> Result<(), String> {
        config.validate()?;
        let json = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&self.dir)
            .map_err(|e| format!("Mappen kunne ikke oprettes: {e}"))?;
        let tmp = self.dir.join(format!("{CONFIG_FILE}.tmp"));
        std::fs::write(&tmp, json)
            .map_err(|e| format!("Udbyderindstillingerne kunne ikke gemmes: {e}"))?;
        std::fs::rename(&tmp, self.path())
            .map_err(|e| format!("Udbyderindstillingerne kunne ikke gemmes: {e}"))
    }

    /// The saved configuration, or on first use the migrated old settings.
    /// Without a database (before onboarding) the defaults are returned and
    /// nothing is written, so the migration still runs once it exists.
    pub async fn load_or_migrate(
        &self,
        pool: Option<&SqlitePool>,
        dictation: Option<&DictationSettings>,
    ) -> Result<ProvidersConfig, String> {
        if let Some(config) = self.load()? {
            return Ok(config);
        }
        let _guard = MIGRATION_LOCK.lock().await;
        if let Some(config) = self.load()? {
            return Ok(config);
        }
        let Some(pool) = pool else {
            return Ok(ProvidersConfig::default());
        };

        let mut legacy = migrate::read_legacy(pool).await?;
        if let Err(e) = migrate::fill_from_store(&mut legacy, self.keys.as_ref()) {
            // A key saved by the old settings code would be missed; try again next time.
            log::warn!("Providers: migration postponed: {e}");
            return Ok(migrate::plan(&legacy, dictation).config);
        }
        let plan = migrate::plan(&legacy, dictation);
        if let Err(e) = plan.config.validate() {
            // plan drops invalid entries, so this should not happen. Never
            // fall back to the defaults, which send audio to Syv.ai.
            log::error!("Providers: migrated settings invalid ({e}); using the local model only");
            let mut config = ProvidersConfig::default();
            for chain in config.functions.values_mut() {
                chain.retain(|link| link.provider == super::model::LOCAL_ID);
            }
            return Ok(config);
        }
        if let Err(e) = migrate::apply(&plan, self.keys.as_ref(), Some(pool), &legacy).await {
            // Keys stay in the old columns; try again next time.
            log::warn!("Providers: migration postponed: {e}");
            return Ok(plan.config);
        }
        self.save(&plan.config)?;
        log::info!(
            "Providers: migrated old settings ({} provider(s), {} key(s) moved to Credential Manager)",
            plan.config.providers.len(),
            plan.keys.len()
        );
        for note in &plan.notes {
            log::info!("Providers: {note}");
        }
        Ok(plan.config)
    }

    /// Like [`ProviderService::load_or_migrate`] with the host's database and
    /// dictation settings.
    pub async fn load_for_host(&self, host: &HostHandle) -> Result<ProvidersConfig, String> {
        if let Some(config) = self.load()? {
            return Ok(config);
        }
        let Some(database) = host.database() else {
            return Ok(ProvidersConfig::default());
        };
        let dictation = dictation_settings(&self.dir).await;
        self.load_or_migrate(Some(database.pool()), dictation.as_ref())
            .await
    }

    pub fn key(&self, provider_id: &str) -> Option<ApiKey> {
        match self.keys.get(&key_target(provider_id)) {
            Ok(key) => key,
            Err(e) => {
                log::warn!("Providers: could not read the key for {provider_id}: {e}");
                None
            }
        }
    }

    pub fn has_key(&self, provider_id: &str) -> bool {
        self.key(provider_id).is_some()
    }

    pub fn set_key(&self, provider_id: &str, key: &str) -> Result<(), String> {
        let key = ApiKey::new(key).ok_or_else(|| "Nøglen er tom".to_string())?;
        self.keys.set(&key_target(provider_id), &key)
    }

    pub fn delete_key(&self, provider_id: &str) -> Result<(), String> {
        self.keys.delete(&key_target(provider_id))
    }

    pub fn statuses(&self, config: &ProvidersConfig) -> Vec<ProviderStatus> {
        config
            .providers
            .iter()
            .map(|p| ProviderStatus {
                provider: p.clone(),
                has_key: p.kind.is_remote() && self.has_key(&p.id),
                requires_key: p.kind.requires_key(),
            })
            .collect()
    }

    /// The function's chain with models and keys filled in. Links to unknown
    /// providers are left out (and logged).
    pub fn resolve(&self, config: &ProvidersConfig, function: Function) -> Vec<ResolvedLink> {
        let capability = function.capability();
        config
            .chain(function)
            .iter()
            .filter_map(|link| {
                let Some(provider) = config.provider(&link.provider) else {
                    log::warn!(
                        "Providers: {function:?} names unknown provider {}",
                        link.provider
                    );
                    return None;
                };
                let model = link
                    .model
                    .clone()
                    .filter(|m| !m.trim().is_empty())
                    .or_else(|| provider.default_model(capability).map(str::to_string))
                    .unwrap_or_default();
                Some(ResolvedLink {
                    function,
                    provider_id: provider.id.clone(),
                    provider_name: provider.name.clone(),
                    kind: provider.kind,
                    base_url: provider.base_url.clone(),
                    model,
                    key: provider
                        .kind
                        .is_remote()
                        .then(|| self.key(&provider.id))
                        .flatten(),
                    timeout: link.timeout_ms.map(Duration::from_millis),
                    diarization: provider.diarization,
                    chat_options: provider.chat_options.clone(),
                })
            })
            .collect()
    }

    /// The test button: a real, small call to the provider with the measured
    /// round trip, or a classified Danish error.
    pub async fn test(
        &self,
        config: &ProvidersConfig,
        provider_id: &str,
    ) -> Result<TestReport, ProviderFailure> {
        let Some(provider) = config.provider(provider_id) else {
            return Err(ProviderFailure::new(
                provider_id,
                provider_id,
                ProviderError::Config {
                    detail: "udbyderen findes ikke".to_string(),
                },
            ));
        };
        let fail = |error| ProviderFailure::new(&provider.id, &provider.name, error);
        if !provider.kind.is_remote() {
            return Err(fail(ProviderError::Config {
                detail: "den lokale model testes ved at bruge den".to_string(),
            }));
        }
        let key = self.key(&provider.id);
        if provider.kind.requires_key() && key.is_none() {
            return Err(fail(ProviderError::MissingKey));
        }
        let link = ResolvedLink {
            function: Function::Assistant,
            provider_id: provider.id.clone(),
            provider_name: provider.name.clone(),
            kind: provider.kind,
            base_url: provider.base_url.clone(),
            model: provider
                .default_model(Capability::Chat)
                .unwrap_or_default()
                .to_string(),
            key,
            timeout: Some(TEST_TIMEOUT),
            diarization: provider.diarization,
            chat_options: provider.chat_options.clone(),
        };
        client::test(&client::http_client(), &link)
            .await
            .map_err(fail)
    }
}

/// The dictation settings, if dictation has a database yet. Read only for the
/// one-time migration.
async fn dictation_settings(dir: &Path) -> Option<DictationSettings> {
    let path = dir.join("dictation.sqlite");
    if !path.exists() {
        return None;
    }
    match crate::dictation::store::Store::open(&path).await {
        Ok(store) => store.settings().await.ok(),
        Err(e) => {
            log::warn!("Providers: dictation settings unavailable for migration: {e}");
            None
        }
    }
}
