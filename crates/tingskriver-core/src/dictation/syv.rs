//! Syv.ai client (OpenAI-compatible `/audio/transcriptions`).

use std::time::{Duration, Instant};

use reqwest::multipart::{Form, Part};
use serde::Deserialize;

#[derive(Deserialize)]
struct TranscriptionResponse {
    text: String,
}

/// Transcribe a WAV file. `prompt` carries dictionary words as hints.
pub async fn transcribe(
    client: &reqwest::Client,
    endpoint: &str,
    api_key: &str,
    model: &str,
    wav: Vec<u8>,
    prompt: Option<String>,
) -> Result<String, String> {
    transcribe_in(client, endpoint, api_key, model, "da", wav, prompt).await
}

/// [`transcribe`] in another speech language ("en"); "" means Danish.
pub async fn transcribe_in(
    client: &reqwest::Client,
    endpoint: &str,
    api_key: &str,
    model: &str,
    language: &str,
    wav: Vec<u8>,
    prompt: Option<String>,
) -> Result<String, String> {
    let language = match language.trim() {
        "" => "da".to_string(),
        l => l.to_lowercase(),
    };
    let file = Part::bytes(wav)
        .file_name("diktering.wav")
        .mime_str("audio/wav")
        .map_err(|e| e.to_string())?;
    let mut form = Form::new()
        .part("file", file)
        .text("model", model.to_string())
        .text("language", language)
        .text("response_format", "json");
    if let Some(prompt) = prompt.filter(|p| !p.trim().is_empty()) {
        form = form.text("prompt", prompt);
    }

    let response = client
        .post(format!("{}/audio/transcriptions", endpoint.trim_end_matches('/')))
        .bearer_auth(api_key)
        .multipart(form)
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| format!("Syv.ai kunne ikke nås: {}", e.without_url()))?;

    let status = response.status();
    if !status.is_success() {
        return Err(format!("Syv.ai svarede {status}"));
    }
    let body: TranscriptionResponse = response
        .json()
        .await
        .map_err(|e| format!("Uventet svar fra Syv.ai: {}", e.without_url()))?;
    Ok(body.text)
}

/// Check the key against `/models` and measure the round trip.
pub async fn test_key(client: &reqwest::Client, endpoint: &str, api_key: &str) -> Result<u64, String> {
    let start = Instant::now();
    let response = client
        .get(format!("{}/models", endpoint.trim_end_matches('/')))
        .bearer_auth(api_key)
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("Syv.ai kunne ikke nås: {}", e.without_url()))?;
    let elapsed = start.elapsed().as_millis() as u64;
    match response.status() {
        s if s.is_success() => Ok(elapsed),
        s if s.as_u16() == 401 || s.as_u16() == 403 => Err("Nøglen blev afvist af Syv.ai".to_string()),
        s => Err(format!("Syv.ai svarede {s}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Minimal OpenAI-compatible server: answers one request and hands the raw
    /// request back for inspection.
    async fn fake_server(status: &'static str, body: &'static str) -> (String, tokio::task::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buf = [0u8; 8192];
            loop {
                let n = socket.read(&mut buf).await.unwrap();
                request.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&request).to_string();
                if let Some(header_end) = text.find("\r\n\r\n") {
                    let length = text[..header_end]
                        .lines()
                        .find_map(|l| l.to_lowercase().strip_prefix("content-length: ").map(|v| v.trim().parse::<usize>().unwrap()))
                        .unwrap_or(0);
                    if request.len() >= header_end + 4 + length {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            let response = format!(
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            String::from_utf8_lossy(&request).to_string()
        });
        (format!("http://{addr}/v1"), handle)
    }

    #[tokio::test]
    async fn sends_the_mode_language() {
        let (endpoint, server) = fake_server("200 OK", r#"{"text":"Hello."}"#).await;
        let client = reqwest::Client::new();
        let text = transcribe_in(&client, &endpoint, "hv_test", "syv-transcribe", " EN", vec![1], None)
            .await
            .unwrap();
        assert_eq!(text, "Hello.");
        let request = server.await.unwrap();
        assert!(request.contains("name=\"language\"\r\n\r\nen\r\n"));
    }

    #[tokio::test]
    async fn sends_openai_compatible_request() {
        let (endpoint, server) = fake_server("200 OK", r#"{"text":"Hej Mads."}"#).await;
        let client = reqwest::Client::new();
        let text = transcribe(&client, &endpoint, "hv_test", "syv-transcribe", vec![1, 2, 3], Some("Mahoje, Syv.ai".into()))
            .await
            .unwrap();
        assert_eq!(text, "Hej Mads.");

        let request = server.await.unwrap();
        assert!(request.starts_with("POST /v1/audio/transcriptions "));
        assert!(request.to_lowercase().contains("authorization: bearer hv_test"));
        for (name, value) in [("model", "syv-transcribe"), ("language", "da"), ("prompt", "Mahoje, Syv.ai")] {
            assert!(
                request.contains(&format!("name=\"{name}\"\r\n\r\n{value}\r\n")),
                "missing field {name}"
            );
        }
        assert!(request.contains("filename=\"diktering.wav\""));
    }

    #[tokio::test]
    async fn reports_http_errors_without_key() {
        let (endpoint, _server) = fake_server("500 Internal Server Error", r#"{"error":"boom"}"#).await;
        let error = transcribe(&reqwest::Client::new(), &endpoint, "hv_secret", "m", vec![0], None)
            .await
            .unwrap_err();
        assert!(error.contains("500"));
        assert!(!error.contains("hv_secret"));
    }

    #[tokio::test]
    async fn key_test_maps_rejection() {
        let (endpoint, _server) = fake_server("401 Unauthorized", "{}").await;
        let error = test_key(&reqwest::Client::new(), &endpoint, "bad").await.unwrap_err();
        assert_eq!(error, "Nøglen blev afvist af Syv.ai");
    }
}
