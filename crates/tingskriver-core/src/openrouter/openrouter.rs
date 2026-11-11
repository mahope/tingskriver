#![deny(clippy::disallowed_methods)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct OpenRouterModel {
    pub id: String,
    pub name: String,
    pub context_length: Option<u32>,
    pub prompt_price: Option<String>,
    pub completion_price: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterApiModel {
    id: String,
    name: Option<String>,
    context_length: Option<u32>,
    #[serde(default)]
    top_provider: Option<TopProvider>,
    #[serde(default)]
    pricing: Option<Pricing>,
}

#[derive(Debug, Deserialize, Default)]
struct TopProvider {
    context_length: Option<u32>,
}

#[derive(Debug, Deserialize, Default)]
struct Pricing {
    prompt: Option<String>,
    completion: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterResponse {
    data: Vec<OpenRouterApiModel>,
}

pub async fn get_openrouter_models() -> Result<Vec<OpenRouterModel>, String> {
    get_openrouter_models_at("https://openrouter.ai/api/v1/models").await
}

async fn get_openrouter_models_at(url: &str) -> Result<Vec<OpenRouterModel>, String> {
    let client = crate::http::client();
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Failed to make HTTP request: {}", e))?;

    if !response.status().is_success() {
        return Err(format!(
            "HTTP request failed with status: {}",
            response.status()
        ));
    }

    let api_response: OpenRouterResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse JSON response: {}", e))?;

    let models = api_response
        .data
        .into_iter()
        .map(|m| OpenRouterModel {
            id: m.id,
            name: m.name.unwrap_or_else(|| "Unknown".to_string()),
            context_length: m
                .top_provider
                .as_ref()
                .and_then(|tp| tp.context_length)
                .or(m.context_length),
            prompt_price: m.pricing.as_ref().and_then(|p| p.prompt.clone()),
            completion_price: m.pricing.as_ref().and_then(|p| p.completion.clone()),
        })
        .collect();

    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[tokio::test(flavor = "current_thread")]
    async fn stalled_model_list_times_out_without_blocking_core() {
        for stalled_body in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}/models", listener.local_addr().unwrap());
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = [0; 4096];
                socket.read(&mut request).await.unwrap();
                if stalled_body {
                    socket
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\n")
                        .await
                        .unwrap();
                }
                tokio::time::sleep(Duration::from_secs(30)).await;
            });
            let request = get_openrouter_models_at(&url);
            let concurrent = async {
                tokio::time::sleep(Duration::from_millis(20)).await;
                // A second model-list call must finish on the same worker.
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let url = format!("http://{}/models", listener.local_addr().unwrap());
                let server = tokio::spawn(async move {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let mut request = [0; 4096];
                    socket.read(&mut request).await.unwrap();
                    let body = r#"{"data":[{"id":"test","top_provider":{"context_length":42},"pricing":{"prompt":"0.1"}}]}"#;
                    socket
                        .write_all(
                            format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                                body.len(),
                                body
                            )
                            .as_bytes(),
                        )
                        .await
                        .unwrap();
                });
                let models =
                    tokio::time::timeout(Duration::from_secs(1), get_openrouter_models_at(&url))
                        .await
                        .unwrap()
                        .unwrap();
                assert_eq!(models[0].id, "test");
                assert_eq!(models[0].name, "Unknown");
                assert_eq!(models[0].context_length, Some(42));
                assert_eq!(models[0].prompt_price.as_deref(), Some("0.1"));
                server.await.unwrap();
            };
            let (result, ()) = tokio::time::timeout(Duration::from_millis(10_500), async {
                tokio::join!(request, concurrent)
            })
            .await
            .expect("model list exceeded its ten-second request budget");
            assert!(result.unwrap_err().contains("timed out"));
            server.abort();
            let _ = server.await;
        }
    }
    async fn server(response: &'static [u8]) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/models", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            socket.read(&mut request).await.unwrap();
            socket.write_all(response).await.unwrap();
            std::future::pending::<()>().await;
        });
        (url, task)
    }

    #[tokio::test]
    async fn models_are_decoded_and_http_errors_returned() {
        let body = r#"{"data":[{"id":"test","top_provider":{"context_length":42},"pricing":{"prompt":"0"}}]}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/models", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            socket.read(&mut request).await.unwrap();
            socket.write_all(response.as_bytes()).await.unwrap();
        });
        let models = get_openrouter_models_at(&url).await.unwrap();
        assert_eq!(models[0].id, "test");
        assert_eq!(models[0].name, "Unknown");
        assert_eq!(models[0].context_length, Some(42));
        task.await.unwrap();
        let (url, task) = server(b"HTTP/1.1 503 Unavailable\r\nContent-Length: 0\r\n\r\n").await;
        assert!(get_openrouter_models_at(&url)
            .await
            .unwrap_err()
            .contains("503"));
        task.abort();
    }
}
