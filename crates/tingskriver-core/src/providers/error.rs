//! Classified provider errors with plain Danish explanations. No variant
//! carries the API key; details from outside pass through
//! [`super::secrets::redact`] first.

use std::fmt;
use std::time::Duration;

use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ProviderError {
    /// No key in Credential Manager for a provider that needs one.
    MissingKey,
    /// 401 or 403: the key is missing or invalid.
    Unauthorized { status: u16 },
    /// 404: the route or the model does not exist.
    NotFound,
    /// 429, with the wait from `Retry-After` when the server sent one.
    RateLimited { retry_after_secs: Option<u64> },
    /// No (complete) answer within the time limit, including a connection
    /// that was accepted and then hung.
    Timeout { after_ms: u64 },
    /// The server could not be reached at all.
    Unreachable { detail: String },
    /// Any other HTTP error status.
    Http { status: u16 },
    /// The answer was not what an OpenAI-compatible API returns.
    BadResponse { detail: String },
    /// The configuration cannot be used (unknown provider, local provider
    /// called over HTTP, ...).
    Config { detail: String },
    /// The local model failed.
    Local { detail: String },
}

impl ProviderError {
    pub fn timeout(after: Duration) -> ProviderError {
        ProviderError::Timeout {
            after_ms: after.as_millis() as u64,
        }
    }

    pub fn local(detail: impl fmt::Display) -> ProviderError {
        ProviderError::Local {
            detail: detail.to_string(),
        }
    }

    /// The explanation shown to the user.
    pub fn message(&self, provider: &str) -> String {
        match self {
            ProviderError::MissingKey => format!(
                "Der mangler en API-nøgle til {provider}. Indtast nøglen under Indstillinger, Udbydere."
            ),
            ProviderError::Unauthorized { status } => format!(
                "{provider} afviste API-nøglen ({status}). Nøglen mangler eller er ugyldig."
            ),
            ProviderError::NotFound => format!(
                "{provider} kender ikke adressen (404). Tjek base-URL og model-id."
            ),
            ProviderError::RateLimited { retry_after_secs: Some(secs) } => format!(
                "{provider} har fået for mange kald (429). Prøv igen om {secs} sekunder."
            ),
            ProviderError::RateLimited { retry_after_secs: None } => {
                format!("{provider} har fået for mange kald (429). Prøv igen om lidt.")
            }
            ProviderError::Timeout { after_ms } => format!(
                "{provider} svarede ikke inden for tidsgrænsen på {}.",
                format_duration(*after_ms)
            ),
            ProviderError::Unreachable { detail } => {
                format!("{provider} kunne ikke nås. Tjek adressen og netværket ({detail}).")
            }
            ProviderError::Http { status } => format!("{provider} svarede med fejl {status}."),
            ProviderError::BadResponse { detail } => format!("Uventet svar fra {provider}: {detail}"),
            ProviderError::Config { detail } => format!("{provider} kan ikke bruges: {detail}"),
            ProviderError::Local { detail } => format!("{provider} fejlede: {detail}"),
        }
    }
}

fn format_duration(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms} ms")
    } else if ms % 1000 == 0 {
        format!("{} s", ms / 1000)
    } else {
        format!("{:.1} s", ms as f64 / 1000.0).replace('.', ",")
    }
}

/// An error together with the provider it came from, ready to show.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderFailure {
    pub provider_id: String,
    pub provider_name: String,
    pub error: ProviderError,
    pub message: String,
}

impl ProviderFailure {
    pub fn new(provider_id: &str, provider_name: &str, error: ProviderError) -> ProviderFailure {
        ProviderFailure {
            provider_id: provider_id.to_string(),
            provider_name: provider_name.to_string(),
            message: error.message(provider_name),
            error,
        }
    }
}

impl fmt::Display for ProviderFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ProviderFailure {}

/// `Retry-After` as seconds: either a number or an HTTP date.
pub fn parse_retry_after(value: &str, now: chrono::DateTime<chrono::Utc>) -> Option<u64> {
    let value = value.trim();
    if let Ok(secs) = value.parse::<u64>() {
        return Some(secs);
    }
    let when = chrono::DateTime::parse_from_rfc2822(value).ok()?;
    let secs = (when.with_timezone(&chrono::Utc) - now).num_seconds();
    Some(secs.max(0) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn danish_messages_per_class() {
        let p = "Syv.ai";
        assert!(ProviderError::Unauthorized { status: 401 }
            .message(p)
            .contains("Nøglen mangler eller er ugyldig"));
        assert!(ProviderError::NotFound.message(p).contains("404"));
        assert_eq!(
            ProviderError::RateLimited {
                retry_after_secs: Some(30)
            }
            .message(p),
            "Syv.ai har fået for mange kald (429). Prøv igen om 30 sekunder."
        );
        assert_eq!(
            ProviderError::timeout(Duration::from_millis(2000)).message(p),
            "Syv.ai svarede ikke inden for tidsgrænsen på 2 s."
        );
        assert!(ProviderError::timeout(Duration::from_millis(1500))
            .message(p)
            .contains("1,5 s"));
        assert!(ProviderError::MissingKey
            .message(p)
            .contains("mangler en API-nøgle"));
    }

    #[test]
    fn retry_after_seconds_and_dates() {
        let now = chrono::DateTime::parse_from_rfc2822("Sat, 31 Oct 2026 10:00:00 GMT")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert_eq!(parse_retry_after("120", now), Some(120));
        assert_eq!(
            parse_retry_after("Sat, 31 Oct 2026 10:01:30 GMT", now),
            Some(90)
        );
        assert_eq!(
            parse_retry_after("Sat, 31 Oct 2026 09:00:00 GMT", now),
            Some(0)
        );
        assert_eq!(parse_retry_after("snart", now), None);
    }

    #[test]
    fn serializes_with_a_kind_tag() {
        let json = serde_json::to_string(&ProviderError::RateLimited {
            retry_after_secs: Some(5),
        })
        .unwrap();
        assert_eq!(json, r#"{"kind":"rate-limited","retry_after_secs":5}"#);
    }
}
