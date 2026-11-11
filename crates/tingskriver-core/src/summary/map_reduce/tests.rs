use super::*;

#[derive(Default)]
struct Fake {
    calls: Vec<(Phase, String)>,
}
#[async_trait::async_trait]
impl Model for Fake {
    async fn generate(
        &mut self,
        phase: Phase,
        _system: &str,
        input: &str,
    ) -> Result<String, String> {
        self.calls.push((phase, input.to_owned()));
        Ok("compact facts ".repeat(4))
    }
}
fn config() -> Budget {
    Budget {
        context: 260,
        answer: 25,
        framing: 20,
        estimate: estimate_tokens,
    }
}
#[tokio::test]
async fn covers_every_segment_with_bounded_hierarchical_deterministic_calls() {
    let segments: Vec<_> = (0..200)
        .map(|i| format!("[segment-{i:04}] Åse: {}", "danske ord ".repeat(12)))
        .collect();
    let mut first = Fake::default();
    let result = prepare(&mut first, &config(), &segments, "final instructions", "")
        .await
        .unwrap();
    assert!(result.stats.reduce_levels > 1);
    for segment in &segments {
        assert!(first
            .calls
            .iter()
            .any(|(phase, text)| *phase == Phase::Map && text.contains(segment)));
    }
    for (_, text) in &first.calls {
        assert!(config().fits(MAP_SYSTEM, text));
    }
    let mut second = Fake::default();
    assert_eq!(
        result.content,
        prepare(&mut second, &config(), &segments, "final instructions", "")
            .await
            .unwrap()
            .content
    );
    assert_eq!(first.calls, second.calls);
}
#[tokio::test]
async fn empty_exact_boundary_and_oversized_segment() {
    let mut model = Fake::default();
    assert!(prepare(&mut model, &config(), &[], "final", "")
        .await
        .unwrap()
        .content
        .is_empty());
    assert!(model.calls.is_empty());
    let budget = Budget {
        context: 100,
        answer: 10,
        framing: 0,
        estimate: |s| s.chars().count(),
    };
    assert_eq!(
        pack(&["x".repeat(90)], &budget, "", "", false).unwrap(),
        vec!["x".repeat(90)]
    );
    let input = format!(
        "{} {} Slutmarkør.",
        "Lang sætning. ".repeat(200),
        "æ".repeat(600)
    );
    let pieces = split_segment(&input, &config(), MAP_SYSTEM, "").unwrap();
    assert_eq!(pieces.concat(), input);
    prepare(&mut model, &config(), &[input], "final", "")
        .await
        .unwrap();
}
#[tokio::test]
async fn refuses_non_contracting_model_and_impossible_budget() {
    struct Echo;
    #[async_trait::async_trait]
    impl Model for Echo {
        async fn generate(&mut self, _: Phase, _: &str, text: &str) -> Result<String, String> {
            Ok(text.into())
        }
    }
    assert!(
        prepare(&mut Echo, &config(), &["words ".repeat(1000)], "final", "")
            .await
            .is_err()
    );
    assert!(pack(
        &["abc".into()],
        &Budget {
            context: 1,
            ..config()
        },
        MAP_SYSTEM,
        "",
        true
    )
    .is_err());
}

#[tokio::test]
async fn reduction_must_make_progress_even_for_legal_sized_outputs() {
    struct Echo;
    #[async_trait::async_trait]
    impl Model for Echo {
        async fn generate(&mut self, _: Phase, _: &str, input: &str) -> Result<String, String> {
            Ok(input.into())
        }
    }
    let budget = Budget {
        answer: 150,
        ..config()
    };
    let error = prepare(&mut Echo, &budget, &["ord ".repeat(300)], "final", "")
        .await
        .err()
        .unwrap();
    assert!(error.contains("did not shrink"), "{error}");
}

#[test]
fn overlaps_whole_segments_and_accounts_for_final_custom_context() {
    let segments = (0..30)
        .map(|i| format!("segment-{i} {}", "ord ".repeat(5)))
        .collect::<Vec<_>>();
    let chunks = pack(&segments, &config(), MAP_SYSTEM, "", true).unwrap();
    assert!(chunks.len() > 1);
    assert!(segments
        .iter()
        .any(|s| chunks[0].contains(s) && chunks[1].contains(s)));
    let custom = "æ".repeat(1000);
    assert!(!config().fits("final", &final_input("", &custom)));
}

