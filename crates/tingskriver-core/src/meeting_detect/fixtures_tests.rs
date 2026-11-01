use super::*;
use serde::Deserialize;
use std::{fs, path::Path};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    steps: Vec<Step>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Step {
    snapshot: Snapshot,
    expected: Decision,
}

#[test]
fn json_fixtures() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/meeting_detect");
    let mut paths: Vec<_> = fs::read_dir(directory)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    paths.sort();
    assert!(paths.len() >= 30, "expected at least 30 fixtures");
    for path in paths {
        assert_eq!(path.extension().and_then(|s| s.to_str()), Some("json"));
        let fixture: Fixture = serde_json::from_slice(&fs::read(&path).unwrap())
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        assert!(!fixture.steps.is_empty(), "{} has no steps", path.display());
        let mut state = DetectorState::default();
        for (index, step) in fixture.steps.into_iter().enumerate() {
            let result = detect(&step.snapshot, &state);
            assert_eq!(result, detect(&step.snapshot, &state), "non-deterministic");
            let mut reordered = step.snapshot.clone();
            reordered.windows.reverse();
            reordered.microphone_users.reverse();
            reordered.calendar.reverse();
            assert_eq!(result, detect(&reordered, &state), "order-dependent");
            assert_eq!(result.0, step.expected, "{} step {index}", path.display());
            state = result.1;
            let json = serde_json::to_string(&state).unwrap();
            assert_eq!(state, serde_json::from_str(&json).unwrap());
        }
    }
}
