use super::*;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

fn pair() -> (Transport, tokio::io::DuplexStream) {
    let (client, helper) = tokio::io::duplex(4096);
    let (reader, writer) = tokio::io::split(client);
    (Transport::new(reader, writer), helper)
}

#[tokio::test]
async fn ping_during_generation_never_steals_response_1000_iterations() {
    let (transport, helper) = pair();
    let transport = Arc::new(Mutex::new(transport));
    let (reader, mut writer) = tokio::io::split(helper);
    let fake = tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        let mut seed = 20_u64;
        while let Some(line) = lines.next_line().await.unwrap() {
            let request: serde_json::Value = serde_json::from_str(&line).unwrap();
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            tokio::time::sleep(Duration::from_micros(seed % 100)).await;
            let response = if request["type"] == "ping" {
                serde_json::json!({"type":"pong"})
            } else {
                serde_json::json!({"type":"response", "text":request["iteration"]})
            };
            writer
                .write_all(format!("{response}\n").as_bytes())
                .await
                .unwrap();
        }
    });
    for iteration in 0..1000 {
        let mut transaction = transport.lock().await;
        let request = serde_json::json!({"type":"generate", "iteration":iteration}).to_string();
        let generation = transaction.exchange(&request);
        let ping = async {
            tokio::task::yield_now().await;
            // Same non-queuing policy as the health monitor.
            assert!(transport.try_lock().is_err());
        };
        let (response, ()) = tokio::join!(generation, ping);
        let response: serde_json::Value = serde_json::from_str(&response.unwrap()).unwrap();
        assert_eq!(response["text"], iteration);
        drop(transaction);
        let response = transport
            .lock()
            .await
            .exchange(r#"{"type":"ping"}"#)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&response).unwrap()["type"],
            "pong"
        );
    }
    drop(transport);
    fake.await.unwrap();
}

#[tokio::test]
async fn timeout_and_cancellation_cannot_reuse_late_response() {
    for cancel in [false, true] {
        let (mut transport, mut helper) = pair();
        if cancel {
            let token = tokio_util::sync::CancellationToken::new();
            tokio::select! {
                biased;
                _ = transport.exchange(r#"{"type":"generate"}"#) => panic!("no response"),
                _ = async { tokio::task::yield_now().await; token.cancel(); token.cancelled().await; } => {}
            }
        } else {
            assert!(tokio::time::timeout(
                Duration::from_millis(5),
                transport.exchange(r#"{"type":"generate"}"#)
            )
            .await
            .is_err());
        }
        helper
            .write_all(b"{\"type\":\"response\",\"text\":\"late\"}\n")
            .await
            .unwrap();
        assert!(transport
            .exchange(r#"{"type":"ping"}"#)
            .await
            .unwrap_err()
            .to_string()
            .contains("restart required"));
    }
}

#[tokio::test]
async fn eof_partial_response_and_wrong_type_fail_closed() {
    for response in ["", "{\"type\":\"response\"", "{\"type\":\"pong\"}\n"] {
        let (mut transport, mut helper) = pair();
        helper.write_all(response.as_bytes()).await.unwrap();
        helper.shutdown().await.unwrap();
        assert!(transport.exchange(r#"{"type":"generate"}"#).await.is_err());
        assert!(transport
            .exchange(r#"{"type":"ping"}"#)
            .await
            .unwrap_err()
            .to_string()
            .contains("restart required"));
    }
}

#[tokio::test]
async fn helper_error_is_delivered_and_next_transaction_succeeds() {
    let (mut transport, mut helper) = pair();
    helper
        .write_all(b"{\"type\":\"error\",\"message\":\"failed\"}\n{\"type\":\"pong\"}\n")
        .await
        .unwrap();
    let result = transport.exchange(r#"{"type":"generate"}"#).await.unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&result).unwrap()["message"],
        "failed"
    );
    assert!(transport.exchange(r#"{"type":"ping"}"#).await.is_ok());
}
