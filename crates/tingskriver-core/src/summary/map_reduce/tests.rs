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
