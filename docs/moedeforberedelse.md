# Meeting preparation (#204)

`tingskriver_core::briefing` builds a short brief from the latest related minutes. It is core only; the UI is not wired yet (see "Wiring").

- **Series match:** same iCalendar `UID` (every instance of a recurring event, including `RECURRENCE-ID` overrides, keeps the master `UID`), then same normalised title, then shared attendee addresses. Newest first within a class; only earlier meetings with non-empty minutes count.
- **Limits:** five meetings, three points each, 512 output tokens, 45 s per model call (`MAX_MEETINGS`, `MAX_POINTS`, `MAX_OUTPUT_TOKENS`, `TIMEOUT`).
- **Cache:** table `briefing_cache`. Key points are keyed by meeting id plus a hash of prompt version, model, title and minutes; the joint text by a hash of the points. Unchanged minutes make no model call.
- **No history:** `build_brief` returns `None`, so nothing is shown and no model is called.
- **Visibility:** `visible(now, upcoming)`: before the meeting and until five minutes after its end.
- **Privacy:** only local minutes are read. The model goes through `Briefer`; `LlmBriefer` uses the summary provider the caller passes. Use a local one (Ollama or the built-in model) as default; nothing leaves the PC unless the user has chosen another provider.
- **Tests:** `cargo test -p tingskriver-core --test briefing` uses a fake `Briefer`.

## Response time and model

Model choice: the default is a local provider (Ollama or the built-in model); the caller passes the provider, and nothing leaves the PC unless the user picks a cloud one. A small instruct model is enough, since each call only summarises one set of minutes into three lines. Response time is not measured yet: the build machine has no local model, so this part of #204 stays open (`Refs #204`). Budget is five key-point calls plus one overview call, each capped at 512 tokens and 45 s, and cached calls cost nothing after the first run. Measure on a real PC with the built-in model and Ollama and record the numbers here.

## Wiring (open)

Neither the calendar nor a meeting-to-calendar link exists in the app on `main`. To finish: store the matched event `UID` and attendees on the meeting when recording starts (`calendar::match_meeting`), load minutes from `summary_processes.result`, call `build_brief` for the next event, and show it on the meeting page/reminder with `strings::BRIEF_*` and a setting to turn it off.
