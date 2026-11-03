# Hex og Tingskriver

Undersøgt 2. november 2026. **Anbefaling: lær af Hex; integrér det ikke som afhængighed nu.**

## Hvad er det?

[anomalyco/hex](https://github.com/anomalyco/hex) findes på den angivne adresse;
GitHubs API returnerer samme navn, ikke en flytning. Det er lokal diktering:
hold en genvej, tal, slip, og teksten indsættes i det aktive program.
Der er også historik, programafhængige tilstande, transformationer og mødeoptagelse.

Snapshot: oprettet 17. juli 2026, 4.067 stjerner, 240 forks, cirka 1.355 commits
på standardgrenen `dev` (API-pagination, ikke optælling i den overfladiske klon).
Seneste læste commit er
[`7201b2377218bedc551074a2ed4fb3a64cb82db1`](https://github.com/anomalyco/hex/commit/7201b2377218bedc551074a2ed4fb3a64cb82db1),
2. november kl. 22:47 UTC. [v3.3.0](https://github.com/anomalyco/hex/releases/tag/v3.3.0)
blev udgivet samme dag kl. 16:43 UTC og er ikke markeret prerelease.
Tallene kommer fra [repo-API](https://api.github.com/repos/anomalyco/hex),
[commits](https://api.github.com/repos/anomalyco/hex/commits?per_page=1) og
[releases](https://api.github.com/repos/anomalyco/hex/releases?per_page=1).
Projektet er ungt og meget aktivt; [Windows-porten](https://github.com/anomalyco/hex/blob/7201b2377218bedc551074a2ed4fb3a64cb82db1/docs/windows.md)
og mødefunktionerne kaldes fortsat beta. Stjerner og versionsnummer beviser ikke stabilitet.

## Arkitektur og sammenligning

Kildelinks nedenfor er låst til det undersøgte commit.

| Område | Hex | Betydning for Tingskriver |
|---|---|---|
| Sprog og UI | Primært Rust, suppleret af Swift og web-/scriptsprog. Rune-baseret desktop-UI, native platformkode og Rust-moduler i én hovedpakke; mindre workspace-crates. | Tingskrivers UI-frie core, host, Win32-overlay og GPUI-app er allerede adskilt. Rune er ikke en GPUI-komponent, og et UI-skift giver ingen påvist gevinst. |
| STT | `transcribe-cpp` med GGUF-modeller, bl.a. Parakeet; Vulkan på Windows x64. ONNX-backend findes som valgfri benchmark-feature. | Tingskriver har ONNX-Parakeet, whisper-rs og Syv.ai med lokal reserve. Et backendskift kræver dansk WER-, svartids- og hukommelsesmåling, ikke blot et nyt modelnavn. |
| Genveje og UX | Hold, toggle, dobbelttryk, indsæt seneste, HUD, historik og programafhængige tilstande. | Hold/dobbelttryk, pille, historik, snippets og udklipsholdergendannelse findes allerede. Brug Hex som sammenligningsgrundlag for fejltilstande og kontekstvalg. |
| Samtidighed | Streaming afgiver plads til diktering; begrænset lydkø og begrænsede bidder i motoren. | Tingskriver har allerede prewarm, modelkeeper og en pool på op til to ONNX-instanser. Lær af prioritering og køgrænser, så hurtig diktering ikke kræver unødvendig modelduplikering. Ingen hastighedsgevinst er målt her. |
| Lyd | Rubato FFT-resampling med eksplicit håndtering af filterforsinkelse og afslutning. | Dikteringshelperen bruger simpel resampling. Sammenlign først kvalitet og bevarelse af sidste stavelse; kopiering af FFT-kode uden korrekt flush kan give regressioner. |

Kilder: [Cargo.toml](https://github.com/anomalyco/hex/blob/7201b2377218bedc551074a2ed4fb3a64cb82db1/Cargo.toml),
[Windows](https://github.com/anomalyco/hex/blob/7201b2377218bedc551074a2ed4fb3a64cb82db1/docs/windows.md),
[stream-kø og prioritet](https://github.com/anomalyco/hex/blob/7201b2377218bedc551074a2ed4fb3a64cb82db1/src/transcription/stream.rs),
[resampling](https://github.com/anomalyco/hex/blob/7201b2377218bedc551074a2ed4fb3a64cb82db1/src/dictation.rs).
Tingskrivers faktiske grundlag er [README](../../README.md),
[arkitektur](../arkitektur.md), [ydeevne](../ydeevne.md),
[dikteringsservice](../../crates/tingskriver-core/src/dictation/service.rs),
[resampling](../../crates/tingskriver-core/src/dictation/logic/audio.rs) og
[Windows-indsættelse](../../crates/tingskriver-core/src/dictation/win.rs).
README beskriver den nuværende Tauri-app; arkitekturdokumentet og crates viser GPUI-overgangen.

## Genbrug og integration

- **Lær af streamingkontrakten:** [lokale streams](https://github.com/anomalyco/hex/blob/7201b2377218bedc551074a2ed4fb3a64cb82db1/docs/streams.md)
  bruger loopback, bearer-token, ejertoken pr. stream og begrænset backlog.
  Den kunne teknisk bruges som ekstern lokal STT-tjeneste, men kræver en kørende
  Hex med modellen allerede indlæst; ellers returneres 503. Det giver endnu en
  installation og livscyklus ved siden af Tingskrivers eksisterende motorer.
  Brug kontraktens ideer ved egen streaming/MCP, ikke Hex som obligatorisk service.
- **Lær af lokal ordbog:** [vocabulary](https://github.com/anomalyco/hex/blob/7201b2377218bedc551074a2ed4fb3a64cb82db1/docs/features/vocabulary.md)
  kan påvirke selve dekodningen gennem eksperimentel Parakeet-rescoring, ikke kun
  rette tekst bagefter. Funktionen er Labs og slået fra som standard. Den kræver
  Hex' patchede transcribe.cpp og kan ikke flyttes direkte til ONNX-dekoderen.
  Undersøg kun videre med et dansk navnekorpus og kontrol for falske erstatninger.
- **Bevar eksisterende backlog:** streaming (#94), indsættelsesmatrix (#79),
  genvejstilstande (#75), programtilstande (#13/#132), hotwords (#57),
  motorisolering (#71), udbydermåling (#69) og MCP (#105) dækker de oplagte spor.
  Se [åbne issues](https://github.com/mahope/tingskriver/issues).
  Denne undersøgelse foreslår derfor **0 nye issues**. Resampling og motorprioritet
  er sammenligningspunkter under eksisterende arbejde, ikke dokumenterede fejl.

## Licens og undersøgelsesgrænse

Hex er [MIT](https://github.com/anomalyco/hex/blob/7201b2377218bedc551074a2ed4fb3a64cb82db1/LICENSE),
copyright 2026 Christopher Langton. Udvalgt kode kan genbruges i Tingskrivers
MIT-kode, også ved distribution, når copyright og MIT-teksten følger kopierede
eller væsentlige dele. Tilføj konkret attribution i tredjepartsoversigten ved
eventuel kopiering. Ideer kan genimplementeres uden at importere hele pakken.
Modeller og assets har særskilte vilkår: [THIRD_PARTY_NOTICES](https://github.com/anomalyco/hex/blob/7201b2377218bedc551074a2ed4fb3a64cb82db1/THIRD_PARTY_NOTICES.md)
angiver bl.a. CC BY 4.0 for Parakeet Ultra og ISC/MIT for ikoner.
MIT på appen er ikke en samlet godkendelse af alle vægte og afhængigheder;
kontrollér dem særskilt og anvend Tingskrivers [cargo-deny-regler](../../deny.toml).

Kun statisk læsning: en overfladisk klon i `/tmp`, ingen Hex-programmer,
scripts, builds eller installationer kørt. Før kloning blev ejer, historik,
aktualitet og manifest undersøgt via GitHub. Seneste release var under tre døgn
gammel. Manifestet indeholder build.rs/cc, native biblioteker og git-patches;
appen har netværkskode til bl.a. modeldownloads og opdateringer. Antallet af nye
transitive afhængigheder er ikke beregnet, da ingen integration foreslås.
Installationskonklusion: **lad være**. Researchen dokumenterer design og kode,
ikke faktisk dansk genkendelseskvalitet, Windows-adfærd eller ydeevne.
