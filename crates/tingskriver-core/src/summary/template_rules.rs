//! Calendar suggestions only: selecting a template never rewrites meeting notes.
use crate::calendar::{CalendarConfig, Event};

/// A 1:1 title wins over external attendees. With no known own domain,
/// missing calendar data or only declined external attendees, use standard.
/// Internal status remains a manual choice.
pub fn select_template(event: Option<&Event>, config: &CalendarConfig) -> &'static str {
    let Some(event) = event else {
        return "standard";
    };
    if event.title.contains("1:1") {
        return "one_to_one";
    }
    let own_domains: Vec<_> = config
        .own_addresses
        .iter()
        .filter_map(|s| domain(s))
        .collect();
    if !own_domains.is_empty()
        && event
            .attendees
            .iter()
            .filter(|a| !a.declined)
            .filter_map(|a| domain(&a.email))
            .any(|d| !own_domains.iter().any(|own| own.eq_ignore_ascii_case(d)))
    {
        return "kunde";
    }
    "standard"
}

fn domain(address: &str) -> Option<&str> {
    let (local, domain) = address.trim().rsplit_once('@')?;
    (!local.is_empty()
        && !local.contains('@')
        && !domain.is_empty()
        && !domain.chars().any(char::is_whitespace))
    .then_some(domain)
}
