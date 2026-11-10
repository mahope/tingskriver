use super::*;
use crate::summary::map_reduce::{Model, Phase};

const VALID: &str = r#"{"summary":"A short summary","decisions":["Proceed"],"tasks":[{"who":null,"what":"Check","deadline":null}],"open_questions":["When?"]}"#;

fn budget() -> Budget {
    Budget {
        context: 2048,
        answer: 256,
        framing: 64,
        estimate: |s| s.chars().count().div_ceil(4),
    }
}

struct Fake {
    calls: usize,
    invalid: usize,
    invalid_response: String,
    cancel: Option<CancellationToken>,
    wait_after_cancel: bool,
}

#[async_trait::async_trait]
impl Model for Fake {
    async fn generate(
        &mut self,
        phase: Phase,
        system: &str,
        input: &str,
    ) -> Result<String, String> {
        assert!(budget().fits(system, input));
        assert!(!input.contains("Min egen note"));
        self.calls += 1;
        if let Some(token) = self.cancel.take() {
            token.cancel();
            if self.wait_after_cancel {
                std::future::pending::<()>().await;
            }
        }
        if phase != Phase::Final {
            return Ok("Condensed source facts.".into());
        }
        if self.invalid > 0 {
            self.invalid -= 1;
            return Ok(self.invalid_response.clone());
        }
        Ok(VALID.into())
    }
}

fn fake(invalid: usize) -> Fake {
    Fake {
        calls: 0,
        invalid,
        invalid_response: "{invalid".into(),
        cancel: None,
        wait_after_cancel: true,
    }
}

#[tokio::test]
async fn preserves_notes_and_marks_origins() {
    let notes = vec!["  Min egen note\næøå\r\n".into(), "".into(), "".into()];
    let result = generate(
        &mut fake(0),
        &budget(),
        &["Transcript".into()],
        &notes,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(
        result
            .notes
            .iter()
            .map(|n| n.text.clone())
            .collect::<Vec<_>>(),
        notes
    );
    assert!(result.notes.iter().all(|n| n.origin == Origin::Own));
    assert_eq!(result.origin, Origin::Ai);
    assert_eq!(result.content.tasks[0].who, None);
}

#[tokio::test]
async fn retries_invalid_json_once() {
    let mut model = fake(1);
    generate(&mut model, &budget(), &[], &[], &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(model.calls, 2);
    let mut model = fake(2);
    assert!(
        generate(&mut model, &budget(), &[], &[], &CancellationToken::new())
            .await
            .is_err()
    );
    assert_eq!(model.calls, 2);
}

#[tokio::test]
async fn retries_schema_invalid_and_oversized_answers() {
    for raw in ["{}".into(), "x".repeat(2048)] {
        let mut model = fake(1);
        model.invalid_response = raw;
        let result = generate(&mut model, &budget(), &[], &[], &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(model.calls, 2);
        assert_eq!(result.content.summary, "A short summary");
    }
}

#[test]
fn rejects_schema_violations() {
    for value in [
        "{}",
        r#"{"summary":5,"decisions":[],"tasks":[],"open_questions":[]}"#,
        &VALID.replace("\"who\":null,", ""),
        &VALID.replace("\"who\":null", "\"who\":42"),
        &VALID.replace("\"summary\":", "\"extra\":true,\"summary\":"),
        &VALID.replace("\"decisions\":[\"Proceed\"]", "\"decisions\":[1]"),
        &VALID.replace("\"what\":\"Check\"", "\"what\":null"),
        &VALID.replace("\"deadline\":null", "\"deadline\":[]"),
    ] {
        assert!(parse(value).is_err(), "{value}");
    }
    assert!(parse(VALID).is_ok());
}

#[tokio::test]
async fn ninety_minutes_stays_within_budget() {
    // 150 spoken words per minute, for 90 minutes.
    let segments = (0..90)
        .map(|_| "meeting word ".repeat(75))
        .collect::<Vec<_>>();
    let mut model = fake(0);
    generate(
        &mut model,
        &budget(),
        &segments,
        &[],
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(model.calls > 2);
}

#[tokio::test]
async fn cancellation_then_restart_has_no_partial_or_duplicate_results() {
    let token = CancellationToken::new();
    let mut model = fake(0);
    model.cancel = Some(token.clone());
    assert!(
        generate(&mut model, &budget(), &["source".into()], &[], &token)
            .await
            .is_err()
    );
    let result = generate(
        &mut model,
        &budget(),
        &["source".into()],
        &["own".into()],
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(result.content.decisions, vec!["Proceed"]);
    assert_eq!(result.content.tasks.len(), 1);
    assert_eq!(result.notes.len(), 1);
    let calls = model.calls;
    assert!(generate(&mut model, &budget(), &[], &[], &token)
        .await
        .is_err());
    assert_eq!(model.calls, calls);
}

#[tokio::test]
async fn cancellation_during_map_then_restart() {
    let segments = vec!["source ".repeat(4000)];
    let token = CancellationToken::new();
    let mut model = fake(0);
    model.cancel = Some(token.clone());
    assert!(generate(&mut model, &budget(), &segments, &[], &token)
        .await
        .is_err());
    let result = generate(
        &mut model,
        &budget(),
        &segments,
        &[],
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(result.content.decisions.len(), 1);
    assert_eq!(result.content.tasks.len(), 1);
}

#[tokio::test]
async fn impossible_budget_makes_no_model_calls() {
    let mut model = fake(0);
    let mut tiny = budget();
    tiny.context = tiny.answer;
    assert!(generate(
        &mut model,
        &tiny,
        &["source".into()],
        &[],
        &CancellationToken::new()
    )
    .await
    .is_err());
    assert_eq!(model.calls, 0);
}

#[tokio::test]
async fn cancellation_wins_when_model_returns_in_the_same_poll() {
    let token = CancellationToken::new();
    let mut model = fake(0);
    model.cancel = Some(token.clone());
    model.wait_after_cancel = false;
    assert!(generate(&mut model, &budget(), &[], &[], &token)
        .await
        .is_err());
}
