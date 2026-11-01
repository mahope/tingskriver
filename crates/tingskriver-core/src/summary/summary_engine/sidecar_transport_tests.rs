use super::*;
use std::time::Duration;

fn pair() -> (Transport, tokio::io::DuplexStream) {
    let (client, helper) = tokio::io::duplex(4096);
    let (reader, writer) = tokio::io::split(client);
    (Transport::new(reader, writer), helper)
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
