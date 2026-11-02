use super::*;

#[test]
fn known_scores() {
    let cases = [
        ("", "", 0, 0, 0),
        ("", "hej", 0, 1, 0),
        ("hej", "", 0, 0, 1),
        ("hej", "hej", 0, 0, 0),
        ("kat", "hund", 1, 0, 0),
        ("en kat", "en stor kat", 0, 1, 0),
        ("en stor kat", "en kat", 0, 0, 1),
        ("Æble Ø Å", "æble ø å", 0, 0, 0),
        ("Hej, verden!", "hej verden", 0, 0, 0),
        ("en\t to\n tre", "en to tre", 0, 0, 0),
        ("42", "toogfyrre", 1, 0, 0),
        ("Aarhus", "Århus", 1, 0, 0),
        ("rød-grøn", "rød grøn", 0, 0, 0),
        ("jeg ser en kat", "du ser kat", 1, 0, 1),
        ("a", "b c d", 1, 2, 0),
    ];
    for (reference, hypothesis, s, i, d) in cases {
        let score = score(reference, hypothesis);
        assert_eq!(
            (score.substitutions, score.insertions, score.deletions),
            (s, i, d)
        );
    }
}

#[test]
fn empty_reference_is_explicit() {
    assert_eq!(score("", "").wer(), Some(0.0));
    assert_eq!(score("", "hej").wer(), None);
    assert_eq!(score("hej", "").wer(), Some(1.0));
}

#[test]
fn aggregate_is_weighted() {
    let mut total = score("en", "to");
    total.add(score(
        "en to tre fire fem seks syv otte ni",
        "en to tre fire fem seks syv otte ni",
    ));
    assert_eq!(total.wer(), Some(0.1));
}

macro_rules! normalization_test {
    ($name:ident, $input:expr, $expected:expr) => {
        #[test]
        fn $name() {
            assert_eq!(normalize($input), $expected);
        }
    };
}
normalization_test!(danish_letters, "ÆØÅ æøå", "æøå æøå");
normalization_test!(case_folding, "HEJ Verden", "hej verden");
normalization_test!(punctuation_boundaries, "hej,verden!", "hej verden");
normalization_test!(whitespace, "  en\tto\n tre  ", "en to tre");
normalization_test!(digits_preserved, "42 1,5", "42 1 5");
normalization_test!(numbers_not_expanded, "tolv 12", "tolv 12");
normalization_test!(aa_not_folded, "Aarhus Århus", "aarhus århus");
normalization_test!(hyphen_boundary, "rød-grøn", "rød grøn");
normalization_test!(apostrophe_boundary, "ta'r", "ta r");
normalization_test!(combining_alphabetic, "a\u{0345}b", "a\u{0345}b");
normalization_test!(empty_input, "", "");
normalization_test!(punctuation_only, "...!?", "");
normalization_test!(no_unicode_composition, "a\u{030a}", "a");

fn valid_manifest(root: &std::path::Path) -> serde_json::Value {
    std::fs::write(root.join("clip.txt"), "fixture").unwrap();
    serde_json::json!({"schema_version":1,"kind":"fixture-text","clips":[{
        "id":"clip", "file":"clip.txt", "reference":"æble", "speech_type":"oplaesning",
        "dialect":"unknown", "source":"test", "license":"MIT", "attribution":"test",
        "consent":"not-applicable"
    }]})
}

#[test]
fn loader_requires_every_field_and_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("manifest.json");
    let valid = valid_manifest(dir.path());
    std::fs::write(&path, serde_json::to_vec(&valid).unwrap()).unwrap();
    assert!(manifest::load(&path).is_ok());
    for key in [
        "id",
        "file",
        "reference",
        "speech_type",
        "dialect",
        "source",
        "license",
        "attribution",
        "consent",
    ] {
        let mut invalid = valid.clone();
        invalid["clips"][0].as_object_mut().unwrap().remove(key);
        std::fs::write(&path, serde_json::to_vec(&invalid).unwrap()).unwrap();
        assert!(manifest::load(&path).is_err(), "missing field: {key}");
    }
    std::fs::write(&path, serde_json::to_vec(&valid).unwrap()).unwrap();
    std::fs::remove_file(dir.path().join("clip.txt")).unwrap();
    assert!(manifest::load(&path).is_err());
}

#[test]
fn loader_rejects_empty_duplicate_unsafe_and_unconsented_clips() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("manifest.json");
    let valid = valid_manifest(dir.path());
    let mut invalids = Vec::new();
    let mut empty = valid.clone();
    empty["clips"] = serde_json::json!([]);
    invalids.push(empty);
    let mut duplicate = valid.clone();
    duplicate["clips"]
        .as_array_mut()
        .unwrap()
        .push(valid["clips"][0].clone());
    invalids.push(duplicate);
    let mut unsafe_path = valid.clone();
    unsafe_path["clips"][0]["file"] = "../clip.txt".into();
    invalids.push(unsafe_path);
    let mut consent = valid.clone();
    consent["kind"] = "audio".into();
    invalids.push(consent);
    let mut blank = valid.clone();
    blank["clips"][0]["license"] = " ".into();
    invalids.push(blank);
    for invalid in invalids {
        std::fs::write(&path, serde_json::to_vec(&invalid).unwrap()).unwrap();
        assert!(manifest::load(&path).is_err());
    }
}

fn wav(rate: u32, channels: u16, samples: usize) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend(b"RIFF");
    b.extend((36 + samples as u32 * 2).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(channels.to_le_bytes());
    b.extend(rate.to_le_bytes());
    b.extend((rate * u32::from(channels) * 2).to_le_bytes());
    b.extend((channels * 2).to_le_bytes());
    b.extend(16u16.to_le_bytes());
    b.extend(b"data");
    b.extend((samples as u32 * 2).to_le_bytes());
    b.resize(44 + samples * 2, 0);
    b
}

#[test]
fn wav_validation_checks_format_duration_and_truncation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.wav");
    std::fs::write(&path, wav(16_000, 1, 128_000)).unwrap();
    assert_eq!(runner::read_pcm16(&path).unwrap().len(), 128_000);
    for bytes in [
        wav(8_000, 1, 100),
        wav(16_000, 2, 100),
        wav(16_000, 1, 128_001),
        wav(16_000, 1, 0),
        vec![0; 10],
        wav(16_000, 1, 10)[..50].to_vec(),
    ] {
        std::fs::write(&path, bytes).unwrap();
        assert!(runner::read_pcm16(&path).is_err());
    }
}

#[test]
fn empty_asset_root_means_current_directory() {
    assert!(manifest::relative_file(std::path::Path::new(""), "Cargo.toml").is_ok());
}
