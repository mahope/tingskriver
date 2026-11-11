//! Connection verification uses its own client: a caller's redirect policy
//! must never forward a key. Only custom keyed gateways need a negative control.
//! Each test makes a fresh request. HTTP follows the settings' provider policy;
//! public HTTP endpoints produce a warning in the report.

use std::time::{Duration, Instant};

use reqwest::{redirect::Policy, StatusCode, Url};

use super::client::{classify_status, map_reqwest, ResolvedLink, TestReport};
use super::error::ProviderError;
use super::model::{accepts_base_url, ProviderKind};

const TIMEOUT: Duration = Duration::from_secs(8);

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

fn http_warning(url: &Url) -> Option<&'static str> {
    (url.scheme() == "http" && !private_host(url))
        .then_some("Advarsel: Forbindelsen bruger ukrypteret HTTP til en offentlig vært.")
}

fn models_url(kind: ProviderKind, base: &str) -> Result<Url, ProviderError> {
    let invalid = || ProviderError::Config {
        detail: "Brug en gyldig HTTP- eller HTTPS-adresse.".into(),
    };
    let mut url = Url::parse(base.trim()).map_err(|_| invalid())?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(ProviderError::Config {
            detail: "Brug en adresse uden brugernavn eller adgangskode.".into(),
        });
    }
    if url.host_str().is_none() || !matches!(url.scheme(), "http" | "https") {
        return Err(invalid());
    }
    if !accepts_base_url(kind, base) {
        return Err(ProviderError::Config {
            detail: "Brug HTTPS til denne udbyder.".into(),
        });
    }
    url.set_path(&format!("{}/models", url.path().trim_end_matches('/')));
    Ok(url)
}

pub(super) async fn test(link: &ResolvedLink) -> Result<TestReport, ProviderError> {
    let url = models_url(link.kind, &link.base_url)?;
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
        if let Some(http_warning) = http_warning(&url) {
            warning = Some(match warning {
                Some(warning) => format!("{http_warning} {warning}"),
                None => http_warning.into(),
            });
        }
        Ok(TestReport {
            latency_ms,
            warning,
            models,
        })
    };
    tokio::time::timeout(timeout, work)
        .await
        .unwrap_or_else(|_| Err(ProviderError::timeout(timeout)))
}

fn check(response: &reqwest::Response) -> Result<(), ProviderError> {
    if response.status().is_redirection() {
        return Err(ProviderError::BadResponse {
            detail: "Omdirigering er ikke tilladt. Nøglen er ikke sendt videre.".into(),
        });
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
    fn public_http_is_allowed_with_a_warning() {
        for kind in [ProviderKind::OpenAiCompatible, ProviderKind::Ollama] {
            let url = models_url(kind, "http://example.com/v1").unwrap();
            assert_eq!(
                http_warning(&url),
                Some("Advarsel: Forbindelsen bruger ukrypteret HTTP til en offentlig vært.")
            );
            assert!(http_warning(&models_url(kind, "https://example.com/v1").unwrap()).is_none());
            assert!(http_warning(&models_url(kind, "http://localhost/v1").unwrap()).is_none());
        }
    }

    #[test]
    fn invalid_addresses_have_specific_messages() {
        for (url, detail) in [
            ("http://example.com/v1", "Brug HTTPS til denne udbyder."),
            (
                "https://user@example.com/v1",
                "Brug en adresse uden brugernavn eller adgangskode.",
            ),
            ("file:///v1", "Brug en gyldig HTTP- eller HTTPS-adresse."),
        ] {
            assert_eq!(
                models_url(ProviderKind::OpenAi, url).unwrap_err(),
                ProviderError::Config {
                    detail: detail.into()
                }
            );
        }
    }

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
    fn allows_configured_http_without_credentials() {
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
            "http://gpu-box:11434",
            "http://ollama.lan",
            "http://100.64.0.1:11434",
            "http://example.com/v1",
            "http://localhost.example.com/v1",
            "http://[fbff::1]/v1",
            "http://[fe00::1]/v1",
            "http://[fe7f::1]/v1",
            "http://[fec0::1]/v1",
        ] {
            for kind in [ProviderKind::OpenAiCompatible, ProviderKind::Ollama] {
                assert!(accepts_base_url(kind, url), "{url}");
                assert!(models_url(kind, url).is_ok(), "{url}");
            }
        }
        for url in [
            "https://user@example.com/v1",
            "https://:password@example.com/v1",
            "file:///v1",
            "not a URL",
        ] {
            assert!(
                models_url(ProviderKind::OpenAiCompatible, url).is_err(),
                "{url}"
            );
        }
    }
}
