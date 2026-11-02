# Danish spoken commands

Layer 1 cleanup recognises the following phrases in normal and raw styles.
Matching uses whole words, ignores case and prefers the longest phrase.

| Symbol | Spoken phrases |
| --- | --- |
| `,` | komma |
| `.` | punktum |
| `:` | kolon |
| `;` | semikolon |
| `?` | spørgsmålstegn |
| `!` | udråbstegn |
| `-` | bindestreg |
| `/` | skråstreg |
| `\` | omvendt skråstreg |
| `(` | parentes start, start parentes |
| `)` | parentes slut, slut parentes |
| `[` / `]` | kantet parentes start / slut |
| `{` / `}` | tuborgklamme start / slut |
| `"` | anførselstegn (alternates opening and closing) |
| `'` | apostrof |
| `@` | snabel-a, snabel a |
| `%` | procent |
| `+` | plus |
| `=` | lig med |
| `#` | hashtag |
| `&` | og tegn |
| `*` | stjerne |
| `_` | understregning |
| `<` / `>` | mindre end / større end |
| vertical bar | lodret streg |
| `~` | tilde |
| `^` | hat |
| `€` / `$` / `£` | eurotegn / dollartegn / pundtegn |
| `°` | gradtegn |
| `…` | tre prikker |
| `§` | paragraftegn |
| newline | ny linje, ny linie, linjeskift |
| paragraph | nyt afsnit, afsnitsskift |

For example, `parentes start hej parentes slut` becomes `(Hej)` in normal
style, and `a bindestreg b snabel a c` becomes `a-b@c` in raw style.
Spoken punctuation and line breaks can also be dictated on their own to extend
previous text.
Say `skriv ordet punktum` or `skriv ordet komma` to insert the literal word,
including at the beginning of a sentence. Dictionary and snippet output is
also protected from being interpreted as commands again.

Windows punctuation aliases additionally cover left/right and open/closed
parentheses and brackets, explicit quotes, `procenttegn`, `lighedstegn`,
`plustegn`, hash/number signs, ellipsis, division, plus/minus, yen and smileys.
The aliases follow [Microsoft's Danish voice typing tables](https://support.microsoft.com/da-dk/accessibility/windows/use-voice-typing-to-talk-instead-of-type-on-your-pc).

Explicit opening and closing quotes retain their direction, including when
the opening quote belongs to an earlier dictation. For example, `hej lukkende
anførselstegn tak` becomes `hej" tak`, and `startende enkelt anførselstegn hej
lukkende enkelt anførselstegn tak` becomes `'hej' tak`. Opening quotes attach
to the following word; closing quotes attach to the preceding word. The plain
`anførselstegn` command still alternates, while `apostrof` joins words and
`apostrof s` produces a suffix followed by a space before the next word.

## Editing commands

These are recognised only when they make up the entire transcription, with
optional trailing sentence punctuation. They are handled before cleanup,
polishing and history storage.

| Action | Phrases |
| --- | --- |
| Delete the last insertion | slet det, slet dette, fortryd det, udvisk det, kassér det |
| Select the last insertion | vælg det, markér det, marker det |
| Stop without inserting command text | stop diktering, stop med at diktere, stop med at lytte |

The stop action also accepts the Danish Windows stop/pause synonyms.

Commands are interpreted after transcription, when the recording has stopped;
this does not add live command detection while the microphone is open.

Deletion and selection currently support standard Windows `Edit` controls.
The focused field, its full text and selection must still match the verified
insertion. Switching fields, editing the text, moving the caret or a failed
insertion invalidates the operation. Only the newly inserted UTF-16 range is
selected or deleted, even if it originally replaced a selection. Successful
deletion consumes the record; repeated deletion cannot remove earlier text.
Password fields, fields over 65,536 UTF-16 units and custom/browser controls
are rejected. There is no Ctrl+Z/backspace fallback. No field content is logged
or persisted. Requests to another process have a 100 ms timeout each.

## User configuration

Create `spoken-symbols.json` next to `dictation.sqlite` in the application's
data directory (the directory returned by the host's `app_data_dir`). Restart
Tingskriver to load it, or use the existing dictionary/snippet save action,
which reloads cleanup rules. The UI for editing this file is a separate issue.

The file is a JSON array of `[spoken phrase, output]` pairs:

```json
[
  ["snabel", "@"],
  ["komma", ";"],
  ["min linje", "\n"]
]
```

Entries add aliases or override built-in phrases; an empty output disables a
phrase. Existing dictionary/snippet rules take precedence on equal-length
matches. `skriv ordet` escapes work for custom symbol phrases too. Opening and
closing delimiters, connecting symbols and operators use their respective
spacing rules; arbitrary custom output uses spaces on both sides. Invalid JSON
is reported rather than silently ignored. The table is loaded once at startup
or rule reload, with no file reads in the dictation hot path.

## Verification

The platform-independent suite can run without desktop/audio dependencies:

```sh
rustc --edition 2021 --test crates/tingskriver-core/src/dictation/logic/mod.rs -o /tmp/dictation-tests
/tmp/dictation-tests
```

Windows desktop integration tests use real edit controls and the clipboard:

```sh
cargo test -p tingskriver-core --lib insertion_window -- --ignored --test-threads=1
```
