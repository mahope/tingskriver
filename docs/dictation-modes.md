# Dictation modes

Core stores a versioned `modes` collection in `DictationSettings`. Existing
settings acquire four Danish presets without replacing engine or polishing
preferences: **Standard** (normal), **Chat** (short, Slack and Teams),
**Kode/rå** (raw, editors and terminals), and **Formel** (Outlook). A missing override
inherits the dictation settings. The first mode is the fallback.

Rules are alternatives. Known code editors take precedence over other rules;
then an explicit shortcut or trigger wins, then the first matching foreground
rule. Program names accept paths and an optional `.exe`, case-insensitively.
Window classes match exactly; website rules match a substring of the window
title, not a verified URL. The foreground snapshot and settings are captured
when recording starts. The existing per-program raw settings still apply.

```json
{
  "version": 1,
  "modes": [{
    "id": "chat",
    "name": "Chat",
    "provider": "syv",
    "model": "syv-transcribe",
    "language": "da",
    "style": "short",
    "polish": true,
    "prompt": "Bevar fagord og skriv korte sætninger.",
    "formatting": { "numberFormat": "dansk", "codeSwitching": true },
    "output": { "kind": "insertAndEnter", "trigger": "send nu" },
    "rules": [
      { "kind": "program", "value": "Slack.exe" },
      { "kind": "windowClass", "value": "ChatWindow" },
      { "kind": "website", "value": "slack.com" },
      { "kind": "triggerWord", "value": "besked" },
      { "kind": "shortcut", "value": "Ctrl+Alt+C" }
    ]
  }]
}
```

Providers are `auto`, `syv`, or `local`. A local mode's explicit model is
loaded through the existing Parakeet engine, including when `auto` selects
local recognition without a Syv key; a Syv model is sent with the
request. Syv accepts the language override. Parakeet detects language itself.
Style, prompt and formatting instructions are passed to Syv and to optional
polishing. Number format and code switching are prompt policies, not a new
deterministic number formatter. With local recognition and polishing off,
only the existing deterministic normal/raw cleanup runs.

An initial spoken trigger, such as “besked hej”, selects its mode and removes
the prefix. A middle occurrence stays ordinary text. Recognition must happen
before a spoken trigger can be known: the initial mode's provider receives
the recording first. Changing provider, model or language via a spoken trigger
requires a second recognition pass. Use a program rule or dedicated shortcut
when the recording must stay local from the outset. Spoken triggers and Enter
commands are disabled in known editors and terminals.

`insert` is the default output. `insertAndEnter` removes a final whole trigger
phrase (ignoring case, repeated whitespace and trailing sentence punctuation). A trigger in the
middle, or a recording containing only the trigger, does not submit. Enter is
sent only after confirmed insertion, while the same foreground window remains
active from recording start, and never to a known editor. Changing foreground
windows during transcription prevents insertion. An insertion failure never sends Enter.

`ModeCollection::export/import` round-trip JSON and reject unknown versions,
duplicate ids, invalid shortcuts and empty names/rules/Enter triggers.
`Service::export_modes/import_modes` expose persistence using the existing
settings save path. The overlay receives `Overlay::set_mode` and hosts receive
`dictation-mode-changed` with the selected mode. This change adds the core API;
it does not add a settings editor or change the overlay's visual layout.

Run focused tests with `cargo test --locked -p tingskriver-core dictation`.
For the manual foreground microbenchmark, run
`cargo test --locked -p tingskriver-core measure_foreground_matching -- --ignored --nocapture`.
