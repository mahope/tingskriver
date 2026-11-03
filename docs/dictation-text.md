# Dictation text: layer 1, modes and user files

Layer 1 is the deterministic cleanup every dictation gets before the
optional LLM polishing. It lives in `crates/tingskriver-core/src/dictation/logic/`
and is tested with `cargo test -p tingskriver-core dictation`.
The long-text regression checks exact output for 200 and 3,200 sentences,
with best-of-three timings. It allows one second for 200 sentences in a
shared runner's debug build and checks that 16 times the input takes less
than 64 times as long (plus 50 ms scheduler allowance), to catch quadratic
scaling without enforcing a release-build latency on CI.

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

Modes use the shared versioned collection described in [dictation-modes.md](dictation-modes.md).
The same collection drives settings, import/export, shortcut selection and the overlay.
A spoken mode change can re-transcribe when its provider, model or language changes.
"Kun lokal model" in the global settings always wins over a mode's provider.

`formatNumbers` and `formatAddresses` in dictation settings control deterministic
formatting. A mode's `formatting.numberFormat: "ordret"` disables number formatting
for that mode. Kode/rå never polishes, formats numbers or addresses, or presses Enter.
An explicit `insertAndEnter` output removes its final trigger before cleanup;
Enter requires confirmed insertion into the original window and focused field.

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
only while the control that had focus at the previous insertion still has
focus (`GetGUIThreadInfo`), and at most 60 seconds after the insertion.
Browsers and Electron apps draw all fields in one control, so the focused UI
Automation element's runtime id is compared too; if UI Automation answered at
insertion but not now, nothing happens. After Chat's Enter the insertion is
forgotten. The insertion retains its target across clipboard restoration;
paste, typing fallback and Chat's Enter check that the same field still has focus.
Typing or moving the cursor inside the same field within the 60
seconds is not detected. The command sends Backspace or Shift+Left once per
inserted character. After "vælg det", another selection command does nothing
and "slet det" sends one Backspace to remove the selection.

A spoken URL path consumes one word per slash-separated segment, with explicit
hyphens or underscores joining words within a segment. For example,
"mahoje punktum dk skråstreg kontakt i morgen" becomes
"mahoje.dk/kontakt i morgen".

"stop diktering" at the end of a dictation is removed from the text and
reported as `stopDictation`, but nothing acts on it yet: the text is cleaned
after the recording has stopped, so hands-free dictation still ends with a
press of the shortcut. Stopping on the spoken phrase needs live recognition
while recording.
