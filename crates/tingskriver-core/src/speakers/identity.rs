/// Common webmail domains must never be presented as an employer.
pub const WEBMAIL_DOMAINS: &[&str] = &[
    "gmail.com",
    "googlemail.com",
    "hotmail.com",
    "hotmail.dk",
    "outlook.com",
    "outlook.dk",
    "live.com",
    "live.dk",
    "msn.com",
    "yahoo.com",
    "yahoo.dk",
    "icloud.com",
    "me.com",
    "proton.me",
    "protonmail.com",
    "mail.dk",
    "email.dk",
    "ofir.dk",
    "jubii.dk",
    "sol.dk",
    "webspeed.dk",
    "stofanet.dk",
    "godmail.dk",
    "post.tele.dk",
    "privat.dk",
    "tdcadsl.dk",
    "yousee.dk",
];

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Identity {
    pub name: String,
    /// Domain evidence, not a verified legal company name.
    pub company: Option<String>,
}

pub(crate) fn valid_name(name: &str) -> bool {
    let lower = name.trim().to_lowercase();
    !lower.is_empty()
        && !name.chars().any(char::is_control)
        && !lower
            .split(|c: char| !c.is_alphabetic())
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .windows(2)
            .any(|parts| parts == ["no", "reply"])
        && ![
            "unknown",
            "ukendt",
            "speaker",
            "taler",
            "participant",
            "deltager",
            "user",
            "bruger",
            "test",
            "example",
            "navn",
            "name",
            "null",
            "none",
            "who",
            "invitee",
            "timezone",
            "meeting",
            "link",
            "info",
            "admin",
            "support",
            "noreply",
        ]
        .iter()
        .any(|word| {
            lower
                .split(|c: char| !c.is_alphabetic())
                .any(|part| part == *word)
        })
        && name.split_whitespace().all(|part| {
            part.chars().any(char::is_alphabetic)
                && part
                    .chars()
                    .all(|c| c.is_alphabetic() || c == '-' || c == '\'' || c == '’')
        })
}

/// Derive a tentative identity only from a personal-looking email address.
pub fn from_email(email: &str) -> Option<Identity> {
    let email = email.trim().to_lowercase();
    let (local, domain) = email.split_once('@')?;
    if domain.contains('@')
        || !domain.contains('.')
        || domain.split('.').any(|label| {
            label.is_empty()
                || label.starts_with('-')
                || label.ends_with('-')
                || !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
    {
        return None;
    }
    let local = local.split('+').next()?;
    let normalized: String = local.chars().filter(|c| c.is_alphabetic()).collect();
    if !valid_name(&normalized) {
        return None;
    }
    let cleaned: String = local.chars().filter(|c| !c.is_ascii_digit()).collect();
    let parts: Vec<_> = cleaned.split(['.', '_', '-']).collect();
    if parts.len() < 2
        || parts
            .iter()
            .any(|p| p.is_empty() || !p.chars().all(char::is_alphabetic))
    {
        return None;
    }
    let name = parts
        .iter()
        .map(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ");
    Some(Identity {
        name,
        company: (!WEBMAIL_DOMAINS.contains(&domain)
            && !["gmail", "hotmail", "outlook", "live", "yahoo", "icloud"]
                .contains(&domain.split('.').next().unwrap_or_default()))
        .then(|| domain.to_owned()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_synthetic_identities() {
        for (email, name, company) in [
            ("ida.lund@example.org", "Ida Lund", Some("example.org")),
            (" Asta_Øster@GMAIL.COM ", "Asta Øster", None),
            ("nora.birk@mail.dk", "Nora Birk", None),
            ("nora.birk@post.tele.dk", "Nora Birk", None),
        ] {
            assert_eq!(
                from_email(email),
                Some(Identity {
                    name: name.into(),
                    company: company.map(str::to_owned)
                }),
                "{email}"
            );
        }
        for email in [
            "no.reply@example.org",
            "no_reply@example.org",
            "no-reply@example.org",
            "taler.1@example.org",
            "ida@example.org",
            "ida..lund@example.org",
            "ida.lund@bad",
            "ida.lund@a..dk",
            "ida.lund@a.dk@b.dk",
            "ida.lund@-a.dk",
        ] {
            assert_eq!(from_email(email), None, "{email}");
        }
    }

    #[test]
    fn handles_aliases_digits_and_whole_local_blocklist() {
        for email in ["mads.holst+cal@firma.dk", "mads.holst2@firma.dk"] {
            assert_eq!(from_email(email).unwrap().name, "Mads Holst");
        }
        for email in [
            "test.person@example.org",
            "info.lund@example.org",
            "ida.testing@example.org",
        ] {
            assert!(from_email(email).is_some(), "{email}");
        }
        for local in [
            "info", "test", "noreply", "no.reply", "no-reply", "no_reply",
        ] {
            assert!(from_email(&format!("{local}@example.org")).is_none());
        }
        for domain in [
            "gmail.dk",
            "hotmail.co.uk",
            "outlook.de",
            "live.fr",
            "yahoo.co.jp",
            "icloud.org",
        ] {
            assert_eq!(
                from_email(&format!("ida.lund@{domain}")).unwrap().company,
                None
            );
        }
        assert_eq!(
            from_email("ida.lund@gmail-firma.dk")
                .unwrap()
                .company
                .as_deref(),
            Some("gmail-firma.dk")
        );
    }

    #[test]
    fn rejects_calendar_placeholders() {
        for name in [
            "who",
            "invitee timezone",
            "meeting link",
            "Taler 2",
            "Unknown",
            "Info Support",
            "No Reply",
            "NO-REPLY",
            "NoReply",
        ] {
            assert!(!valid_name(name), "{name}");
        }
        for name in [
            "Ida Lund",
            "Åse Øster",
            "Anne-Lise Birk",
            "Nora O'Birke",
            "Nora O’Birke",
        ] {
            assert!(valid_name(name), "{name}");
        }
    }

    #[test]
    fn webmail_is_not_a_company() {
        for domain in WEBMAIL_DOMAINS {
            assert_eq!(
                from_email(&format!("ida.lund@{domain}")).unwrap().company,
                None,
                "{domain}"
            );
        }
    }
}
