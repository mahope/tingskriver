# Dictation text: layer 1, modes and user files

Layer 1 is the deterministic cleanup every dictation gets before the
optional LLM polishing. It lives in `crates/tingskriver-core/src/dictation/logic/`
and is tested with `cargo test -p tingskriver-core dictation`.

| Pass | Module | Example |
|---|---|---|
| Voice commands (whole dictation) | `spoken.rs`, `cleanup.rs` | "slet det", "vælg det", "... stop diktering" |
| Literal words | `cleanup.rs` | "skriv ordet komma" → komma |
| Snippets and dictionary | `cleanup.rs` | "mit cvr" → 39413892, "maho je" → Mahoje |
| Addresses | `address.rs` | "anna snabel-a eksempel punktum d k" → anna@eksempel.dk |
| Numbers | `numbers.rs` | "den tredje november" → 3. november, "tolv procent" → 12 % |
| Spoken signs | `spoken.rs` | "parentes start" → (, "linjeskift" → line break |
| Fillers, Enter trigger, capitals | `cleanup.rs` | "øh", "... send" in chat |

Nothing in layer 1 translates. Words it does not recognise are kept as
spoken, which is what keeps English terms in Danish sentences intact
(fixture: `crates/tingskriver-core/tests/fixtures/dictation/blandet_sprog.tsv`).
Dictionary words marked English (`english` column) keep their exact
spelling and case and are never read as signs, numbers or commands; the
polishing prompt lists them and says never to translate.

## Number formats

Numbers up to twelve stay words in running text ("to æbler"); larger ones
become digits. With a unit or fixed context digits are always used: `12 %`,
`5 kr.`, `kl. 8`, `uge 2`, `3. november`. Thousands use a period (`1.350`),
decimals a comma (`12,5 %`), years are not grouped (`i 2026`), times use a
period (`kl. 14.30`), phone numbers are written in pairs (`12 34 56 78`).
"komma" between numbers is a decimal only before a unit ("tolv komma fem
procent"); otherwise it is a comma. Digits the speech model wrote in English
format are converted next to units ("3.5%" → `3,5 %`, "5000 kroner" →
`5.000 kr.`). Words with letters and digits, slashes or dots (file names,
versions, URLs) are never changed. These rules follow common Danish usage and
have not been checked against Sprogrådet's text.

## Modes

A mode bundles engine, model, language, cleanup style, polishing, prompt,
output and formatting overrides, plus the rules that select it:

```json
{
  "format": "tingskriver-modes",
  "version": 1,
  "modes": [
    {
      "id": "chat",
      "name": "Chat",
      "style": "normal",
      "output": "pasteAndEnter",
      "enterTriggers": ["send", "send den"],
      "formatNumbers": null,
      "rules": [
        { "type": "program", "value": "slack.exe" },
        { "type": "titleContains", "value": "Slack" },
        { "type": "triggerWord", "value": "chatbesked" }
      ]
    }
  ]
}
```

Rule types: `program`, `windowClass`, `titleContains`, `triggerWord` (said
first; removed from the text) and `shortcut`. Order: shortcut, trigger word,
program, window class, title, then Standard. A raw match in the older
"style per program" list picks Kode/rå. Kode/rå never polishes, never formats
numbers or addresses and never presses Enter. `Service::export_modes` and
`Service::import_modes` read and write this format; a newer `version` is
refused.

## Spoken signs file

`dictation_spoken.json` in the app data folder (`%APPDATA%\Tingskriver` on
Windows) changes the built-in table. An entry replaces the built-in entries
with the same phrases; `"disabled": true` removes them. A broken file is
reported as the dictation error and the built-in table is used.

```json
{
  "version": 1,
  "entries": [
    { "phrases": ["tankestreg"], "action": { "type": "symbol", "text": "—", "spaceBefore": true, "spaceAfter": true } },
    { "phrases": ["pil højre"], "action": { "type": "symbol", "text": "→", "spaceBefore": true, "spaceAfter": true } },
    { "phrases": ["smiley"], "action": { "type": "quote" }, "disabled": true }
  ]
}
```

Action types: `punct` (`"char": "."`, sentence punctuation), `symbol`,
`quote` (opens or closes a straight double quote), `break` (`"lines": 1` or
`2`) and `command` (`"command": "deleteThat" | "selectThat" | "stopDictation"`).
The file is read at start and when the dictionary changes.

## Voice commands on Windows

"slet det" and "vælg det" act only when they are the whole dictation, and
only while the control that had focus after the previous insertion still has
focus (`GetGUIThreadInfo`). Browsers and Electron apps draw all fields in one
control, so there "the same field" means the same page or app window. The
command sends Backspace or Shift+Left once per inserted character.

"stop diktering" at the end of a dictation is removed from the text and
reported as `stopDictation`, but nothing acts on it yet: the text is cleaned
after the recording has stopped, so hands-free dictation still ends with a
press of the shortcut. Stopping on the spoken phrase needs live recognition
while recording.
