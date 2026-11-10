//! Explicitly approved, durable exports. No transport runs during configuration.
pub mod meeting;
mod store;
mod transport;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
pub use store::{Delivery, JobStatus};

pub const SIDECAR_SCHEMA: &str = include_str!("../../../../docs/schemas/delivery-v1.schema.json");

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    pub tekst: String,
    pub ansvarlig: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sidecar {
    pub kilde: String,
    pub skemaversion: u32,
    pub titel: String,
    pub tid: DateTime<Utc>,
    pub talere: Vec<String>,
    pub opgaver: Vec<Task>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bundle {
    pub sidecar: Sidecar,
    pub transcript: String,
    pub summary: String,
    pub notes: String,
}

impl Bundle {
    pub fn validate(&self) -> Result<(), Error> {
        if self.sidecar.kilde != "tingskriver"
            || self.sidecar.skemaversion != 1
            || self.sidecar.titel.trim().is_empty()
            || self.summary.trim().is_empty()
            || serde_json::to_vec(self).map_err(|_| Error::Invalid)?.len() > 16 * 1024 * 1024
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }

    pub fn test_sample() -> Self {
        Self {
            sidecar: Sidecar {
                kilde: "tingskriver".into(),
                skemaversion: 1,
                titel: "Test fra Tingskriver".into(),
                tid: Utc::now(),
                talere: vec![],
                opgaver: vec![],
            },
            transcript: "Dette er en test uden mødedata.".into(),
            summary: "Dette er en test uden mødedata.".into(),
            notes: String::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebhookFormat {
    Json,
    Slack,
    Discord,
    Teams,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Destination {
    Folder {
        path: PathBuf,
    },
    /// The address (including its token) is stored in KeyStore, never here.
    Webhook {
        format: WebhookFormat,
    },
    /// SFTP uses OpenSSH without a remote shell. Only a key file reference.
    Ssh {
        host: String,
        port: u16,
        user: String,
        key_path: PathBuf,
        remote_dir: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub id: String,
    pub name: String,
    pub destination: Destination,
}

pub(crate) fn safe_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
}

impl Target {
    pub fn validate(&self) -> Result<(), Error> {
        if !safe_id(&self.id) || self.name.trim().is_empty() {
            return Err(Error::Invalid);
        }
        match &self.destination {
            Destination::Folder { path } if !path.is_absolute() => return Err(Error::Invalid),
            Destination::Ssh {
                host,
                port,
                user,
                key_path,
                remote_dir,
            } if host.is_empty()
                || host.starts_with('-')
                || !host
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-' | b':'))
                || !safe_id(user)
                || user.starts_with('-')
                || *port == 0
                || !key_path.is_absolute()
                || !remote_dir.starts_with('/')
                || remote_dir.split('/').any(|s| s == "..")
                || !remote_dir.bytes().all(|c| {
                    c.is_ascii_alphanumeric() || matches!(c, b'/' | b'_' | b'-' | b'.')
                }) =>
            {
                return Err(Error::Invalid)
            }
            _ => {}
        }
        Ok(())
    }
}

/// Missing policy is deliberately closed. A host must explicitly enable delivery.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub local_only: bool,
    pub private: bool,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            local_only: true,
            private: false,
        }
    }
}
impl Policy {
    pub(crate) fn check(self) -> Result<(), Error> {
        if self.local_only || self.private {
            Err(Error::Blocked)
        } else {
            Ok(())
        }
    }
}

/// Fixed messages only: never expose URLs, SSH stderr, response bodies or content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("Aflevering er blokeret af kun lokal eller privat tilstand")]
    Blocked,
    #[error("Godkend referatet før aflevering")]
    Approval,
    #[error("Afleveringsdata eller mål er ugyldige")]
    Invalid,
    #[error("Afleveringsmålet findes ikke")]
    Missing,
    #[error("Afleveringskøen kunne ikke læses eller gemmes")]
    Storage,
    #[error("Webhook-adressen kunne ikke læses eller gemmes i nøglelageret")]
    Credential,
    #[error("Aflevering mislykkedes. Kontrollér mål og adgang")]
    Transport,
}

fn clipped(text: &str, limit: usize) -> String {
    let mut used = 0;
    text.chars()
        .take_while(|c| {
            used += c.len_utf16();
            used <= limit
        })
        .collect()
}

pub fn webhook_payload(format: WebhookFormat, bundle: &Bundle, id: &str) -> Value {
    let text = format!("{}\n\n{}", bundle.sidecar.titel, bundle.summary);
    match format {
        WebhookFormat::Json => json!({"delivery_id": id, "sidecar": bundle.sidecar,
            "transcript": bundle.transcript, "summary": bundle.summary, "notes": bundle.notes}),
        WebhookFormat::Slack => {
            json!({"text": clipped(&text, 4000), "unfurl_links": false, "unfurl_media": false})
        }
        WebhookFormat::Discord => {
            json!({"content": clipped(&text, 2000), "allowed_mentions": {"parse": []}})
        }
        // A conservative cap also bounds JSON escaping below Teams' 28 KB limit.
        WebhookFormat::Teams => json!({"text": clipped(&text, 4000)}),
    }
}
