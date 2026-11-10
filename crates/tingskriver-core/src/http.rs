//! All core HTTP requests have a total deadline, including the response body.
use std::time::Duration;

pub(crate) const TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) fn client() -> reqwest::Client {
    static CLIENT: once_cell::sync::Lazy<reqwest::Client> = once_cell::sync::Lazy::new(|| {
        reqwest::Client::builder()
            .connect_timeout(TIMEOUT)
            .timeout(TIMEOUT)
            .build()
            .expect("failed to build core HTTP client")
    });
    CLIENT.clone()
}
