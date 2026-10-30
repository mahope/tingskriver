//! Which transcription engine to use, and when to fall back.

/// Transcription engine for dictation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engine {
    /// Syv.ai (OpenAI-compatible, Danish).
    Syv,
    /// Local Parakeet model.
    Local,
}

impl Engine {
    pub fn as_str(&self) -> &'static str {
        match self {
            Engine::Syv => "syv",
            Engine::Local => "local",
        }
    }
}

/// The user's engine preference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnginePreference {
    /// Syv.ai when a key is set, otherwise local.
    Auto,
    /// Always local, never send audio out.
    LocalOnly,
}

impl EnginePreference {
    pub fn parse(value: &str) -> Self {
        match value {
            "local" | "local-only" => EnginePreference::LocalOnly,
            _ => EnginePreference::Auto,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            EnginePreference::Auto => "auto",
            EnginePreference::LocalOnly => "local",
        }
    }
}

/// Order in which engines are tried.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub primary: Engine,
    /// Used if the primary fails or is slower than the deadline.
    pub fallback: Option<Engine>,
}

pub fn plan(preference: EnginePreference, has_syv_key: bool, local_available: bool) -> Option<Plan> {
    let use_syv = preference == EnginePreference::Auto && has_syv_key;
    match (use_syv, local_available) {
        (true, true) => Some(Plan {
            primary: Engine::Syv,
            fallback: Some(Engine::Local),
        }),
        (true, false) => Some(Plan {
            primary: Engine::Syv,
            fallback: None,
        }),
        (false, true) => Some(Plan {
            primary: Engine::Local,
            fallback: None,
        }),
        (false, false) => None,
    }
}

/// Result of a primary engine attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Attempt {
    Text(String),
    Failed(String),
    /// Did not answer within the deadline (still running).
    Slow,
}

/// What to do after the primary attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Next {
    Use(String),
    /// Start the fallback. If `keep_primary` the primary is still running and
    /// whichever answers first wins.
    Fallback { keep_primary: bool },
    Fail(String),
}

pub fn after_primary(attempt: Attempt, plan: &Plan) -> Next {
    match (attempt, plan.fallback) {
        (Attempt::Text(text), _) => Next::Use(text),
        (Attempt::Failed(_), Some(_)) => Next::Fallback {
            keep_primary: false,
        },
        (Attempt::Slow, Some(_)) => Next::Fallback { keep_primary: true },
        (Attempt::Failed(error), None) => Next::Fail(error),
        // Nothing to fall back to: keep waiting on the primary.
        (Attempt::Slow, None) => Next::Fallback { keep_primary: true },
    }
}

/// LLM polishing is only used if it finished within the budget and returned
/// something sensible; otherwise the layer 1 text is inserted.
pub fn choose_polished(layer1: &str, polished: Option<&str>, elapsed_ms: u64, budget_ms: u64) -> String {
    match polished.map(str::trim) {
        Some(text) if elapsed_ms <= budget_ms && !text.is_empty() && text.len() <= layer1.len() * 3 + 40 => {
            text.to_string()
        }
        _ => layer1.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_prefers_syv_with_key() {
        assert_eq!(
            plan(EnginePreference::Auto, true, true),
            Some(Plan {
                primary: Engine::Syv,
                fallback: Some(Engine::Local)
            })
        );
        assert_eq!(
            plan(EnginePreference::Auto, true, false),
            Some(Plan {
                primary: Engine::Syv,
                fallback: None
            })
        );
    }

    #[test]
    fn plan_without_key_or_local_only() {
        let local = Some(Plan {
            primary: Engine::Local,
            fallback: None,
        });
        assert_eq!(plan(EnginePreference::Auto, false, true), local);
        assert_eq!(plan(EnginePreference::LocalOnly, true, true), local);
        assert_eq!(plan(EnginePreference::LocalOnly, true, false), None);
        assert_eq!(plan(EnginePreference::Auto, false, false), None);
    }

    #[test]
    fn fallback_decisions() {
        let both = plan(EnginePreference::Auto, true, true).unwrap();
        let syv_only = plan(EnginePreference::Auto, true, false).unwrap();
        assert_eq!(after_primary(Attempt::Text("hej".into()), &both), Next::Use("hej".into()));
        assert_eq!(
            after_primary(Attempt::Failed("500".into()), &both),
            Next::Fallback { keep_primary: false }
        );
        assert_eq!(after_primary(Attempt::Slow, &both), Next::Fallback { keep_primary: true });
        assert_eq!(after_primary(Attempt::Failed("500".into()), &syv_only), Next::Fail("500".into()));
    }

    #[test]
    fn polishing_respects_budget() {
        assert_eq!(choose_polished("hej", Some("Hej."), 800, 2000), "Hej.");
        assert_eq!(choose_polished("hej", Some("Hej."), 2500, 2000), "hej");
        assert_eq!(choose_polished("hej", None, 100, 2000), "hej");
        assert_eq!(choose_polished("hej", Some("  "), 100, 2000), "hej");
        let rambling = "x".repeat(200);
        assert_eq!(choose_polished("hej", Some(&rambling), 100, 2000), "hej");
    }

    #[test]
    fn preference_parsing() {
        assert_eq!(EnginePreference::parse("local"), EnginePreference::LocalOnly);
        assert_eq!(EnginePreference::parse("auto"), EnginePreference::Auto);
        assert_eq!(EnginePreference::parse(""), EnginePreference::Auto);
    }
}
