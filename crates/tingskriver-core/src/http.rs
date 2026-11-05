//! Short metadata deadlines are separate from inference and download budgets.
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

/// Downloads have no total deadline: each read is bounded by the caller.
pub(crate) fn download_client_builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder().connect_timeout(TIMEOUT)
}

pub(crate) async fn send_download(
    request: reqwest::RequestBuilder,
) -> anyhow::Result<reqwest::Response> {
    Ok(tokio::time::timeout(TIMEOUT, request.send()).await??)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn healthy_download_and_inference_can_exceed_ten_seconds() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let mut tasks = Vec::new();
            for _ in 0..2 {
                let (mut socket, _) = listener.accept().await.unwrap();
                tasks.push(tokio::spawn(async move {
                    let mut headers = Vec::new();
                    while !headers.ends_with(b"\r\n\r\n") {
                        headers.push(socket.read_u8().await.unwrap());
                    }
                    if headers.starts_with(b"GET /download ") {
                        socket
                            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\n")
                            .await
                            .unwrap();
                        for _ in 0..6 {
                            tokio::time::sleep(Duration::from_secs(2)).await;
                            socket.write_all(b"x").await.unwrap();
                        }
                    } else {
                        tokio::time::sleep(Duration::from_secs(12)).await;
                        socket
                            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK")
                            .await
                            .unwrap();
                    }
                }));
            }
            for task in tasks {
                task.await.unwrap();
            }
        });
        let download = async {
            send_download(
                download_client_builder()
                    .build()
                    .unwrap()
                    .get(format!("{url}/download")),
            )
            .await
            .unwrap()
            .text()
            .await
            .unwrap()
        };
        let inference = async {
            crate::providers::client::http_client()
                .get(format!("{url}/inference"))
                .timeout(Duration::from_secs(300))
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap()
        };
        let (download, inference) = tokio::join!(download, inference);
        assert_eq!(download, "xxxxxx");
        assert_eq!(inference, "OK");
        server.await.unwrap();
    }
}
