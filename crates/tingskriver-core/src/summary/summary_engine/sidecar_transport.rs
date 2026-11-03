use anyhow::{anyhow, Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

/// A cancelled transaction poisons the connection: its reply must never be reused.
pub(super) struct Transport {
    reader: BufReader<Box<dyn AsyncRead + Send + Unpin>>,
    writer: Box<dyn AsyncWrite + Send + Unpin>,
    interrupted: bool,
}

#[cfg(test)]
#[path = "sidecar_transport_tests.rs"]
mod tests;

impl Transport {
    pub(super) fn new(
        reader: impl AsyncRead + Send + Unpin + 'static,
        writer: impl AsyncWrite + Send + Unpin + 'static,
    ) -> Self {
        Self {
            reader: BufReader::new(Box::new(reader)),
            writer: Box::new(writer),
            interrupted: false,
        }
    }

    pub(super) async fn exchange(&mut self, request: &str) -> Result<String> {
        if self.interrupted {
            return Err(anyhow!("Helper connection interrupted; restart required"));
        }
        self.interrupted = true;
        self.writer.write_all(request.as_bytes()).await?;
        self.writer.write_all(b"\n").await?;
        self.writer.flush().await?;
        let mut line = String::new();
        let size = self
            .reader
            .read_line(&mut line)
            .await
            .context("Read helper response")?;
        if size == 0 || !line.ends_with('\n') {
            return Err(anyhow!("Helper closed stdout before a complete response"));
        }
        let response: serde_json::Value =
            serde_json::from_str(&line).context("Invalid helper response")?;
        let request: serde_json::Value = serde_json::from_str(request)?;
        let expected = match request["type"].as_str() {
            Some("ping") => "pong",
            Some("generate") => "response",
            Some("shutdown") => "goodbye",
            _ => return Err(anyhow!("Unknown helper request type")),
        };
        if response["type"] != expected && response["type"] != "error" {
            return Err(anyhow!("Unexpected helper response type"));
        }
        self.interrupted = false;
        Ok(line.trim().to_owned())
    }
}