// -- report quality: early validation, retry, preferred names ---------------

/// A provider that answers from a script and records every prompt.
struct Scripted {
    answers: Vec<String>,
    systems: Vec<String>,
}
#[async_trait::async_trait]
impl Model for Scripted {
    async fn generate(&mut self, _: Phase, system: &str, _: &str) -> Result<String, String> {
        self.systems.push(system.to_owned());
        Ok(self.answers.remove(0))
    }
}
fn roomy() -> Budget {
    Budget {
        context: 100_000,
        answer: 100,
        framing: 20,
        estimate: estimate_tokens,
    }
}
fn standup() -> super::super::templates::Template {
    super::super::templates::get_template("daily_standup").unwrap()
}
fn good_report(template: &super::super::templates::Template) -> String {
    format!("# Titel\n\n**{}**\n\nIndhold", template.sections[0].title)
}

#[tokio::test]
async fn wrong_heading_is_retried_once_with_feedback() {
    let template = standup();
    let mut model = Scripted {
        answers: vec![
            "# Titel\n\n**Helt forkert afsnit**\n\nx".into(),
            good_report(&template),
        ],
        systems: Vec::new(),
    };
    let report = english_report(&mut model, &roomy(), "[0] Hej alle sammen", &template, "")
        .await
        .unwrap();
    assert_eq!(report.content, good_report(&template));
    assert_eq!(model.systems.len(), 2, "exactly one retry");
    assert!(!model.systems[0].contains("Previous attempt failed"));
    assert!(model.systems[1].contains("IMPORTANT: Previous attempt failed."));
    assert!(model.systems[1].contains(&template.sections[0].title));
}

#[tokio::test]
async fn a_valid_answer_is_not_retried_and_a_second_failure_is_not_retried_again() {
    let template = standup();
    let mut model = Scripted {
        answers: vec![good_report(&template)],
        systems: Vec::new(),
    };
    english_report(&mut model, &roomy(), "[0] Hej", &template, "")
        .await
        .unwrap();
    assert_eq!(model.systems.len(), 1);

    // Both attempts are wrong: the better one (it has a title) is kept, no third call.
    let first = "# Titel\n\n**Forkert**\n\nx".to_string();
    let mut model = Scripted {
        answers: vec![first.clone(), "Ingen overskrift".into()],
        systems: Vec::new(),
    };
    let report = english_report(&mut model, &roomy(), "[0] Hej", &template, "")
        .await
        .unwrap();
    assert_eq!(report.content, first);
    assert_eq!(model.systems.len(), 2);
}

#[tokio::test]
async fn dictionary_name_survives_in_the_report() {
    let template = standup();
    let names = vec!["Jørgen Brøndum".to_string(), "Syv.ai".to_string()];
    let rules = super::super::processor::report_extra_rules("[0] Hej", &template, &names);
    // The fake provider copies the names it is told to spell exactly, as a
    // well-behaved model would.
    struct Echo(super::super::templates::Template);
    #[async_trait::async_trait]
    impl Model for Echo {
        async fn generate(&mut self, _: Phase, system: &str, _: &str) -> Result<String, String> {
            let listed: Vec<&str> = system
                .lines()
                .filter_map(|line| line.strip_prefix("- "))
                .filter(|name| !name.starts_with("**"))
                .collect();
            Ok(format!(
                "# Titel\n\n**{}**\n\n{} deltog.",
                self.0.sections[0].title,
                listed.join(" og ")
            ))
        }
    }
    let mut model = Echo(template.clone());
    let report = english_report_with(&mut model, &roomy(), "[0] Hej", &template, "", &rules)
        .await
        .unwrap();
    assert!(report.content.contains("Jørgen Brøndum og Syv.ai"));
    assert!(
        rules.contains("at most"),
        "length guidance is part of the rules"
    );
}
