//! Synchronous ingest validation. Call from a blocking worker in async hosts.
use serde_json::Value;
use std::path::Path;

/// Validate the published v1 sidecar and the fixed files produced by delivery.
/// Errors contain field paths, never meeting content.
#[allow(clippy::disallowed_methods)] // Headless synchronous disk entry point.
pub fn validate_delivery(folder: &Path) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    match std::fs::read(folder.join("sidecar.json")) {
        Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
            Ok(value) => {
                let schema: Value = serde_json::from_str(super::delivery::SIDECAR_SCHEMA)
                    .expect("embedded delivery schema");
                validate_value(&value, &schema, "sidecar", &mut errors);
            }
            Err(_) => errors.push("sidecar.json: ugyldig JSON".into()),
        },
        Err(_) => errors.push("sidecar.json: kunne ikke læses".into()),
    }
    // V1 has no path fields: these names are the export's fixed companions.
    for name in ["transcript.md", "summary.md", "notes.md"] {
        if !folder.join(name).is_file() {
            errors.push(format!("{name}: filen mangler"));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

// Only the keywords used by delivery-v1; no general JSON Schema engine.
fn validate_value(value: &Value, schema: &Value, path: &str, errors: &mut Vec<String>) {
    let matches_type = |kind: &str| match kind {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "null" => value.is_null(),
        _ => false,
    };
    if let Some(kind) = schema.get("type") {
        let valid = match kind {
            Value::String(kind) => matches_type(kind),
            Value::Array(kinds) => kinds.iter().any(|k| k.as_str().is_some_and(matches_type)),
            _ => false,
        };
        if !valid {
            errors.push(format!("{path}: forkert type"));
            return;
        }
    }
    if schema.get("const").is_some_and(|expected| {
        expected != value
            && !(expected.is_number() && value.is_number() && expected.as_f64() == value.as_f64())
    }) {
        errors.push(format!("{path}: forkert konstant eller skemaversion"));
    }
    if let Some(object) = value.as_object() {
        if let Some(required) = schema["required"].as_array() {
            for key in required.iter().filter_map(Value::as_str) {
                if !object.contains_key(key) {
                    errors.push(format!("{path}.{key}: påkrævet felt mangler"));
                }
            }
        }
        for (key, child) in object {
            if let Some(child_schema) = schema["properties"].get(key) {
                validate_value(child, child_schema, &format!("{path}.{key}"), errors);
            } else if schema["additionalProperties"] == false {
                errors.push(format!("{path}.{key}: ukendt felt"));
            }
        }
    }
    if let Some(items) = value.as_array() {
        for (index, child) in items.iter().enumerate() {
            validate_value(child, &schema["items"], &format!("{path}[{index}]"), errors);
        }
    }
    if let Some(text) = value.as_str() {
        if schema["minLength"]
            .as_u64()
            .is_some_and(|min| (text.chars().count() as u64) < min)
            || (schema["pattern"] == "\\S" && text.trim().is_empty())
        {
            errors.push(format!("{path}: teksten er tom"));
        }
        if schema["format"] == "date-time" && chrono::DateTime::parse_from_rfc3339(text).is_err() {
            errors.push(format!("{path}: forventede RFC 3339-dato med tidszone"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Value {
        serde_json::from_str(include_str!(
            "../../../docs/examples/aflevering/gyldig/sidecar.json"
        ))
        .unwrap()
    }

    fn check_supported_schema(schema: &Value) -> Result<(), String> {
        let object = schema.as_object().ok_or("expected schema object")?;
        for (key, value) in object {
            match key.as_str() {
                "$schema" | "$id" | "title" | "const" | "required" | "minLength" => {}
                "additionalProperties" if value.is_boolean() => {}
                "pattern" if value == "\\S" => {}
                "format" if value == "date-time" => {}
                "type" => {
                    let kinds = match value {
                        Value::Array(kinds) => kinds.clone(),
                        _ => vec![value.clone()],
                    };
                    for kind in kinds {
                        if !matches!(kind.as_str(), Some("object" | "array" | "string" | "null")) {
                            return Err(format!("unsupported type: {kind}"));
                        }
                    }
                }
                "properties" => {
                    for child in value
                        .as_object()
                        .ok_or("expected properties object")?
                        .values()
                    {
                        check_supported_schema(child)?;
                    }
                }
                "items" => check_supported_schema(value)?,
                _ => return Err(format!("unsupported keyword or value: {key}: {value}")),
            }
        }
        Ok(())
    }

    #[test]
    fn delivery_validate_schema_uses_only_supported_features() {
        let schema: Value = serde_json::from_str(super::super::delivery::SIDECAR_SCHEMA).unwrap();
        check_supported_schema(&schema).unwrap();
        for unsupported in [
            serde_json::json!({"type": "integer"}),
            serde_json::json!({"type": "number"}),
            serde_json::json!({"type": "boolean"}),
            serde_json::json!({"enum": [1]}),
            serde_json::json!({"minItems": 1}),
            serde_json::json!({"maximum": 1}),
            serde_json::json!({"pattern": "other"}),
            serde_json::json!({"futureKeyword": true}),
        ] {
            for nested in [
                unsupported.clone(),
                serde_json::json!({"properties": {"field": unsupported.clone()}}),
                serde_json::json!({"items": unsupported}),
            ] {
                assert!(check_supported_schema(&nested).is_err(), "{nested}");
            }
        }
    }

    #[test]
    fn delivery_validate_unknown_field_names() {
        let schema: Value = serde_json::from_str(super::super::delivery::SIDECAR_SCHEMA).unwrap();
        let mut value = sample();
        value["unexpected"] = serde_json::json!(true);
        value["opgaver"] = serde_json::json!([{"tekst": "test", "ansvarlig": null, "extra": 1}]);
        let mut errors = vec![];
        validate_value(&value, &schema, "sidecar", &mut errors);
        assert!(errors.contains(&"sidecar.unexpected: ukendt felt".to_string()));
        assert!(errors.contains(&"sidecar.opgaver[0].extra: ukendt felt".to_string()));
    }

    #[test]
    fn delivery_validate_examples() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/aflevering");
        assert_eq!(validate_delivery(&root.join("gyldig")), Ok(()));
        assert!(validate_delivery(&root.join("ugyldig")).is_err());
        assert!(validate_delivery(&root.join("findes-ikke")).is_err());
    }

    #[test]
    fn delivery_validate_required_fields_and_types() {
        let schema: Value = serde_json::from_str(super::super::delivery::SIDECAR_SCHEMA).unwrap();
        for key in ["kilde", "skemaversion", "titel", "tid", "talere", "opgaver"] {
            for missing in [true, false] {
                let mut value = sample();
                if missing {
                    value.as_object_mut().unwrap().remove(key);
                } else {
                    value[key] = serde_json::json!(false);
                }
                let mut errors = vec![];
                validate_value(&value, &schema, "sidecar", &mut errors);
                assert!(!errors.is_empty(), "{key}");
            }
        }
        for value in [serde_json::json!(null), serde_json::json!([])] {
            let mut errors = vec![];
            validate_value(&value, &schema, "sidecar", &mut errors);
            assert!(!errors.is_empty());
        }
        let mut numeric_version = sample();
        numeric_version["skemaversion"] = serde_json::json!(1.0);
        let mut errors = vec![];
        validate_value(&numeric_version, &schema, "sidecar", &mut errors);
        assert!(errors.is_empty());
        for (key, bad) in [
            ("kilde", serde_json::json!("other")),
            ("skemaversion", serde_json::json!(2)),
            ("titel", serde_json::json!("  ")),
            ("tid", serde_json::json!("2026-01-01")),
            ("talere", serde_json::json!([1])),
            ("opgaver", serde_json::json!([{"tekst": "test"}])),
            (
                "opgaver",
                serde_json::json!([{"tekst": 1, "ansvarlig": null}]),
            ),
            (
                "opgaver",
                serde_json::json!([{"tekst": "test", "ansvarlig": 1}]),
            ),
            (
                "opgaver",
                serde_json::json!([{"tekst": "test", "ansvarlig": null, "extra": 1}]),
            ),
            ("extra", serde_json::json!(true)),
        ] {
            let mut value = sample();
            value[key] = bad;
            let mut errors = vec![];
            validate_value(&value, &schema, "sidecar", &mut errors);
            assert!(!errors.is_empty(), "{key}");
        }
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // Isolated disk fixtures.
    fn delivery_validate_missing_files_and_bad_json() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("sidecar.json"), sample().to_string()).unwrap();
        assert_eq!(validate_delivery(dir.path()).unwrap_err().len(), 3);
        for name in ["transcript.md", "summary.md", "notes.md"] {
            std::fs::write(dir.path().join(name), "syntetisk").unwrap();
        }
        assert!(validate_delivery(dir.path()).is_ok());
        std::fs::write(dir.path().join("sidecar.json"), "{").unwrap();
        assert_eq!(
            validate_delivery(dir.path()).unwrap_err(),
            ["sidecar.json: ugyldig JSON"]
        );
    }
}
