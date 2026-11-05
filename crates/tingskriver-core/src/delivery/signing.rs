//! HMAC-SHA256 signatures for webhook deliveries.
//!
//! The signed message is `<timestamp>.<raw body>`, where the timestamp is Unix
//! seconds as sent in `X-Tingskriver-Timestamp`. The receiver recomputes the
//! MAC over the exact bytes it received and rejects timestamps outside a window.
use hmac::{Hmac, Mac};
use sha2::Sha256;

pub const SIGNATURE_HEADER: &str = "X-Tingskriver-Signature";
pub const TIMESTAMP_HEADER: &str = "X-Tingskriver-Timestamp";
/// Receivers should reject timestamps further than this from their own clock.
pub const DEFAULT_TOLERANCE_SECS: i64 = 300;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum VerifyError {
    #[error("Tidsstemplet ligger uden for tilladt vindue")]
    Stale,
    #[error("Signaturen er ugyldig")]
    Mismatch,
}

fn mac(secret: &[u8], timestamp: i64, body: &[u8]) -> Hmac<Sha256> {
    // HMAC accepts keys of any length.
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC takes any key length");
    mac.update(timestamp.to_string().as_bytes());
    mac.update(b".");
    mac.update(body);
    mac
}

/// The header value: `sha256=<lowercase hex>`.
pub fn sign(secret: &[u8], timestamp: i64, body: &[u8]) -> String {
    let tag = mac(secret, timestamp, body).finalize().into_bytes();
    let mut out = String::from("sha256=");
    for byte in tag {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn decode_hex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 || !text.is_ascii() {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}

/// Reference verifier (constant-time MAC comparison). `now` is the receiver's
/// Unix time in seconds.
pub fn verify(
    secret: &[u8],
    timestamp: i64,
    body: &[u8],
    signature: &str,
    now: i64,
    tolerance_secs: i64,
) -> Result<(), VerifyError> {
    if (now - timestamp).abs() > tolerance_secs {
        return Err(VerifyError::Stale);
    }
    let tag = signature
        .strip_prefix("sha256=")
        .and_then(decode_hex)
        .ok_or(VerifyError::Mismatch)?;
    mac(secret, timestamp, body)
        .verify_slice(&tag)
        .map_err(|_| VerifyError::Mismatch)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Vectors computed independently with Python's hmac module.
    const SECRET: &[u8] = b"whsec_test_secret";
    const BODY: &[u8] = br#"{"hello":"world"}"#;
    const SIG: &str = "sha256=86748dbec9cc87a9219f8a96632da703271bf5e85aa0afa2b310c37ba059d514";

    #[test]
    fn matches_fixed_vectors() {
        assert_eq!(sign(SECRET, 1_700_000_000, BODY), SIG);
        assert_eq!(
            sign(b"k", 0, b""),
            "sha256=6b4a4b8b3c40f1e8f53a3d36682e5f99f7ad2ac1df1c93dfe336f329167641e7"
        );
    }

    #[test]
    fn unchanged_body_verifies_and_changed_body_is_rejected() {
        let now = 1_700_000_010;
        assert_eq!(verify(SECRET, 1_700_000_000, BODY, SIG, now, 300), Ok(()));
        assert_eq!(
            verify(
                SECRET,
                1_700_000_000,
                br#"{"hello":"World"}"#,
                SIG,
                now,
                300
            ),
            Err(VerifyError::Mismatch)
        );
        assert_eq!(
            verify(b"other", 1_700_000_000, BODY, SIG, now, 300),
            Err(VerifyError::Mismatch)
        );
        // A tampered timestamp breaks the MAC too.
        assert_eq!(
            verify(SECRET, 1_700_000_001, BODY, SIG, now, 300),
            Err(VerifyError::Mismatch)
        );
    }

    #[test]
    fn timestamp_outside_window_is_rejected() {
        assert_eq!(
            verify(SECRET, 1_700_000_000, BODY, SIG, 1_700_000_301, 300),
            Err(VerifyError::Stale)
        );
        assert_eq!(
            verify(SECRET, 1_700_000_000, BODY, SIG, 1_699_999_699, 300),
            Err(VerifyError::Stale)
        );
        assert_eq!(
            verify(SECRET, 1_700_000_000, BODY, SIG, 1_700_000_300, 300),
            Ok(())
        );
    }

    #[test]
    fn malformed_signatures_are_rejected() {
        for bad in [
            "",
            "sha256=",
            "sha256=zz",
            "sha1=00",
            &SIG[7..],
            "sha256=abc",
        ] {
            assert_eq!(
                verify(SECRET, 1, BODY, bad, 1, 300),
                Err(VerifyError::Mismatch),
                "{bad}"
            );
        }
    }
}
