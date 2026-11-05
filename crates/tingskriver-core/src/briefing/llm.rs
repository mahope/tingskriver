use super::{BriefError, Briefer, MeetingPoints, MAX_OUTPUT_TOKENS, MAX_POINTS, TIMEOUT};
use crate::summary::llm_client::{generate_summary, LLMProvider};
use async_trait::async_trait;
use std::path::PathBuf;

const POINTS_SYSTEM: &str = "Du læser referatet af ét tidligere møde. Skriv højst tre korte nøglepunkter på dansk, ét pr. linje med \"- \" foran. Brug kun det, der står i referatet. Gæt ikke, og tilføj ingen fakta udefra.";
const OVERVIEW_SYSTEM: &str = "Du forbereder brugeren til et kommende møde ud fra nøglepunkter fra tidligere møder. Skriv en kort samlet tekst på dansk (højst fire sætninger). Gentag ikke mødets titel, tid eller deltagere, og brug kun punkterne, ingen fakta udefra.";

/// Calls the chosen summary provider. Use a local one (Ollama or the built-in
/// model) unless the user picked another; the caller decides the provider.
pub struct LlmBriefer {
    pub provider: LLMProvider,
    pub model: String,
    pub api_key: String,
    pub ollama_endpoint: Option<String>,
    pub app_data_dir: Option<PathBuf>,
    pub client: reqwest::Client,
}

impl LlmBriefer {
    async fn ask(&self, system: &str, user: &str) -> Result<String, BriefError> {
        let call = generate_summary(
            &self.client,
            &self.provider,
            &self.model,
            &self.api_key,
            system,
            user,
            self.ollama_endpoint.as_deref(),
            None,
            Some(MAX_OUTPUT_TOKENS),
            Some(0.2),
            None,
            self.app_data_dir.as_ref(),
            None,
        );
        match tokio::time::timeout(TIMEOUT, call).await {
            Ok(Ok(done)) => Ok(done.content),
            Ok(Err(e)) => Err(BriefError::Model(e)),
            Err(_) => Err(BriefError::Model(
                "tidsgrænsen på 45 sekunder blev nået".into(),
            )),
        }
    }
}

pub(super) fn parse_points(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| {
            l.trim()
                .trim_start_matches(|c: char| {
                    matches!(c, '-' | '*' | '•' | '.' | ')') || c.is_ascii_digit()
                })
                .trim()
        })
        .filter(|l| !l.is_empty())
        .take(MAX_POINTS)
        .map(str::to_string)
        .collect()
}

#[async_trait]
impl Briefer for LlmBriefer {
    fn model_id(&self) -> String {
        format!("{:?}:{}", self.provider, self.model)
    }
    async fn key_points(&self, title: &str, minutes: &str) -> Result<Vec<String>, BriefError> {
        let text = self
            .ask(
                POINTS_SYSTEM,
                &format!("Møde: {title}\n\nReferat:\n{minutes}"),
            )
            .await?;
        Ok(parse_points(&text))
    }
    async fn overview(&self, meetings: &[MeetingPoints]) -> Result<String, BriefError> {
        let mut user = String::new();
        for m in meetings {
            user.push_str(&format!(
                "{} ({}):\n",
                m.title,
                m.started_at.format("%Y-%m-%d")
            ));
            for p in &m.points {
                user.push_str(&format!("- {p}\n"));
            }
        }
        self.ask(OVERVIEW_SYSTEM, &user).await
    }
}

#[cfg(test)]
mod tests {
    use super::parse_points;

    #[test]
    fn parses_bullets_and_caps_at_three() {
        let p = parse_points("- Et\n* To\n1. Tre\n\n- Fire");
        assert_eq!(p, ["Et", "To", "Tre"]);
    }
}
