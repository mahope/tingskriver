//! Ordered, editable calendar suggestions; selection never rewrites meeting notes.
use crate::calendar::{email, CalendarConfig, Event};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "when", rename_all = "snake_case", deny_unknown_fields)]
pub enum Rule {
    TitleContains { text: String, template: String },
    ExternalAttendee { template: String },
    Default { template: String },
}

fn defaults() -> Vec<Rule> {
    serde_json::from_str(include_str!("../../templates/rules/calendar.json"))
        .expect("valid embedded rules")
}

fn rules_path() -> Result<std::path::PathBuf, String> {
    super::templates::loader::get_custom_templates_dir()
        .map(|p| p.join(".rules/calendar.json"))
        .ok_or_else(|| "Regelmappen kunne ikke findes".into())
}

#[allow(clippy::disallowed_methods)]
pub fn load_rules() -> Result<Vec<Rule>, String> {
    match std::fs::read_to_string(rules_path()?) {
        Ok(json) => parse_rules(&json),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(defaults()),
        Err(e) => Err(e.to_string()),
    }
}

fn parse_rules(json: &str) -> Result<Vec<Rule>, String> {
    let rules: Vec<Rule> =
        serde_json::from_str(json).map_err(|e| format!("Ugyldige skabelonregler: {e}"))?;
    if !matches!(rules.last(), Some(Rule::Default { .. })) {
        return Err("Sidste regel skal være en standardregel".into());
    }
    for rule in &rules {
        let template = match rule {
            Rule::TitleContains { text, template } => {
                if text.trim().is_empty() {
                    return Err("Titelreglen skal indeholde tekst".into());
                }
                template
            }
            Rule::ExternalAttendee { template } | Rule::Default { template } => template,
        };
        super::templates::get_template(template)?;
    }
    Ok(rules)
}

pub async fn save_rules(json: &str) -> Result<(), String> {
    let rules = parse_rules(json)?;
    let path = rules_path()?;
    let parent = path.parent().ok_or("Regelmappen kunne ikke findes")?;
    tokio::fs::create_dir_all(parent)
        .await
        .map_err(|e| e.to_string())?;
    tokio::task::spawn_blocking(move || {
        use std::io::Write;
        let parent = path.parent().ok_or("Regelmappen kunne ikke findes")?;
        let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        file.write_all(&serde_json::to_vec_pretty(&rules).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        file.as_file().sync_all().map_err(|e| e.to_string())?;
        file.persist(path).map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

pub fn select_template(event: Option<&Event>, config: &CalendarConfig) -> String {
    let rules = load_rules().unwrap_or_else(|e| {
        tracing::warn!("Could not load template rules: {e}");
        defaults()
    });
    for rule in rules {
        match rule {
            Rule::TitleContains { text, template }
                if event.is_some_and(|e| e.title.contains(&text)) =>
            {
                return template
            }
            Rule::ExternalAttendee { template } if event.is_some_and(|e| external(e, config)) => {
                return template
            }
            Rule::Default { template } => return template,
            _ => {}
        }
    }
    "standard".into()
}

fn address_domain(address: &str, bare: bool) -> Option<String> {
    let address = email(address.trim());
    let domain = if let Some((local, domain)) = address.rsplit_once('@') {
        if local.is_empty() || local.contains('@') {
            return None;
        }
        domain
    } else if bare {
        address
    } else {
        return None;
    };
    (!domain.is_empty()
        && domain.contains('.')
        && !domain
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '@' | '/' | ':' | '<' | '>')))
    .then(|| domain.to_ascii_lowercase())
}

fn external(event: &Event, config: &CalendarConfig) -> bool {
    let own: Vec<_> = config
        .own_addresses
        .iter()
        .filter_map(|a| address_domain(a, true))
        .collect();
    !own.is_empty()
        && event
            .attendees
            .iter()
            .filter(|a| !a.declined)
            .filter(|a| {
                !config
                    .own_addresses
                    .iter()
                    .any(|own| email(own.trim()).eq_ignore_ascii_case(email(a.email.trim())))
            })
            .filter_map(|a| address_domain(&a.email, false))
            .any(|d| {
                matches!(
                    d.as_str(),
                    "gmail.com"
                        | "googlemail.com"
                        | "outlook.com"
                        | "hotmail.com"
                        | "hotmail.dk"
                        | "live.com"
                        | "live.dk"
                        | "msn.com"
                        | "yahoo.com"
                        | "yahoo.dk"
                        | "icloud.com"
                        | "me.com"
                        | "aol.com"
                        | "proton.me"
                        | "protonmail.com"
                ) || !own.contains(&d)
            })
}
