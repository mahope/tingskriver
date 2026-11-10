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
    prompts: Vec<(String, String)>,
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
        self.prompts.push((system.into(), input.into()));
        assert!(system.contains("Danish"));
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
        prompts: Vec::new(),
    }
}

#[tokio::test]
async fn preserves_notes_and_marks_origins() {
    let notes = MeetingNotes {
        before: vec!["### Plan\n  Min egen note\næøå\r\n".into()],
        during: vec![
            "### Opfølgning\nAftalt under mødet".into(),
            "".into(),
            "".into(),
        ],
    };
    let mut model = fake(0);
    let result = generate(
        &mut model,
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
            .before
            .iter()
            .chain(&notes.during)
            .cloned()
            .collect::<Vec<_>>()
    );
    assert!(result.notes.iter().all(|n| n.origin == Origin::Own));
    let (system, input) = &model.prompts[0];
    assert!(input.contains("<notes_before_meeting>\n### Plan"));
    assert!(input.contains("<notes_during_meeting>\n### Opfølgning"));
    assert!(system.contains("H3"));
    assert!(system.contains("summary"));
    assert_eq!(result.origin, Origin::Ai);
    assert_eq!(result.content.tasks[0].who, None);
}

#[tokio::test]
async fn retries_invalid_json_once() {
    let mut model = fake(1);
    generate(
        &mut model,
        &budget(),
        &[],
        &MeetingNotes::default(),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(model.calls, 2);
    assert!(!model.prompts[0].0.contains("previous response"));
    assert!(model.prompts[1].0.contains("previous response"));
    assert!(!model.prompts[1].0.contains("{invalid"));
    assert_eq!(model.prompts[0].1, model.prompts[1].1);
    let mut model = fake(2);
    assert!(generate(
        &mut model,
        &budget(),
        &[],
        &MeetingNotes::default(),
        &CancellationToken::new()
    )
    .await
    .is_err());
    assert_eq!(model.calls, 2);
}

#[tokio::test]
async fn retries_schema_invalid_and_oversized_answers() {
    for raw in ["{}".into(), "x".repeat(2048)] {
        let mut model = fake(1);
        model.invalid_response = raw;
        let result = generate(
            &mut model,
            &budget(),
            &[],
            &MeetingNotes::default(),
            &CancellationToken::new(),
        )
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
        &VALID.replace("A short summary", ""),
        &VALID.replace("A short summary", "  "),
        &VALID.replace("Check", ""),
        &VALID.replace("Check", "  "),
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
        &MeetingNotes::default(),
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
    assert!(generate(
        &mut model,
        &budget(),
        &["source".into()],
        &MeetingNotes::default(),
        &token
    )
    .await
    .is_err());
    let result = generate(
        &mut model,
        &budget(),
        &["source".into()],
        &MeetingNotes {
            before: vec!["own".into()],
            during: vec![],
        },
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(result.content.decisions, vec!["Proceed"]);
    assert_eq!(result.content.tasks.len(), 1);
    assert_eq!(result.notes.len(), 1);
    let calls = model.calls;
    assert!(
        generate(&mut model, &budget(), &[], &MeetingNotes::default(), &token)
            .await
            .is_err()
    );
    assert_eq!(model.calls, calls);
}

#[tokio::test]
async fn cancellation_during_map_then_restart() {
    let segments = vec!["source ".repeat(4000)];
    let token = CancellationToken::new();
    let mut model = fake(0);
    model.cancel = Some(token.clone());
    assert!(generate(
        &mut model,
        &budget(),
        &segments,
        &MeetingNotes::default(),
        &token
    )
    .await
    .is_err());
    let result = generate(
        &mut model,
        &budget(),
        &segments,
        &MeetingNotes::default(),
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
        &MeetingNotes::default(),
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
    assert!(
        generate(&mut model, &budget(), &[], &MeetingNotes::default(), &token)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn requires_danish_through_map_and_reduce() {
    struct LanguageSensitive {
        maps: usize,
        reductions: usize,
    }

    #[async_trait::async_trait]
    impl Model for LanguageSensitive {
        async fn generate(
            &mut self,
            phase: Phase,
            system: &str,
            input: &str,
        ) -> Result<String, String> {
            assert!(budget().fits(system, input));
            assert!(system.contains("Danish"));
            let summary = "Beslutningen er godkendt";
            match phase {
                Phase::Final => Ok(serde_json::json!({
                    "summary": summary, "decisions": [], "tasks": [], "open_questions": []
                })
                .to_string()),
                Phase::Translation => {
                    panic!("structured reports request Danish directly in every phase")
                }
                Phase::Map | Phase::Reduce => {
                    if phase == Phase::Map {
                        self.maps += 1;
                    } else {
                        self.reductions += 1;
                    }
                    Ok(format!("{summary}. ").repeat(30))
                }
            }
        }
    }

    for (source, expected) in [
        ("Beslutningen er godkendt. ", "Beslutningen er godkendt"),
        ("The decision is approved. ", "Beslutningen er godkendt"),
    ] {
        for minutes in [1, 90] {
            let segments = vec![source.repeat(75); minutes];
            let mut model = LanguageSensitive {
                maps: 0,
                reductions: 0,
            };
            let result = generate(
                &mut model,
                &budget(),
                &segments,
                &MeetingNotes::default(),
                &CancellationToken::new(),
            )
            .await
            .unwrap();
            assert_eq!(result.content.summary, expected);
            if minutes == 90 {
                assert!(model.maps > 1);
                assert!(model.reductions > 0);
            } else {
                assert_eq!(model.maps, 0);
                assert_eq!(model.reductions, 0);
            }
        }
    }
}

#[test]
fn parses_first_complete_object_with_wrapping_and_string_braces() {
    let json = VALID.replace(
        "A short summary",
        r#"En {beslutning} med \"citat\" og \\ sti"#,
    );
    for raw in [
        json.clone(),
        format!("```json\n{json}\n```"),
        format!("Her er referatet:\n{json}\nSlut. {{}} "),
    ] {
        assert_eq!(parse(&raw).unwrap(), parse(&json).unwrap());
    }
    assert!(parse("prosa {\"summary\": \"unfinished").is_err());
}

#[tokio::test]
async fn oversized_notes_fail_before_any_model_call() {
    let mut model = fake(0);
    let notes = MeetingNotes {
        before: vec!["note ".repeat(2048)],
        during: vec![],
    };
    let error = generate(
        &mut model,
        &budget(),
        &["source ".repeat(4000)],
        &notes,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error, "Structured report exceeds context budget");
    assert_eq!(model.calls, 0);
}

#[tokio::test]
async fn returns_last_validation_error() {
    let mut model = fake(2);
    model.invalid_response = "{}".into();
    let error = generate(
        &mut model,
        &budget(),
        &[],
        &MeetingNotes::default(),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error, "Report does not match schema");
    assert_eq!(model.calls, 2);
}
