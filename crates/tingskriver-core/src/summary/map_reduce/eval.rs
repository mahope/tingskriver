//! Synthetic fixtures and text-free measurement records shared by the CLI and tests.
use super::*;

#[derive(Clone, Serialize, Deserialize)]
pub struct Fixture {
    pub id: String,
    pub minutes: usize,
    pub segments: Vec<String>,
    pub decisions: Vec<Vec<String>>,
    pub tasks: Vec<Vec<String>>,
    pub names: Vec<String>,
}
pub fn fixture(minutes: usize) -> Fixture {
    let mut segments = Vec::new();
    // 120 spoken words/minute; timestamps and labels are not counted as speech.
    let filler = "Vi drøfter den fiktive plan og gennemgår muligheder sammen. Der er endnu ingen ny beslutning om dette punkt i dag.";
    for i in 0..minutes * 6 {
        segments.push(format!(
            "[S{i:04} {:02}:{:02}] {}: {filler}",
            i / 6,
            i % 6 * 10,
            if i % 2 == 0 {
                "Alma Vinterdal"
            } else {
                "Birk Måneskov"
            }
        ));
    }
    let facts = [
        "Alma Vinterdal: Vi beslutter at vælge blå mapper. Budgettet er 2400 kroner.",
        "Birk Måneskov: Jeg, Birk Måneskov, bestiller 12 blå mapper senest 2026-12-04.",
        "Alma Vinterdal: Vi beslutter at afholde prøveværkstedet i sal 7.",
        "Alma Vinterdal: Jeg, Alma Vinterdal, sender invitationen senest 2026-12-08.",
    ];
    for (i, fact) in facts.iter().enumerate() {
        let index = (segments.len().saturating_sub(1)) * i / 3;
        if let Some(segment) = segments.get_mut(index) {
            segment.push(' ');
            segment.push_str(fact);
        }
    }
    Fixture {
        id: format!("fiktivt-{minutes:02}min"),
        minutes,
        segments,
        decisions: vec![
            vec!["blå".into(), "mapper".into()],
            vec!["prøveværksted".into(), "7".into()],
        ],
        tasks: vec![
            vec![
                "Birk Måneskov".into(),
                "bestiller".into(),
                "12".into(),
                "mapper".into(),
                "2026-12-04".into(),
            ],
            vec![
                "Alma Vinterdal".into(),
                "sender".into(),
                "invitation".into(),
                "2026-12-08".into(),
            ],
        ],
        names: vec!["Alma Vinterdal".into(), "Birk Måneskov".into()],
    }
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Quality {
    pub decisions_found: usize,
    pub decisions_total: usize,
    pub tasks_found: usize,
    pub tasks_total: usize,
    pub suspect_numbers: Vec<String>,
    pub suspect_names: Vec<String>,
}
fn number_tokens(text: &str) -> std::collections::BTreeSet<String> {
    text.split(|c: char| !c.is_numeric() && c != '-' && c != ',')
        .filter(|s| s.chars().any(char::is_numeric))
        .map(str::to_owned)
        .collect()
}
pub fn quality(fixture: &Fixture, summary: &str) -> Quality {
    let source = fixture.segments.join("\n");
    let lower = summary.to_lowercase();
    // Require all planted fields on the same line; this remains a lexical proxy.
    let found = |facts: &[Vec<String>]| {
        facts
            .iter()
            .filter(|fields| {
                lower
                    .lines()
                    .any(|line| fields.iter().all(|f| line.contains(&f.to_lowercase())))
            })
            .count()
    };
    let source_numbers = number_tokens(&source);
    let words = summary
        .split_whitespace()
        .map(|s| s.trim_matches(|c: char| !c.is_alphabetic()))
        .collect::<Vec<_>>();
    let suspect_names = words
        .windows(2)
        .filter(|pair| {
            pair.iter()
                .all(|w| w.chars().next().is_some_and(char::is_uppercase))
        })
        .map(|pair| pair.join(" "))
        .filter(|name| !source.contains(name))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    Quality {
        decisions_found: found(&fixture.decisions),
        decisions_total: fixture.decisions.len(),
        tasks_found: found(&fixture.tasks),
        tasks_total: fixture.tasks.len(),
        suspect_numbers: number_tokens(summary)
            .difference(&source_numbers)
            .cloned()
            .collect(),
        suspect_names,
    }
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Call {
    pub phase: Phase,
    pub seconds: f64,
    pub estimated_output_tokens: usize,
}
pub struct Meter<M> {
    pub inner: M,
    pub calls: Vec<Call>,
}
#[async_trait::async_trait]
impl<M: Model> Model for Meter<M> {
    async fn generate(
        &mut self,
        phase: Phase,
        system: &str,
        input: &str,
    ) -> Result<String, String> {
        let start = Instant::now();
        let output = self.inner.generate(phase, system, input).await?;
        self.calls.push(Call {
            phase,
            seconds: start.elapsed().as_secs_f64(),
            estimated_output_tokens: estimate_tokens(&output),
        });
        Ok(output)
    }
}
pub struct Fake;
#[async_trait::async_trait]
impl Model for Fake {
    async fn generate(&mut self, _: Phase, _: &str, _: &str) -> Result<String, String> {
        Ok("# Referat\nBeslutning: blå mapper, 2400 kroner.\nBeslutning: prøveværksted i sal 7.\nBirk Måneskov bestiller 12 mapper senest 2026-12-04.\nAlma Vinterdal sender invitation senest 2026-12-08.".into())
    }
}
pub async fn danish_report<M: Model>(
    model: &mut M,
    budget: &Budget,
    fixture: &Fixture,
    template: &str,
) -> Result<Prepared, String> {
    let template = super::super::templates::get_template(template)
        .map_err(|_| "Cannot load evaluation template")?;
    let mut report =
        english_report(model, budget, &fixture.segments.join("\n"), &template, "").await?;
    let system = super::super::processor::translation_system_prompt("Danish");
    let input = format!("Translate the following Markdown document into Danish. Return ONLY the translated Markdown, nothing else.\n\n<document>\n{}\n</document>", report.content);
    if !budget.fits(&system, &input) {
        return Err("Translation exceeds context budget".into());
    }
    report.content = model.generate(Phase::Translation, &system, &input).await?;
    Ok(report)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tasks_require_actions_as_well_as_owners_objects_and_deadlines() {
        let fixture = fixture(2);
        let summary =
            "Birk Måneskov: 12 mapper, 2026-12-04\nAlma Vinterdal: invitation, 2026-12-08";
        assert_eq!(quality(&fixture, summary).tasks_found, 0);
        assert_eq!(
            quality(
                &fixture,
                "Birk Måneskov bestiller 12 mapper senest 2026-12-04.\nAlma Vinterdal sender invitation senest 2026-12-08."
            )
            .tasks_found,
            2
        );
    }
    #[tokio::test]
    async fn fake_runs_actual_report_prompts_and_scores_facts() {
        let fixture = fixture(2);
        let mut model = Meter {
            inner: Fake,
            calls: Vec::new(),
        };
        let report = danish_report(
            &mut model,
            &Budget::for_model("qwen3.5:4b").unwrap(),
            &fixture,
            "daily_standup",
        )
        .await
        .unwrap();
        let score = quality(&fixture, &report.content);
        assert_eq!((score.decisions_found, score.tasks_found), (2, 2));
        assert!(score.suspect_numbers.is_empty());
        assert_eq!(model.calls.len(), 2);
        let bad = quality(&fixture, "Nora Fantasifuld bruger 9999 kroner.");
        assert_eq!(bad.tasks_found, 0);
        assert_eq!(bad.suspect_numbers, vec!["9999"]);
        assert_eq!(bad.suspect_names, vec!["Nora Fantasifuld"]);
        assert_eq!(
            serde_json::to_string(&fixture).unwrap(),
            serde_json::to_string(&super::fixture(2)).unwrap()
        );
    }
}
