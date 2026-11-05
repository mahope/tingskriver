# Prompt window (issue #200)

A small clickable window for questions that need an answer: "Er du i et møde?"
(#59), a reminder before a calendar meeting (#201) and "Er mødet slut?" with a
countdown. It is not a system toast, it never takes focus from the app the user
is dictating into, and it respects Windows' Focus Assist.

Code: `crates/tingskriver-overlay`.

| Module | What | Tested on |
| --- | --- | --- |
| `prompt` | Requests, buttons, outcomes, keys, shortcuts, countdown with hover pause, accessible names | any OS |
| `prompt_layout` | Geometry in physical pixels (100/150/200 %), hit-test, placement, low work areas | any OS |
| `focus` | Show / defer / drop from `QUERY_USER_NOTIFICATION_STATE` | any OS |
| `windows_prompt` | The Win32 window on its own thread (`PromptWindow`) | Windows |

## Event API (what #59 and #201 use)

```rust
let prompt = PromptWindow::new()?;                         // once, Windows only
prompt.show(PromptRequest::meeting_question(id));          // or meeting_reminder(id, minutes) / meeting_end(id, countdown)
prompt.cancel(id);                                         // withdraw it, no answer

for event in prompt.events().try_iter() {
    match event {
        PromptEvent::Answered { id, outcome } => { /* Chosen(action) | Dismissed | Expired(action) */ }
        PromptEvent::Dropped { id } => { /* held back until out of date, never shown */ }
        PromptEvent::Held(held) => core_overlay.set_prompt_held(held), // the pill's mark
    }
}
```

- One prompt is on screen at a time; further requests queue in order.
- `PromptRequest::ttl` is when the question is moot (a reminder: the meeting
  start). A prompt that is still held back at that time is dropped. A prompt
  that waited behind another one past its `ttl` is dropped too.
- The buttons carry a `PromptAction` (`Record`, `Ignore`, `Later`,
  `StopRecording`, `KeepRecording`). The caller decides what each one does.
- `meeting_end` stops the recording when its countdown runs out
  (`Expired(StopRecording)`). Build the request yourself for another default.
- The question prompt closes itself after 60 s (`Expired(Ignore)`), the
  reminder after 30 s (`Expired(Later)`).
- Hover or keyboard focus pauses the countdown.

## Keyboard and screen readers

A prompt that must not steal focus cannot receive Enter and Esc by itself.
So there are global shortcuts, registered only while a prompt is up:

| Shortcut | Does |
| --- | --- |
| Ctrl+Alt+Enter | The first button (the primary action) |
| Ctrl+Alt+Esc | Close without an answer |
| Ctrl+Alt+P | Move keyboard focus into the prompt (the user asked, so it is not theft). Then Tab / Shift+Tab, Enter, Space and Esc work. The previous foreground window gets focus back when the prompt closes |

The text for settings and the about page is `windows_prompt::keys_hint()`
(`strings::PROMPT_KEYS_HINT`).

The title and body are real `STATIC` controls and the actions real `BUTTON`
controls with their labels as text, and the window has a name
("Tingskriver spørger: <title>"). UI Automation and MSAA therefore expose
names, roles and focus without a hand-written provider (this is the point of
not drawing the popup ourselves, see #73). Dark and light follow Windows'
"app mode".

## Focus Assist and full screen

`SHQueryUserNotificationState` is asked before every show and every 5 s while
a prompt waits:

| State | Prompt |
| --- | --- |
| Accepts notifications, Windows Store app, unknown or unreadable | shown |
| Quiet time (Focus Assist) | held back; the pill gets an amber mark |
| Full-screen app, full-screen Direct3D, presentation mode, screen saver or locked | held back (no mark) |

**Screen sharing is not reported by this API.** A shared window or screen in
Teams, Meet or Zoom does not hold a prompt back by itself; a full-screen
presentation does.

An unreadable state fails open (shows the prompt): a broken query must not
swallow every question.

## Where it is not connected yet

- Meeting detection (#33/#59) and calendar start (#201) are on other branches.
  They create the `PromptWindow` and map the outcomes.
- The GPUI settings page is a stub on `main` (`SETTINGS_LATER`), and there is
  no about page or changelog there. The prompt behaviour (and `keys_hint()`)
  has to be listed on them when they exist.
- No screenshot infrastructure (#49) is on `main`. The pill has a PNG renderer;
  the prompt uses native controls, so its screenshots are part of the manual
  test below. Layout is covered by the numeric tests in `prompt_layout`.

## Manual test plan (Windows)

Preparation: a build with a caller that shows the three prompts (for example
a small example binary or the #59 branch). Open Word with a document.

1. **Dictation continues (acceptance criterion).** Put the caret in Word, hold
   the dictation shortcut and speak. While speaking, show a question prompt.
   Expected: the prompt appears bottom right, Word keeps the caret and the
   title bar stays active, dictation text keeps arriving.
2. **Click without focus change.** Click "Ignorér" (or "Ikke nu"). Expected:
   the prompt closes, Word is still the active window and typing continues.
3. **Shortcuts.** Show a prompt. Ctrl+Alt+Enter answers with the first button;
   show another and press Ctrl+Alt+Esc, which closes it with no answer.
4. **Keyboard focus.** Show a prompt, press Ctrl+Alt+P. Expected: the prompt is
   active with a focus ring on the first button. Tab and Shift+Tab move between
   buttons, Space and Enter press the focused one, Esc closes. Afterwards Word
   is active again.
5. **Screen reader.** Start Narrator (Win+Ctrl+Enter), show a prompt and press
   Ctrl+Alt+P. Expected: it reads the window name, the title and "Optag,
   knap" / "Ignorér, knap".
6. **Countdown.** Show "Er mødet slut?" with 60 s. The text counts down. Hover
   with the mouse: the count stops; move away: it continues. At 0 the
   `Expired(StopRecording)` answer arrives.
7. **Focus Assist.** Settings > System > Focus (Fokusassistent): "Kun alarmer".
   Show a prompt. Expected: no window, the pill (while dictating) has an amber
   dot. Turn Focus Assist off. Expected: the prompt appears within 5 s (if it
   is not out of date). Turn it on again and wait past the `ttl`: a
   `Dropped` event, no prompt, and the dot goes away.
8. **Full-screen presentation.** Start a PowerPoint slide show (or a video in
   full screen). Show a prompt. Expected: held back; after leaving full screen
   it appears (inside its `ttl`).
9. **Two monitors, mixed scaling.** Monitor A at 100 %, B at 150 % or 200 %.
   Put the active window on A, show a prompt: right size, bottom right of A.
   Repeat on B. The prompt follows the monitor of the active window.
10. **Dark and light.** Switch Windows' app mode and show a prompt each time:
    background, text and buttons are readable in both.
11. **Low work area.** Set the display to 1280x720 at 200 % (or a very high
    taskbar) and show the prompt with body text: it stays inside the work
    area and the buttons are visible.
