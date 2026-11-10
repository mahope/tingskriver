//! Connection verification uses its own client: a caller's redirect policy
//! must never forward a key. Only custom keyed gateways need a negative control.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use reqwest::{redirect::Policy, StatusCode, Url};

use super::client::{classify_status, map_reqwest, ResolvedLink, TestReport};
use super::error::ProviderError;
use super::model::ProviderKind;

const TIMEOUT: Duration = Duration::from_secs(8);
const CACHE_TTL: Duration = Duration::from_secs(60);
type Cached = (Instant, TestReport);
pub(super) type Cache = Mutex<HashMap<u64, Cached>>;
static CACHE: OnceLock<Cache> = OnceLock::new();

fn unverifiable(detail: &str) -> ProviderError {
    ProviderError::BadResponse {
        detail: format!("forbindelsen kan ikke verificeres: {detail}"),
    }
}

fn private_host(url: &Url) -> bool {
    url.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host.to_ascii_lowercase().ends_with(".local")
            || host
                .trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| match ip {
                    std::net::IpAddr::V4(ip) => {
                        ip.is_loopback() || ip.is_private() || ip.is_link_local()
                    }
                    std::net::IpAddr::V6(ip) => {
                        ip.is_loopback()
                            || (ip.segments()[0] & 0xfe00) == 0xfc00
                            || (ip.segments()[0] & 0xffc0) == 0xfe80
                            || ip.to_ipv4_mapped().is_some_and(|ip| {
                                ip.is_loopback() || ip.is_private() || ip.is_link_local()
                            })
                    }
                })
    })
}

fn key_warning(url: &Url) -> String {
    if private_host(url) {
        "Serveren tjekker ikke nøglen.".into()
    } else {
        "Advarsel: Den offentlige gateway tjekker ikke nøglen. Kontrollér adressen, før du sender data.".into()
    }
}

fn models_url(base: &str) -> Result<Url, ProviderError> {
    let invalid = || {
        ProviderError::Config {
        detail: "brug HTTPS til offentlige værter (HTTP kun til lokale og private adresser) og en URL uden brugernavn eller adgangskode"
            .into(),
    }
    };
    let mut url = Url::parse(base).map_err(|_| invalid())?;
    if !url.username().is_empty()
        || url.password().is_some()
        || !(url.scheme() == "https" || (url.scheme() == "http" && private_host(&url)))
        || url.host_str().is_none()
    {
        return Err(invalid());
    }
    url.set_path(&format!("{}/models", url.path().trim_end_matches('/')));
    Ok(url)
}

pub(super) async fn test(link: &ResolvedLink) -> Result<TestReport, ProviderError> {
    test_with_cache(link, CACHE.get_or_init(Mutex::default), Instant::now).await
}

pub(super) async fn test_with_cache(
    link: &ResolvedLink,
    cache: &Cache,
    now: impl Fn() -> Instant,
) -> Result<TestReport, ProviderError> {
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
    if let Ok(mut entries) = cache.lock() {
        entries.retain(|_, (at, _)| now().saturating_duration_since(*at) < CACHE_TTL);
        if let Some((_, result)) = entries.get(&cache_key) {
            return Ok(result.clone());
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
            .map_err(|error| map_reqwest(error, timeout, link))?;
        check(&response)?;
        let raw: serde_json::Value = response
            .json()
            .await
            .map_err(|error| map_reqwest(error, timeout, link))?;
        let models = raw
            .get("data")
            .and_then(|data| data.as_array())
            .ok_or_else(|| unverifiable("svaret indeholdt ingen modelliste"))?
            .len();
        let latency_ms = start.elapsed().as_millis() as u64;
        let mut warning = None;
        if link.kind == ProviderKind::OpenAiCompatible && link.key.is_some() {
            let control = format!("tingskriver-invalid-{}", uuid::Uuid::new_v4());
            let response = client
                .get(url.clone())
                .bearer_auth(control)
                .send()
                .await
                .map_err(|error| map_reqwest(error, timeout, link))?;
            match response.status() {
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {}
                StatusCode::TOO_MANY_REQUESTS => {
                    check(&response)?;
                }
                status if status.is_redirection() => {
                    check(&response)?;
                }
                status if status.is_success() => warning = Some(key_warning(&url)),
                _ => check(&response)?,
            }
        }
        Ok(TestReport {
            latency_ms,
            warning,
            models,
        })
    };
    let result = tokio::time::timeout(timeout, work)
        .await
        .unwrap_or_else(|_| Err(ProviderError::timeout(timeout)));
    if let Ok(report) = &result {
        if let Ok(mut entries) = cache.lock() {
            entries.insert(cache_key, (now(), report.clone()));
        }
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
    Err(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_gateways_get_a_stronger_warning() {
        assert!(key_warning(&Url::parse("https://example.com/v1").unwrap())
            .contains("offentlige gateway"));
        assert_eq!(
            key_warning(&Url::parse("http://server.local/v1").unwrap()),
            "Serveren tjekker ikke nøglen."
        );
    }

    #[test]
    fn allows_https_and_private_http_without_credentials() {
        for url in [
            "https://example.com/v1",
            "http://localhost/v1",
            "http://127.0.0.2/v1",
            "http://[::1]/v1",
            "http://192.168.1.2/v1",
            "http://10.0.0.1/v1",
            "http://172.16.0.1/v1",
            "http://169.254.1.1/v1",
            "http://server.local/v1",
            "http://[fe80::1]/v1",
            "http://[febf::1]/v1",
            "http://[fc00::1]/v1",
            "http://[fdff::1]/v1",
        ] {
            assert!(models_url(url).is_ok(), "{url}");
        }
        for url in [
            "http://example.com/v1",
            "http://localhost.example.com/v1",
            "http://[fbff::1]/v1",
            "http://[fe00::1]/v1",
            "http://[fe7f::1]/v1",
            "http://[fec0::1]/v1",
            "https://user@example.com/v1",
            "https://:password@example.com/v1",
            "file:///v1",
            "not a URL",
        ] {
            assert!(models_url(url).is_err(), "{url}");
        }
    }
}
