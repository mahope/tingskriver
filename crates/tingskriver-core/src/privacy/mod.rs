//! Configuration-derived privacy decisions, independent of UI and scheduling.
pub mod flows;
pub mod retention;

use crate::providers::{secrets::CredentialManager, ProviderService, ProvidersConfig};
use std::{path::Path, sync::Arc};

pub fn local_only(settings: &ProvidersConfig) -> bool {
    settings.local_only
}

/// Read the same persisted switch used by the provider API. Invalid settings
/// propagate as errors so older call paths cannot silently enable the network.
pub async fn local_only_in(dir: &Path) -> Result<bool, String> {
    let service = ProviderService::new(dir, Arc::new(CredentialManager));
    tokio::task::spawn_blocking(move || {
        service
            .load()
            .map(|config| config.as_ref().is_some_and(local_only))
    })
    .await
    .map_err(|_| "Privatlivsindstillingerne kunne ikke læses".to_string())?
}

pub async fn check_network(dir: &Path) -> Result<(), String> {
    if local_only_in(dir).await? {
        log::info!("Privacy: skipped network call (local only)");
        Err(crate::strings::LOCAL_ONLY_ENABLED.to_string())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn invalid_settings_never_allow_network() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!local_only_in(dir.path()).await.unwrap());
        tokio::fs::write(
            dir.path().join(crate::providers::service::CONFIG_FILE),
            b"invalid",
        )
        .await
        .unwrap();
        assert!(check_network(dir.path()).await.is_err());
    }
}
