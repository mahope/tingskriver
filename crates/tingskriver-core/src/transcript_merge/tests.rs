use super::*;
use proptest::{
    prelude::*,
    test_runner::{Config, TestRunner},
};
use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    mic: Vec<Segment>,
    system: Vec<Segment>,
    diarized: bool,
    expected: Vec<Expected>,
}
#[derive(Debug, Deserialize, PartialEq)]
struct Expected {
    start_ms: u64,
    slut_ms: u64,
    tekst: String,
    kilde: Kilde,
    label: String,
    overlapper: bool,
    taler_mangler: bool,
}

#[test]
fn json_fixtures() {
    let fixtures = [
        include_str!("../../tests/fixtures/transcript_merge/overlap.json"),
        include_str!("../../tests/fixtures/transcript_merge/gaps.json"),
        include_str!("../../tests/fixtures/transcript_merge/empty_mic.json"),
        include_str!("../../tests/fixtures/transcript_merge/empty_system.json"),
        include_str!("../../tests/fixtures/transcript_merge/both_empty.json"),
        include_str!("../../tests/fixtures/transcript_merge/no_diarization.json"),
        include_str!("../../tests/fixtures/transcript_merge/partial_diarization.json"),
        include_str!("../../tests/fixtures/transcript_merge/returning_speakers.json"),
        include_str!("../../tests/fixtures/transcript_merge/ties.json"),
        include_str!("../../tests/fixtures/transcript_merge/unsorted.json"),
        include_str!("../../tests/fixtures/transcript_merge/reversed.json"),
        include_str!("../../tests/fixtures/transcript_merge/danish.json"),
        include_str!("../../tests/fixtures/transcript_merge/nested.json"),
        include_str!("../../tests/fixtures/transcript_merge/boundaries.json"),
    ];
    for json in fixtures {
        let f: Fixture = serde_json::from_str(json).unwrap();
        let config = MergeConfig {
            system_diariseret: f.diarized,
            ..Default::default()
        };
        let actual: Vec<_> = merge(&f.mic, &f.system, &config)
            .into_iter()
            .map(|s| Expected {
                start_ms: s.start_ms,
                slut_ms: s.slut_ms,
                tekst: s.tekst,
                kilde: s.kilde,
                label: s.taler.label(&config).into_owned(),
                overlapper: s.overlapper,
                taler_mangler: s.taler_mangler,
            })
            .collect();
        assert_eq!(actual, f.expected, "fixture: {json}");
    }
}

fn segments() -> impl Strategy<Value = Vec<Segment>> {
    prop::collection::vec(
        (
            any::<u64>(),
            any::<u64>(),
            ".{0,30}",
            prop::option::of("[a-c]{0,2}"),
        ),
        0..40,
    )
    .prop_map(|rows| {
        rows.into_iter()
            .map(|(start_ms, slut_ms, tekst, taler)| Segment {
                start_ms,
                slut_ms,
                tekst,
                taler,
            })
            .collect()
    })
}
fn monotone(rows: &[MergedSegment]) -> bool {
    rows.windows(2).all(|w| w[0].start_ms <= w[1].start_ms)
}

proptest! {
    #[test]
    fn preserves_order_and_every_text(mic in segments(), system in segments(), diarized in any::<bool>()) {
        let config = MergeConfig {system_diariseret: diarized, ..Default::default()};
        let out = merge(&mic, &system, &config);
        prop_assert!(monotone(&out));
        prop_assert_eq!(out.len(), mic.len() + system.len());
        let mut before: Vec<_> = mic.iter().chain(&system).map(|s| &s.tekst).collect();
        let mut after: Vec<_> = out.iter().map(|s| &s.tekst).collect();
        before.sort(); after.sort();
        prop_assert_eq!(before, after);
        for (i, s) in out.iter().enumerate() {
            let overlaps = out.iter().enumerate().any(|(j,t)| i != j && s.start_ms < s.slut_ms && t.start_ms < t.slut_ms && s.start_ms < t.slut_ms && t.start_ms < s.slut_ms);
            prop_assert_eq!(s.overlapper, overlaps);
            prop_assert_eq!(s.taler_mangler, s.taler == Taler::Ukendt);
        }
    }
}

#[test]
fn property_rejects_end_time_sort_mutant() {
    let mut runner = TestRunner::new(Config {
        failure_persistence: None,
        ..Config::default()
    });
    let result = runner.run(&(1u64..1000), |n| {
        let mic = vec![
            Segment {
                start_ms: 0,
                slut_ms: n + 2,
                tekst: String::new(),
                taler: None,
            },
            Segment {
                start_ms: n,
                slut_ms: n + 1,
                tekst: String::new(),
                taler: None,
            },
        ];
        let mut out = merge(&mic, &[], &MergeConfig::default());
        out.sort_by_key(|s| s.slut_ms); // Deliberate mutation of the ordering contract.
        prop_assert!(monotone(&out));
        Ok(())
    });
    assert!(matches!(
        result,
        Err(proptest::test_runner::TestError::Fail(..))
    ));
}

#[test]
fn labels_use_configuration_and_identity_is_independent_of_label() {
    let config = MergeConfig {
        mikrofon_label: "Mads".into(),
        ukendt_label: "Mangler".into(),
        ..Default::default()
    };
    assert_eq!(Taler::Mig.label(&config), "Mads");
    assert_eq!(Taler::Ukendt.label(&config), "Mangler");
    assert_eq!(Taler::Nummereret(2).label(&config), "Taler 2");
}

#[test]
fn whitespace_id_is_missing_and_zero_length_does_not_hide_overlap() {
    let mic = vec![Segment {
        start_ms: 0,
        slut_ms: 100,
        tekst: "outer".into(),
        taler: None,
    }];
    let system = vec![
        Segment {
            start_ms: 1,
            slut_ms: 1,
            tekst: "point".into(),
            taler: Some("   ".into()),
        },
        Segment {
            start_ms: 2,
            slut_ms: 3,
            tekst: "inner".into(),
            taler: Some("x".into()),
        },
    ];
    let config = MergeConfig {
        system_diariseret: true,
        ..Default::default()
    };
    let out = merge(&mic, &system, &config);
    assert!(out[0].overlapper);
    assert!(!out[1].overlapper);
    assert!(out[1].taler_mangler);
    assert_eq!(out[1].taler, Taler::Ukendt);
    assert!(out[2].overlapper);
    assert_eq!(out[2].taler, Taler::Nummereret(1));
}
