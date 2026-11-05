# Dictation: which fields it writes into

Right before the text is inserted, Tingskriver reads four things about the
focused field through UI Automation (`target_win.rs`) and decides in
`dictation/logic/target_check.rs`:

| Field | Result |
|---|---|
| Password field (`IsPassword`: Win32 `ES_PASSWORD`, `<input type=password>`) | Refused. The pill says "Ikke i adgangskodefelt", nothing is inserted or put on the clipboard, and the text is **not saved to the history** (nor the audio) |
| Read-only field (Value pattern `IsReadOnly`) | Refused, pill: "Feltet er skrivebeskyttet". The text stays in the history |
| Disabled field (`IsEnabled` false) | Refused, pill: "Feltet er slået fra" |
| Ordinary field | Inserted |
| Field that does not report read-only (typically browser and Electron fields without a Value pattern) | Inserted. A property that is missing, or a UI Automation call that fails, never blocks a field |
| Focus moved to another field (runtime id differs) or the field cannot be identified | Refused, pill: "Feltet skiftede" |

The control type is not used: terminals, browsers and Electron apps report
many types for fields that take text, and a type allow-list would block them
by mistake.

The check runs three times: when the insertion begins (before the clipboard
is touched), just before Ctrl+V or typing, and before Enter. The runtime id
is compared each time, and a password field that took focus in between is
reported as a password field. Messages never contain the field's content or
the dictated text. Limit: a password field that does not expose `IsPassword`
to UI Automation (some custom-drawn controls) cannot be recognised.

## Test matrix (rows for #79)

| Field | Expected |
|---|---|
| Password field in a Windows dialog | Refused, pill message, no history entry |
| `<input type=password>` in a browser | Refused, pill message, no history entry |
| Read-only field (Notepad "Read-only" file, Word in read-only mode) | Refused, pill message |
| Ordinary field (Notepad, browser text box) | Inserted |
| Field without read-only information (Electron app) | Inserted |
| Focus moved to another field during processing | Refused, "Feltet skiftede" |

Automatic: `cargo test -p tingskriver-core target_check` (decision on a UI
Automation fake). Manual test on Windows:

1. Open a page with `<input type=password>` in a browser, focus it, dictate.
   The pill shows "Ikke i adgangskodefelt", nothing appears, and the new
   dictation is missing from the history (Diktering page).
2. Do the same in a Windows dialog with a password box. The Windows sign-in
   and UAC screens run on the secure desktop; they cannot be reached by any
   program and count as accessibility targets only.
3. Open a file in Notepad, set it read-only (file properties) and dictate:
   the pill says "Feltet er skrivebeskyttet".
4. Dictate into an ordinary Notepad field and a browser text box: text appears.
5. Start dictating in one field and click another before the text is
   inserted: nothing is inserted, the pill says "Feltet skiftede".
