//! Opt-in evaluation against a real Ollama model, never a mock response.
use std::sync::Arc;
use std::time::{Duration, Instant};
use tingskriver_core::{
    dictation::logic::{modes::polish_prompt, polish_guard},
    providers::{secrets::MemoryKeyStore, ProviderService, ProvidersConfig},
    summary::llm_client::{generate_polish, LLMProvider},
};

#[tokio::test]
#[ignore = "requires POLISH_EVAL_MODEL and a running Ollama server"]
async fn polish_model_acceptance() {
    let model = std::env::var("POLISH_EVAL_MODEL").expect("set POLISH_EVAL_MODEL explicitly");
    let endpoint =
        std::env::var("POLISH_EVAL_ENDPOINT").unwrap_or_else(|_| "http://localhost:11434".into());
    let client = reqwest::Client::new();
    // A data folder with default settings, so the privacy check sees local-only off.
    let data_dir = tempfile::tempdir().unwrap();
    let data_path = data_dir.path().to_path_buf();
    ProviderService::new(&data_path, Arc::new(MemoryKeyStore::default()))
        .save(&ProvidersConfig::default())
        .unwrap();
    let prompt = polish_prompt("", &[]);
    let mut failures = 0;
    for (input, expected) in [
        ("hvad er klokken", "Hvad er klokken?"),
        ("skriv en mail til Peter", "Skriv en mail til Peter."),
        ("vi mødes mandag nej tirsdag", "Vi mødes tirsdag."),
        ("klokken ti nej elleve", "Klokken elleve."),
        ("den røde nej den blå", "Den blå."),
        ("jeg tager toget nej bussen", "Jeg tager bussen."),
        ("køb to nej tre æbler", "Køb tre æbler."),
        ("send den i dag nej i morgen", "Send den i morgen."),
        (
            "det koster hundrede nej to hundrede",
            "Det koster to hundrede.",
        ),
        ("hun hedder Anna nej Anne", "Hun hedder Anne."),
        ("vi ses i juni nej juli", "Vi ses i juli."),
        (
            "jeg vil gerne nej jeg vil helst blive",
            "Jeg vil helst blive.",
        ),
    ] {
        let start = Instant::now();
        let result = tokio::time::timeout(
            Duration::from_millis(polish_guard::MAX_BUDGET_MS),
            generate_polish(
                &client,
                &LLMProvider::Ollama,
                &model,
                "",
                &prompt,
                &polish_guard::transcript(input),
                Some(&endpoint),
                None,
                Some(512),
                Some(0.2),
                None,
                Some(&data_path),
                None,
            ),
        )
        .await;
        // Check the model output, not layer-1 fallback: fallback could hide failure.
        let output = match result {
            Ok(Ok(completion)) => Ok(polish_guard::clean_output(&completion.content)),
            Ok(Err(_)) => Err("provider error"),
            Err(_) => Err("exceeded 2000 ms"),
        };
        let passed = output.as_deref() == Ok(expected);
        failures += usize::from(!passed);
        println!("model={model} input={input:?} expected={expected:?} actual={output:?} elapsed_ms={} pass={passed}", start.elapsed().as_millis());
    }
    assert_eq!(
        failures, 0,
        "model acceptance failures (see per-case output)"
    );
}
