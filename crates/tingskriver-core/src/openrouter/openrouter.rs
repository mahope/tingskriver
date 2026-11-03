#![deny(clippy::disallowed_methods)]

use crate::http::client;
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
    get_models_from("https://openrouter.ai/api/v1/models").await
}

async fn get_models_from(url: &str) -> Result<Vec<OpenRouterModel>, String> {
    let client = client();
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
    use std::time::{Duration, Instant};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

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

    #[tokio::test(flavor = "current_thread")]
    async fn hanging_headers_and_body_timeout_without_blocking_other_calls() {
        assert!(crate::http::TIMEOUT <= Duration::from_secs(10));
        for response in [
            b"".as_slice(),
            b"HTTP/1.1 200 OK\r\nContent-Length: 999\r\n\r\n".as_slice(),
        ] {
            let (url, server) = server(response).await;
            let started = Instant::now();
            let request = get_models_from(&url);
            let other_call = async {
                tokio::time::sleep(Duration::from_millis(20)).await;
                assert!(started.elapsed() < Duration::from_secs(1));
                crate::set_language_preference("auto-translate".into()).unwrap();
            };
            let (result, ()) = tokio::join!(request, other_call);
            assert!(result.unwrap_err().contains("timed out"));
            assert!(started.elapsed() < Duration::from_millis(10_500));
            server.abort();
        }
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
        let models = get_models_from(&url).await.unwrap();
        assert_eq!(models[0].id, "test");
        assert_eq!(models[0].name, "Unknown");
        assert_eq!(models[0].context_length, Some(42));
        task.await.unwrap();
        let (url, task) = server(b"HTTP/1.1 503 Unavailable\r\nContent-Length: 0\r\n\r\n").await;
        assert!(get_models_from(&url).await.unwrap_err().contains("503"));
        task.abort();
    }
}
