# Tingskriver Privacy Policy

*Last updated: 11 November 2026. Applies to the current Tingskriver app (GPUI) and, where noted, the Tauri app it replaces.*

Tingskriver records, transcribes and summarizes meetings on your own computer. There is no Tingskriver server, no account, no analytics and no telemetry: the project operates nothing that receives your data. This page lists every feature that makes a network request, what leaves your machine, to whom, and how to turn it off. Everything not in the table stays on your machine.

This table is the single source of truth for the in-app privacy page (#84) and for the removal of the last analytics code (#42). A test (`crates/tingskriver-core/tests/privacy_policy.rs`) fails when the code contains a host that this page does not name.

## What leaves your machine

| Feature | What leaves the machine | To whom | How to turn it off |
|---|---|---|---|
| Recording | Nothing. Audio is saved as files in the app's data folder. | Nobody | Always local |
| Meeting transcription with the built-in models (Whisper, Parakeet) | Nothing | Nobody | Always local |
| Download of speech and summary models (only when you start a download) | A standard HTTPS request: your IP address and the requested file name. No meeting data. | `huggingface.co` (Whisper, Parakeet v2, Gemma and Qwen models) and `meetily.towardsgeneralintelligence.com` (Parakeet v3) | Do not download; models you copy into the models folder yourself need no network |
| Summaries with the built-in model (default) | Nothing | Nobody | Default |
| Summaries, assistant and dictation polish through an AI provider you add to a chain | The transcript text (or dictated text) and your prompt. For structured meeting reports, also your own notes, including notes written before and during the meeting. | The provider you chose: Syv.ai (`platform.syv.ai`), OpenAI (`api.openai.com`), OdinCore (`api.odincore.ai`), a custom OpenAI-compatible address you type in, or Ollama (`localhost:11434` unless you point it elsewhere). The Tauri app also supports Anthropic (`api.anthropic.com`), Groq (`api.groq.com`), OpenRouter (`openrouter.ai`) and Google Gemini's OpenAI-compatible endpoint (`generativelanguage.googleapis.com`). | Keep the chain on the built-in model, or remove the provider and its API key. A provider that needs a key is never called without one. |
| Dictation with Syv.ai (engine "auto" and a Syv.ai key saved) | The recorded dictation clip as WAV, your dictionary words as a hint, and the language | `platform.syv.ai` (or the Syv.ai address set in dictation settings) | Set the dictation engine to "local", or delete the key. With no key, dictation runs locally. |
| Calendar (private ICS address) | A request for the calendar file, with the ETag from the last fetch. Redirects are not followed. Titles and participants are read on your machine and are not sent anywhere. | The host of the ICS address you pasted (for example Google or Microsoft). The address is stored in Windows Credential Manager. | Delete the saved address; this removes the stored credential |
| Approved delivery of a meeting | The transcript, approved summary, notes and metadata (no audio) when you approve an export. Channel webhooks get an abbreviated summary. | The folder, webhook (generic, Slack, Discord or Teams) or SSH/SFTP server you configured | Off by default. "Kun lokal" and "Privat tilstand" block every delivery, including tests and queued retries. See [delivery](docs/delivery.md). |
| Update check | A standard HTTPS request for the list of releases and, when a newer version exists, the download: your IP address and the version in the request. No meeting data, no identifier. The first check runs about a minute after start, then every six hours. | GitHub (`github.com`, and `api.github.com` for the release list of the Tauri app and its move to the new app) | Set the environment variable `TINGSKRIVER_UPDATE_FEED=off`. There is no switch in the app yet. |
| Website links on the About page (only when you click them) | Nothing from Tingskriver. Your browser opens the page and sends what any page visit sends, such as your IP address. | `mahoje.dk` and the project site `tingskriver.mahoje.dk` | Do not click the links |
| Logs and crash dumps | Nothing; both stay on your computer. | Nobody | See below |

Sending to a provider or a webhook only happens when you configure it. Those services have their own privacy policies; Tingskriver does not control what they do with the data.

Structured meeting reports send the transcript and your own notes, including notes written before and during the meeting, to the selected model provider. Processing is local by default. Choosing an external provider sends both the transcript and notes to that provider. Original notes are also retained unchanged in the report, marked as your own.

## Keep a meeting entirely local

1. Use the built-in models for transcription and summaries (the default for summaries).
2. Set the dictation engine to "local" or do not enter a Syv.ai key.
3. Do not add an AI provider, a calendar address or a delivery target, or switch on "Kun lokal" and "Privat tilstand".
4. Download the models once, or copy them into the models folder, and set `TINGSKRIVER_UPDATE_FEED=off` if you do not want update checks.

## Where your data is stored

Meetings, transcripts, summaries and settings are in a local database and audio files in the app's data folder. They are ordinary files protected by your Windows account's file permissions; Tingskriver does not encrypt them itself. API keys and webhook addresses are stored in Windows Credential Manager; SSH private keys stay in your own key files.

You can delete a meeting in the app, which removes it from the database. When you uninstall, you can choose to delete the data folder and the log folder as well. If the database cannot be opened at startup, a copy is placed in `backups` in the data folder before the error is shown; nothing is deleted.

## Logs and crash dumps

- **Log**: `%LOCALAPPDATA%\dk.mahoje.tingskriver\logs`, at most three files of 5 MB. It records what the app did (startup, devices, errors, panics with a backtrace), not what was said in a meeting. Before a line is written, e-mail addresses, IP addresses, UUIDs, API keys and tokens, webhook addresses and your Windows profile folder are replaced by placeholders such as `[EMAIL]`, `[IP]` and `[HOME]`. Meeting titles, participant names, calendar titles and transcript text must not be logged directly; a source guard checks the code that handles them. File and folder paths are logged (for example where a recording was saved). In meeting-folder paths matching `<title>_YYYY-MM-DD_HH-MM`, the title is replaced with `[TITLE]`, both inside and outside your profile folder. Other folder names are not masked and may contain personal data. Device-follow lines (`[DEVICE_FOLLOW]`) are written unchanged because they are used for troubleshooting; they contain device names. Read a log before you attach it to a public issue.
- **Crash dumps (old Tauri app only)**: when Tingskriver crashes in native code, Windows Error Reporting writes a minidump (`DumpType` 1: threads, call stacks and loaded modules, no full memory) to `%LOCALAPPDATA%\dk.mahoje.tingskriver\dumps`, at most five. The app sets this up with the per-user registry key `HKCU\Software\Microsoft\Windows\Windows Error Reporting\LocalDumps\tingskriver.exe`. A minidump holds the program's state at the moment of the crash and can contain fragments of what it was working on. They are not masked. On by default; turn it off under Settings > Preferences > "Gem nedbrudsfiler", which removes the registry key. The current GPUI app has no such setting and does not configure these crash dumps; it logs panics instead.
- Neither is uploaded. You decide whether to send one to the developer.

## Changes to this policy

Every release is checked against the table: a new host or provider in the code must be added here before release (see the [release checklist](docs/release-checkliste.md)). Changes appear in this file's history in the GitHub repository and in the release notes.

## Contact

- **GitHub Issues**: [Create an issue](https://github.com/mahope/tingskriver/issues)
- **Community**: [Discord](https://discord.gg/crRymMQBFH)

## Open source

Tingskriver is MIT licensed. You can review exactly what the app does, change it, and run it on your own infrastructure.
