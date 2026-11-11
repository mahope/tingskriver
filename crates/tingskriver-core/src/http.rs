//! HTTP deadlines distinguish short requests from long-running operations.
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

/// Large models may take hours on a slow connection; stalled reads are bounded
/// separately by each download loop.
pub(crate) fn download_client_builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .connect_timeout(TIMEOUT)
        .timeout(Duration::from_secs(4 * 60 * 60))
}

pub(crate) async fn send_download(
    request: reqwest::RequestBuilder,
) -> anyhow::Result<reqwest::Response> {
    // Bound header latency without applying the short deadline to the body.
    tokio::time::timeout(TIMEOUT, request.send())
        .await
        .map_err(|_| anyhow::anyhow!("Download timeout - No HTTP headers received for 10 seconds"))?
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn download_headers_have_a_short_deadline() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.unwrap();
            std::future::pending::<()>().await;
        });
        let request = download_client_builder()
            .build()
            .unwrap()
            .get(format!("http://{address}"));
        let result = tokio::time::timeout(Duration::from_secs(12), send_download(request)).await;
        server.abort();
        let error = result
            .expect("header deadline must precede the total download deadline")
            .expect_err("a server without headers must time out");
        assert!(error.to_string().contains("headers"), "{error}");
    }

    #[tokio::test]
    async fn progressing_download_outlives_short_request_deadline() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            socket.read(&mut request).await.unwrap();
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 12\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
            for _ in 0..12 {
                socket.write_all(b"x").await.unwrap();
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });
        let request = download_client_builder()
            .build()
            .unwrap()
            .get(format!("http://{address}"));
        let body = send_download(request).await.unwrap().bytes().await.unwrap();
        assert_eq!(body.as_ref(), b"xxxxxxxxxxxx");
        server.await.unwrap();
    }
}
