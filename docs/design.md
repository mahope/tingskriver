# Design: foundation, components, concepts and experiences

Index for everyone who builds screens in the GPUI app, human or agent. It
says what exists, what each piece is responsible for and where to look
before writing anything new. It describes only what is in the code on
`main`; where a screen is still Tauri-only the page says so.

Related: [gpui.md](gpui.md) (version lock, shell, manual Windows tests),
[dansk-tekst.md](dansk-tekst.md) (text and formats), [arkitektur.md](arkitektur.md)
(crate boundaries), [agenter.md](agenter.md) (build and CI loop).

## Reuse check (do this before you build a screen or a component)

1. **Find.** Search this page and the code: `git grep -n "<concept>" crates/tingskriver-app/src`.
   Look at the component table below and at an existing screenshot of a
   similar screen ([Reference](#reference-screenshots)).
2. **Reuse.** If a component covers it, use it. Do not copy its body into a
   page.
3. **Extend.** If it lacks a state (a variant, a disabled or error look),
   add the state to the component and keep every existing caller unchanged
   in behaviour.
4. **Create** a new component only if nothing covers the need. Give it one
   responsibility, a name from the [naming rules](#naming), and a row in the
   component table in the same change.
5. **Check the consumers** before you change a shared component:
   `git grep -n "<name>(" crates/`. Every caller still has to look and work
   the same, or you change them in the same PR.
6. **Update this page** if a component's responsibility or an experience
   changes. `scripts/check-design-doc.py` fails the build when a component
   in the app has no row here, or a row names something that no longer
   exists.

The PR template asks "reused or new component, and why". Answer it with
the step you stopped at.

## Foundation

All values come from `crates/tingskriver-app/src/theme.rs`. A page never
writes a literal color, size or font; it uses a token. Tokens that
gpui-component's theme has no slot for (`recording`, `success`, `warning`)
are read through `Palette`, via the private `palette(cx)` in `ui.rs`.

| Area | Tokens |
|---|---|
| Typography | Hanken Grotesk for text (`typography::SANS`), Literata for page headings (`typography::SERIF`). Sizes `XS` 12, `SM` 13, `BASE` 15, `LG` 17, `XL` 20, `XXL` 26 px; line height 1.55. Fonts are embedded by `assets`. |
| Spacing | `space::XS` 4, `SM` 8, `MD` 12, `LG` 16, `XL` 24, `XXL` 32 px. Nothing between the steps. |
| Radii | `RADIUS` 6 px (small boxes), `RADIUS_LG` 8 px (cards). |
| Layout | Sidebar 256 px, page content at most 760 px wide, notices at most 600 px, empty-state text at most 420 px, default window 1100×720, minimum 640×420 (`layout::*`). |
| Icons | `icon_size::EMPTY_STATE` 28 px, `icon_size::ERROR` 32 px. Icons come from gpui-component's defaults plus the few the shell names in `assets`. |
| Light and dark | `Palette::LIGHT` (warm paper, ink-blue accent) and `Palette::DARK` (cool slate). The window follows the system live; `TINGSKRIVER_APPEARANCE=light\|dark` pins it for tests and screenshots. Every text pair has a WCAG AA contrast test, so a new color pair needs a row in that test. |
| Text | Danish, with æøå, from `tingskriver_core::strings`; never a string literal in a component and never a sentence built with `format!`. See [dansk-tekst.md](dansk-tekst.md). |
| Accessibility | Use gpui-component controls (they carry role and name for UI Automation) and give custom elements an id and a text label. Manual check: [gpui.md](gpui.md#manual-test-on-windows-mads), test 4. |

## Components

One row per module and per UI building block in `crates/tingskriver-app/src`.
The first column is the exact identifier in the code; the script checks it.

### Modules

| Name | Responsibility | Use it for / do not use it for |
|---|---|---|
| `theme` | Tokens (`Palette`, `space`, `typography`, `layout`, `icon_size`, radii), mapping onto gpui-component's theme (`apply`), live light/dark (`sync_with_window`). | All visual values. Not for page structure. |
| `ui` | Main window, `Shell` (sidebar plus the page in the window), the pages and the shared page building blocks below. | New pages and page-level blocks. |
| `meetings` | Virtual meeting list backed by the core list model, inline rename/delete confirmation, startup recovery and navigation handoffs. | Only meeting-list presentation; data, status mapping and mutations live in core. |
| `measurement` | The Måling page: compare a user's sample against a reference text, per model. | Only this page. |
| `about` | Offline build metadata, an update-status snapshot, allowlisted diagnostics and bundled legal documents. | The Om page and copying diagnostics. Opening the page never starts an update check. |
| `probe` | Describes what the keyboard produced (code points, uncomposed dead keys) for the test field on the Diktering page. | Manual input tests. |
| `assets` | Embedded fonts and icons (`Assets`). | Adding a font or an icon. |
| `startup` | Startup clock, first-frame budget log, database open with Danish error classification (`DbFailure`). | Timing and database start. Not for rendering. |
| `window_state` | Saves and restores window position and size (`SavedWindow`). | Window geometry only. |
| `single_instance` | One app per user over a named pipe or socket; a second launch shows the first window. | Process-level behaviour. |
| `args` | Command line (`StartupArgs`): `--hidden`, `--page`. | Start options. |
| `crash` | Panic hook that turns graphics failures into a log entry and a native error dialog. | Never a place for UI. |
| `update` | Velopack installer hooks, background update check and download, applying an update at next start. | Update behaviour. The update screen is not built yet. |
| `proposals` | Review of AI proposals for a meeting's notes and summary: the band "N ventende ændringer", a diff per proposal and Anvend / Afvis / Fortryd. Changes go only through `ProposalStore::apply/undo` on a click. Not mounted in `ui.rs` until the meeting document exists. | AI proposals. Never write notes or summary directly. |

### Page building blocks (in `ui.rs`)

| Name | Responsibility | Use it for / do not use it for |
|---|---|---|
| `Shell` | The window content: sidebar plus the current page, or the database error page instead of both. Owns page selection, appearance observer and the debounced window-position save. | The one root view. Do not add a second root. |
| `Page` | The pages in the sidebar (Møder, Diktering, Måling, Indstillinger, Om), their Danish labels, icons and `--page` names. | Adding a page: add a variant, `label`, `icon`, `from_arg` and the arm in `Shell::render`. |
| `AppModel` | Shared state: database state (`DbState`: loading, not created, ready, failed), data folder, log path, runtime handle. | Reading state in a page. Core is only called on its runtime, never on the UI thread. |
| `sidebar` | Sidebar with the app name and one menu item per `Page`. | Navigation. |
| `page` | Frame around every page: scroll area, heading, optional subtitle, content width limit. | Every page body. Do not rebuild the frame. |
| `heading` | Page heading in Literata. | Page titles and the error page title. |
| `section_title` | Semibold title for a block inside a page. | Grouping within a page. |
| `paragraph` | Body text in the foreground color. | Running text. |
| `muted` | Smaller secondary text in the muted color. | Help texts and details. |
| `setting_group` | Card with a title and muted lines. | Read-only groups on the Indstillinger page. Editable settings need their own component when the first one is built. |
| `loading` | Spinner with a text. | Anything waiting for core (the Møder page while the database opens). |
| `empty_state` | Card with an icon, a title and a short explanation. | Every list without content. Every new list needs one. |
| `meetings_page` | Hosts `MeetingsView`. | Møder navigation. |
| `MeetingsView` | Fixed viewport with GPUI `list`, date groups, durations and job explanations; empty/loading/error states, rename, confirmed deletion and recovery. | Reuses `page`, `loading`, `empty_state`, text blocks, Button and Input. Recording and meeting-document screens remain separate ports; opens the saved audio folder. |
| `dictation_page` | Diktering: placeholder text plus the input test field. | |
| `settings_page` | Indstillinger: read-only groups (appearance, data folder and database, log) and the buttons "Åbn logmappen" and "Afslut". | |
| `about_page` | Om: build metadata, update status, copying diagnostics with progress and result feedback, and bundled legal documents. | Reuses the page frame, text helpers and gpui-component buttons. |
| `database_error_page` | Full-window error: icon, summary, advice, technical error, paths, "Åbn logmappen" and "Prøv igen". | The model for any blocking error: say what happened, what to do, where the log is. |
| `MeasurementView` | The view behind the Måling page (inputs, model choices, result). | |
| `ProposalReview` | The view behind `proposals`: pending proposals for one meeting with their diffs and buttons. | |
| `card` | One proposal in `ProposalReview`: title, diff and the buttons for its state. | Only inside `ProposalReview`. |

### From gpui-component (use as they are)

`Button`, `Input`, `Textarea`, `Sidebar`/`SidebarMenu`, `Spinner`, `Icon`,
`Root`. Wrap one only when the wrapper adds a Tingskriver rule (a token, a
Danish default); do not restyle one inline in a page.

### Outside the app crate

`crates/tingskriver-overlay` draws the pill and the recording indicator in a
native layered window, not in GPUI: `model` (states and timing, testable on
any OS), `layout`, `render` (tiny-skia), `text`, `theme`. `crates/tingskriver-host`
owns the tray icon, global shortcuts and the dictation keyboard hook
([host.md](host.md)). Do not draw a pill inside a GPUI window.

## Concepts

Words the product and the code use. Use the same word in text, names and
docs.

| Concept | Danish in the UI | What it is | Where |
|---|---|---|---|
| Pill | pille | The small floating window at the bottom of the screen: audio level while dictating, "Skriver …" while processing. Never takes focus. It also carries the recording indicator. | `tingskriver-overlay` (`PillState`, `Pill`), started by the host |
| Meeting document | mødedokument | One meeting's page: transcript (microphone and system tracks), own notes, summary, and the approval to deliver. Built in the Tauri app; the GPUI port is a separate issue, so no GPUI component exists yet. | Tauri `frontend/`; data in `tingskriver_core::database`, `notes`, `summary` |
| Queue | kø | Persistent job queue for saved audio that is transcribed after the recording, with retries and back-off. A delivery queue with the same shape exists for delivery. Neither runs during recording. | [transcription-queue.md](transcription-queue.md), `transcription_queue`; [delivery.md](delivery.md) |
| Delivery | aflevering | Sending an approved meeting document to a target (folder, and others as described in delivery.md). Opt-in, blocked by "Kun lokal" and "Privat tilstand". Never sends on save. | [delivery.md](delivery.md), `tingskriver_core::delivery` |
| Microphone track / system track | mikrofonspor / systemspor | The user's own voice / what the PC plays. Always called "microphone" and "system" in code. | [SPEC.md](SPEC.md) |

## Experiences

What a user can do in the GPUI app today and which components make it up.
Change a row when the experience changes.

| Experience | Components | Notes |
|---|---|---|
| Start | `startup`, `Shell`, `loading` | First frame within 500 ms; the database opens in the background. |
| Look at meetings | `meetings_page`, `MeetingsView`, `empty_state`, `loading` | Virtual date groups; open audio folder, rename, confirmed deletion and recovery of interrupted audio. Recording and document handoffs explain the pending ports. |
| Test Danish input | `dictation_page`, `probe` | Manual Windows test 2 and 3 in [gpui.md](gpui.md). |
| Compare transcription on own sample | `MeasurementView`, `Page` | See [measurement.md](measurement.md). |
| See app settings | `settings_page`, `setting_group` | Read-only. |
| See app information and copy diagnostics | `about_page`, `about`, `page`, `paragraph`, `section_title` | Offline metadata and legal documents; diagnostics include only build information and catalogue-owned model names. Update status is a snapshot. |
| Database cannot open | `database_error_page` | Replaces sidebar and page. |
| Second launch, hidden start | `single_instance`, `args` | Shows the existing window; tray is the host's. |
| Dictate | pill (overlay), host | Not a GPUI screen. |

## Naming

- A component is named for what it is, not where it sits: `empty_state`, not
  `meetings_empty_box`. Page-level methods are `<page>_page`.
- One word per concept: "pill", "queue", "delivery", "meeting document" (see
  [Concepts](#concepts)); do not coin synonyms in code or text.
- Tokens, not values: if you need a number twice, it belongs in `theme.rs`.
- Audio sources are "microphone" and "system", never "input"/"output".

## Reference screenshots

- `docs/screenshots/` holds the README images (light and dark) of the Tauri
  UI: meeting list and page, recording, settings, onboarding, dictation and
  the pill. Use them as the target for the screens still to be ported.
- The GPUI shell is captured under Xvfb with `TINGSKRIVER_APPEARANCE` and
  `--page` (see [gpui.md](gpui.md#the-shell)). The mock-data screenshot test
  from #49 is not on `main` yet; when it lands, link its fixtures here and
  make "a screenshot of the new screen in light and dark" part of the reuse
  check.
- Stories alone do not prove behaviour, and a screenshot is not the installed
  app: check the installed build too (the Windows artifact of the CI job,
  [gpui.md](gpui.md#manual-test-on-windows-mads)).

## Check

`python3 scripts/check-design-doc.py` (runs in CI with the GPUI lock check)
verifies that:

- every module in `crates/tingskriver-app/src/lib.rs` has a row,
- every UI function (returns `impl IntoElement` or `AnyElement`) and every
  `impl Render` type in the app has a row,
- every identifier in the first column of the component tables still exists
  in the app source,
- `AGENTS.md` and `CONTRIBUTING.md` link to this page and the pull request
  template has the reuse field.

It reads source text only, takes well under a second and needs no build.

## Meeting-list verification pending #49

After #49 / PR #172 lands, add light and dark fixtures for: empty and loading
lists; grouped meetings with all job statuses and unknown duration; load failure
and retry; recovery banner with multiple candidates and failed/partial recovery;
rename with an empty title; delete confirmation and cancellation; meeting-folder
handoff and the new-recording handoff. Include a narrow window and long titles.
No snapshots are added before that harness is merged.

Core `meeting_list` owns local-date grouping, status explanations, title
validation, deletion guards and recovery candidate selection. Its viewport API
returns borrowed slices; the 1,000-meeting test checks their addresses against
the original storage for every scroll position. The GPUI list only creates
elements for the visible range (plus a small overdraw). Date headers and
meeting rows may have different heights, and action buttons have their own
line so explanations can wrap at the minimum window size. The existing transcription queue is not wired
to a host worker yet (see transcription-queue.md); `load_with_jobs` accepts its
status snapshot when that integration is available and reads persisted queue rows
when the queue table exists in the meeting database. Unchanged rows preserve
viewport measurements across refreshes. The page layout takes an explicit flag
for a virtualized body. A raw-folder OS lock protects active recordings across
processes from recovery, finalization and deletion. Persisted transcripts also
identify transcribed meetings after restart; summary states are case-insensitive
and include cancelled generation. Open meetings are selected by id across list
refreshes. Recovery resolves FFmpeg on a blocking worker before restoring audio.
