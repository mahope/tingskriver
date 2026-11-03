use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TimedWord {
    pub text: String,
    pub start_ms: f64,
    pub end_ms: f64,
}

pub(crate) fn words_from_tokens(tokens: Vec<(String, f64, f64)>) -> Vec<TimedWord> {
    let mut words: Vec<TimedWord> = Vec::new();
    let mut boundary = true;
    for (text, start_ms, end_ms) in tokens {
        for part in text.split_inclusive(char::is_whitespace) {
            if part.starts_with(char::is_whitespace) {
                boundary = true;
            }
            let word = part.trim();
            if !word.is_empty() {
                if boundary || words.is_empty() {
                    words.push(TimedWord {
                        text: word.into(),
                        start_ms,
                        end_ms,
                    });
                } else if let Some(previous) = words.last_mut() {
                    previous.text.push_str(word);
                    previous.end_ms = end_ms;
                }
            }
            boundary = part.ends_with(char::is_whitespace);
        }
    }
    words
}

pub(crate) fn words_from_token_bytes(
    tokens: Vec<(Vec<u8>, f64, f64)>,
) -> anyhow::Result<Vec<TimedWord>> {
    let mut decoded = Vec::new();
    let mut pending = Vec::new();
    let mut first_start = 0.0;
    for (bytes, start, end) in tokens {
        if pending.is_empty() {
            first_start = start;
        }
        pending.extend(bytes);
        match std::str::from_utf8(&pending) {
            Ok(text) => {
                decoded.push((text.to_string(), first_start, end));
                pending.clear();
            }
            Err(e) if e.error_len().is_none() => {}
            Err(e) => return Err(e.into()),
        }
    }
    anyhow::ensure!(
        pending.is_empty(),
        "Incomplete UTF-8 in transcription tokens"
    );
    Ok(words_from_tokens(decoded))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_split_across_model_tokens_is_preserved() {
        let words = words_from_token_bytes(vec![
            (vec![b' ', 0xc3], 100.0, 200.0),
            (vec![0xa6, b'b', b'l', b'e'], 200.0, 400.0),
        ])
        .unwrap();
        assert_eq!(
            words,
            vec![TimedWord {
                text: "æble".into(),
                start_ms: 100.0,
                end_ms: 400.0
            }]
        );
        assert!(words_from_token_bytes(vec![(vec![0xc3], 0.0, 1.0)]).is_err());
    }

    #[test]
    fn subwords_and_punctuation_keep_model_times() {
        let words = words_from_tokens(vec![
            (" hej".into(), 100.0, 200.0),
            ("san".into(), 200.0, 300.0),
            (",".into(), 300.0, 320.0),
            (" næste".into(), 600.0, 900.0),
        ]);
        assert_eq!(
            words,
            vec![
                TimedWord {
                    text: "hejsan,".into(),
                    start_ms: 100.0,
                    end_ms: 320.0
                },
                TimedWord {
                    text: "næste".into(),
                    start_ms: 600.0,
                    end_ms: 900.0
                },
            ]
        );
    }
}
