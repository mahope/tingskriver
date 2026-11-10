//! Connection verification uses its own client: a caller's redirect policy
//! must never forward a key. Only custom keyed gateways need a negative control.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tokio::time::Instant;

use reqwest::{redirect::Policy, StatusCode, Url};

use super::client::{classify_status, ResolvedLink, TestReport};
use super::error::ProviderError;
use super::model::ProviderKind;

const TIMEOUT: Duration = Duration::from_secs(8);
const CACHE_TTL: Duration = Duration::from_secs(60);
type Cached = (Instant, Result<TestReport, ProviderError>);
static CACHE: OnceLock<Mutex<HashMap<u64, Cached>>> = OnceLock::new();

fn unverifiable(detail: &str) -> ProviderError {
    ProviderError::BadResponse {
        detail: format!("forbindelsen kan ikke verificeres: {detail}"),
    }
}

fn network_error(error: reqwest::Error, timeout: Duration) -> ProviderError {
    if error.is_timeout() {
        ProviderError::timeout(timeout)
    } else {
        unverifiable("netværksfejl eller ugyldigt svar")
    }
}

fn models_url(base: &str) -> Result<Url, ProviderError> {
    let invalid = || ProviderError::Config {
        detail: "brug HTTPS (HTTP kun til loopback) og en URL uden brugernavn eller adgangskode"
            .into(),
    };
    let mut url = Url::parse(base).map_err(|_| invalid())?;
    let loopback = url.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    if !url.username().is_empty()
        || url.password().is_some()
        || !(url.scheme() == "https" || (url.scheme() == "http" && loopback))
        || url.host_str().is_none()
    {
        return Err(invalid());
    }
    url.set_path(&format!("{}/models", url.path().trim_end_matches('/')));
    Ok(url)
}

pub(super) async fn test(link: &ResolvedLink) -> Result<TestReport, ProviderError> {
    let url = models_url(&link.base_url)?;
    // Keep only a process-local fingerprint, never the credential itself.
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    (
        url.as_str(),
        link.kind,
        link.key.as_ref().map(|key| key.expose()),
        link.remote_timeout(),
    )
        .hash(&mut hash);
    let cache_key = hash.finish();
    let cache = CACHE.get_or_init(Mutex::default);
    if let Ok(mut entries) = cache.lock() {
        entries.retain(|_, (at, _)| at.elapsed() < CACHE_TTL);
        if let Some((_, result)) = entries.get(&cache_key) {
            return result.clone();
        }
    }
    let timeout = TIMEOUT.min(link.remote_timeout());
    let work = async {
        let client = reqwest::Client::builder()
            .redirect(Policy::none())
            .timeout(timeout)
            .build()
            .map_err(|_| unverifiable("HTTP-klienten kunne ikke startes"))?;
        let start = Instant::now();
        let mut request = client.get(url.clone());
        if let Some(key) = &link.key {
            request = request.bearer_auth(key.expose());
        }
        let response = request
            .send()
            .await
            .map_err(|error| network_error(error, timeout))?;
        check(&response)?;
        let raw: serde_json::Value = response
            .json()
            .await
            .map_err(|error| network_error(error, timeout))?;
        let models = raw
            .get("data")
            .and_then(|data| data.as_array())
            .ok_or_else(|| unverifiable("svaret indeholdt ingen modelliste"))?
            .len();
        if link.kind == ProviderKind::OpenAiCompatible && link.key.is_some() {
            let control = format!("tingskriver-invalid-{}", uuid::Uuid::new_v4());
            let response = client
                .get(url)
                .bearer_auth(control)
                .send()
                .await
                .map_err(|error| network_error(error, timeout))?;
            match response.status() {
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {}
                StatusCode::TOO_MANY_REQUESTS => {
                    check(&response)?;
                }
                status if status.is_redirection() => {
                    check(&response)?;
                }
                _ => {
                    return Err(unverifiable(
                        "gatewayen skal afvise kontrolnøglen med 401 eller 403",
                    ))
                }
            }
        }
        Ok(TestReport {
            latency_ms: start.elapsed().as_millis() as u64,
            models,
        })
    };
    let result = tokio::time::timeout(timeout, work)
        .await
        .unwrap_or_else(|_| Err(ProviderError::timeout(timeout)));
    if let Ok(mut entries) = cache.lock() {
        entries.insert(cache_key, (Instant::now(), result.clone()));
    }
    result
}

fn check(response: &reqwest::Response) -> Result<(), ProviderError> {
    if response.status().is_redirection() {
        return Err(unverifiable(
            "omdirigering er ikke tilladt; nøglen er ikke sendt videre",
        ));
    }
    if response.status().is_success() {
        return Ok(());
    }
    let error = classify_status(
        response.status(),
        response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok()),
    );
    match error {
        ProviderError::Unauthorized { .. } | ProviderError::RateLimited { .. } => Err(error),
        _ => Err(unverifiable("serveren returnerede en HTTP-fejl")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_only_https_or_loopback_http_without_credentials() {
        for url in [
            "https://example.com/v1",
            "http://localhost/v1",
            "http://127.0.0.2/v1",
            "http://[::1]/v1",
        ] {
            assert!(models_url(url).is_ok(), "{url}");
        }
        for url in [
            "http://example.com/v1",
            "http://localhost.example.com/v1",
            "https://user@example.com/v1",
            "https://:password@example.com/v1",
            "file:///v1",
            "not a URL",
        ] {
            assert!(models_url(url).is_err(), "{url}");
        }
    }
}
