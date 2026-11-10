//! User templates use the existing override directory and portable JSON schema.
use super::{get_template, validate_and_parse_template};
use std::path::PathBuf;

pub(super) fn validate_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
    {
        return Err(
            "Skabelon-id må kun indeholde bogstaver, tal, bindestreg og understregning".into(),
        );
    }
    Ok(())
}

/// Create a custom template or edit an existing one, including a built-in override.
/// Validation happens before touching disk; replacement is atomic.
pub async fn save_template(id: &str, json: &str) -> Result<(), String> {
    let directory =
        super::loader::get_custom_templates_dir().ok_or("Skabelonmappen kunne ikke findes")?;
    save_in(directory, id, json).await
}

async fn save_in(directory: PathBuf, id: &str, json: &str) -> Result<(), String> {
    validate_id(id)?;
    let template = validate_and_parse_template(json)?;
    let json = serde_json::to_vec_pretty(&template).map_err(|e| e.to_string())?;
    let id = id.to_owned();
    tokio::task::spawn_blocking(move || write_template(directory, id, json))
        .await
        .map_err(|e| e.to_string())?
}

#[allow(clippy::disallowed_methods)] // Synchronous disk worker, called only via spawn_blocking.
fn write_template(directory: PathBuf, id: String, json: Vec<u8>) -> Result<(), String> {
    use std::io::Write;
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(&directory).map_err(|e| e.to_string())?;
    file.write_all(&json).map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist(directory.join(format!("{id}.json")))
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Export the complete effective template, including user edits, as JSON.
pub async fn export_template(id: &str) -> Result<String, String> {
    validate_id(id)?;
    let id = id.to_owned();
    tokio::task::spawn_blocking(move || {
        serde_json::to_string_pretty(&get_template(&id)?).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn create_edit_and_reject_invalid_replacements() {
        let dir = tempfile::tempdir().unwrap();
        let json = export_template("standard").await.unwrap();
        save_in(dir.path().into(), "custom", &json).await.unwrap();
        let mut template = validate_and_parse_template(&json).unwrap();
        template.name = "Mit referat".into();
        let edited = serde_json::to_string(&template).unwrap();
        save_in(dir.path().into(), "custom", &edited).await.unwrap();
        assert!(save_in(dir.path().into(), "custom", "{}").await.is_err());
        let saved = tokio::fs::read_to_string(dir.path().join("custom.json"))
            .await
            .unwrap();
        assert_eq!(
            validate_and_parse_template(&saved).unwrap().name,
            "Mit referat"
        );
        assert!(save_in(dir.path().into(), "../outside", &json)
            .await
            .is_err());
        assert!(export_template("../outside").await.is_err());
    }
}
