//! A pure overview of configured AI data flows, including fallback order.
use crate::dictation::{logic::engine::EnginePreference, settings::DictationSettings};
use crate::providers::{Function, ProvidersConfig};
use crate::summary::llm_client::LLMProvider;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataKind {
    Audio,
    Text,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Flow {
    pub function: Function,
    pub data: DataKind,
    pub provider_id: String,
    pub remote: bool,
}

/// Describes possible destinations, not actual delivery or key availability.
/// Addresses and credentials are deliberately excluded.
/// `summary` is parsed from the summary API's selected provider, and dictation
/// includes mode overrides. Neither path currently uses the provider chains.
pub fn overview(
    config: &ProvidersConfig,
    dictation: &DictationSettings,
    summary: Option<&LLMProvider>,
) -> Vec<Flow> {
    let mut flows: Vec<_> = [Function::Transcription, Function::Assistant]
        .into_iter()
        .flat_map(|function| {
            config.chain(function).iter().filter_map(move |link| {
                let provider = config.provider(&link.provider)?;
                config.allows(provider).then(|| Flow {
                    function,
                    data: match function {
                        Function::Transcription | Function::Dictation => DataKind::Audio,
                        _ => DataKind::Text,
                    },
                    provider_id: provider.id.clone(),
                    remote: provider.kind.is_remote(),
                })
            })
        })
        .collect();
    if let Some(provider) = summary {
        let id = match provider {
            LLMProvider::BuiltInAI => "local",
            LLMProvider::Ollama => "ollama",
            LLMProvider::OpenAI => "openai",
            LLMProvider::Claude => "claude",
            LLMProvider::Groq => "groq",
            LLMProvider::OpenRouter => "openrouter",
            LLMProvider::CustomOpenAI => "custom-openai",
        };
        add(
            &mut flows,
            config,
            Function::Summary,
            id,
            *provider != LLMProvider::BuiltInAI,
        );
    }
    if dictation.enabled {
        dictation_flows(&mut flows, config, dictation);
        for mode in &dictation.modes.modes {
            let mut effective = dictation.clone();
            effective.apply_mode(mode);
            dictation_flows(&mut flows, config, &effective);
        }
    }
    flows
}

fn add(
    flows: &mut Vec<Flow>,
    config: &ProvidersConfig,
    function: Function,
    id: &str,
    remote: bool,
) {
    if remote && super::local_only(config) {
        return;
    }
    let flow = Flow {
        function,
        data: if function == Function::Dictation {
            DataKind::Audio
        } else {
            DataKind::Text
        },
        provider_id: id.to_string(),
        remote,
    };
    if !flows.contains(&flow) {
        flows.push(flow);
    }
}

fn dictation_flows(flows: &mut Vec<Flow>, config: &ProvidersConfig, settings: &DictationSettings) {
    if EnginePreference::parse(&settings.engine) == EnginePreference::Auto {
        add(flows, config, Function::Dictation, "syv-ai", true);
    }
    add(flows, config, Function::Dictation, "local", false);
    if settings.polish_enabled {
        let remote = settings.polish_provider == "ollama";
        add(
            flows,
            config,
            Function::DictationPolish,
            if remote { "ollama" } else { "local" },
            remote,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overview_uses_mode_overrides_and_ignores_unused_chains() {
        let mut config = ProvidersConfig::default();
        config.functions.insert(Function::Dictation, vec![]);
        config.functions.insert(Function::Summary, vec![]);
        config.functions.insert(Function::DictationPolish, vec![]);
        let mut settings = DictationSettings {
            enabled: true,
            polish_enabled: false,
            polish_provider: "ollama".into(),
            ..Default::default()
        };
        settings.modes.modes[0].polish = Some(true);
        let flows = overview(&config, &settings, Some(&LLMProvider::CustomOpenAI));
        assert!(flows
            .iter()
            .any(|f| f.function == Function::Dictation && f.remote));
        assert!(flows
            .iter()
            .any(|f| f.function == Function::DictationPolish && f.remote));
        assert!(flows
            .iter()
            .any(|f| f.function == Function::Summary && f.remote));
        settings.engine = "local".into();
        settings.modes.modes[0].provider = Some("syv".into());
        assert!(!overview(&config, &settings, None)
            .iter()
            .any(|f| f.function == Function::Dictation && f.remote));
    }

    #[test]
    fn overview_follows_configuration_and_local_only() {
        let mut config = ProvidersConfig::default();
        let mut dictation = DictationSettings {
            enabled: true,
            polish_enabled: true,
            polish_provider: "ollama".into(),
            ..Default::default()
        };
        let flows = overview(&config, &dictation, Some(&LLMProvider::OpenAI));
        assert_eq!(flows[0].data, DataKind::Audio);
        assert!(flows
            .iter()
            .any(|f| f.function == Function::Summary && f.provider_id == "openai" && f.remote));
        assert!(!flows
            .iter()
            .any(|f| f.function == Function::Summary && f.provider_id == "local"));
        assert!(flows
            .iter()
            .any(|f| f.function == Function::Dictation && f.remote));
        assert!(flows.iter().any(|f| f.function == Function::DictationPolish
            && f.provider_id == "ollama"
            && f.remote));
        config.local_only = true;
        config.functions.insert(Function::Assistant, vec![]);
        let flows = overview(&config, &dictation, Some(&LLMProvider::OpenAI));
        assert_eq!(flows.len(), 2);
        assert!(flows.iter().all(|f| !f.remote));
        assert_eq!(flows.last().unwrap().function, Function::Dictation);
        config.local_only = false;
        dictation.engine = "local".into();
        assert!(!overview(&config, &dictation, None)
            .iter()
            .any(|f| f.function == Function::Dictation && f.remote));
    }
}
