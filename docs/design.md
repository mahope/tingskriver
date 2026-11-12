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
are read through `Palette`, via `palette(cx)` in `ui.rs`. The values are the
Tauri app's (`frontend/src/app/globals.css`, `tailwind.config.js` and
`components/ui/*`); [design-paritet.md](design-paritet.md) maps each one to
its source and plans the screen ports.

| Area | Tokens |
|---|---|
| Typography | Hanken Grotesk for text (`typography::SANS`), Literata for every heading (`typography::SERIF`, Tauri's `h1, h2, h3`). Tailwind steps: `XS` 12, `SM` 14, `BASE` 15 (body), `MD` 16 (`text-base`), `LG` 18, `XL` 20, `XXL` 24, `XXXL` 30, `DISPLAY` 36 px; body line height 1.55 (`LINE_HEIGHT`), headings 1.25 (`LINE_HEIGHT_TIGHT`). Fonts are embedded by `assets`. |
| Headings | `heading(text, Level)`: `Page` 30 regular, `Document` 36 semibold, `Hero` 30 semibold, `Section` 24 regular. `section_title` is 18 semibold serif (Tauri h3). Known deviation: Tauri sets `letter-spacing: -0.01em` on headings; GPUI has no letter spacing, so GPUI headings are a hair wider. |
| Spacing | Tailwind's 4 px grid: `space::XXS` 2, `XS` 4, `XS2` 6, `SM` 8, `MD` 12, `LG` 16, `XL2` 20, `XL` 24, `XXL` 32, `XXXL` 40 px. |
| Radii | `RADIUS_SM` 4 (kbd chips), `RADIUS` 6 (controls), `RADIUS_LG` 8 (cards), `RADIUS_FULL` (pills, switches, dots). |
| Controls | `layout::CONTROL_HEIGHT` 44 (default button), `CONTROL_HEIGHT_SM` 32, `ICON_BUTTON` 36, `ICON_BUTTON_SM` 32, `INPUT_HEIGHT` 36 (inputs, segmented items). Build them with `controls`, never `Button::new`. |
| Layout | Sidebar 256 px (`SIDEBAR_WIDTH`), collapsed 64 px (`SIDEBAR_COLLAPSED_WIDTH`). Content width per page: `TRANSCRIPT_MAX` 704, `DOCUMENT_MAX` 736, `SETTINGS_MAX` 1152, `SETTINGS_SECTION_MAX` 672, `DICTATION_MAX` 768, `ONBOARDING_TEXT_MAX` 448, `NOTICE_MAX` 576; empty-state text at most 420 px. Default window 1100×700 (as Tauri), minimum 640×420. |
| Shadows | `shadow::sm()` (outline and secondary buttons), `shadow::base()` (primary button), `shadow::md()` (recording pill, toasts); the same in both modes. gpui-component's own shadows are on (`theme.shadow`). |
| States | Hover and pressed are the fill painted on the background at an alpha (`over`), as Tauri's `bg-primary/90`: filled buttons hover at `HOVER_ALPHA` 0.9 and press at `PRESSED_ALPHA` 0.8 (Tauri has no pressed state; destructive stops at 0.9 so dark-mode text stays AA), secondary at 0.8 / 0.7, list rows hover at accent 0.6 and are active at `accent` without a border. Outline and ghost buttons hover on `accent`. Selection is `primary` at 0.22. |
| Icons | Lucide. `icon_size::SM` 16 (buttons, rows), `MD` 20 (collapsed sidebar, "Tilbage"), `EMPTY_STATE` 28, `ERROR` 32. `assets` adds the lucide icons the Tauri UI uses that gpui-component's defaults lack, and the logo mark (`assets::LOGO_MARK`). |
| Light and dark | `Palette::LIGHT` (warm paper, ink-blue accent) and `Palette::DARK` (cool slate), value for value the Tauri tokens. The window follows the system live; `TINGSKRIVER_APPEARANCE=light\|dark` pins it for tests and screenshots. Every text pair has a WCAG AA contrast test, and every filled button keeps AA while hovered and pressed, so a new color pair needs a row in that test. |
| Text | Danish, with æøå, from `tingskriver_core::strings`; never a string literal in a component and never a sentence built with `format!`. See [dansk-tekst.md](dansk-tekst.md). |
| Accessibility | Use gpui-component controls (they carry role and name for UI Automation) and give custom elements an id and a text label. Manual check: [gpui.md](gpui.md#manual-test-on-windows-mads), test 4. |

## Components

One row per module and per UI building block in `crates/tingskriver-app/src`.
The first column is the exact identifier in the code; the script checks it.

### Modules

| Name | Responsibility | Use it for / do not use it for |
|---|---|---|
| `theme` | Tokens (`Palette`, `space`, `typography`, `layout`, `icon_size`, `shadow`, radii, hover alphas and `over`), mapping onto gpui-component's theme (`apply`), live light/dark (`sync_with_window`). | All visual values. Not for page structure. |
| `controls` | Buttons, inputs, switches, the segmented control and the recording pill button with the Tauri sizes (see [Controls](#controls-in-controlsrs)). | Every button and text input. Not for page layout. |
| `ui` | Main window, `Shell` (sidebar plus the page in the window), shared state (`AppModel`) and the page building blocks below. | The shell and blocks shared by several pages. |
| `pages` | One file per page, each an `impl Shell` block: `settings` (`settings_page`, `mcp_settings`, `language_group`), `dictation`, `about`, `database_error`. | A page's own code, so each page port owns its file. |
| `meetings` | Virtual meeting list backed by the core list model, inline rename/delete confirmation, startup recovery and navigation handoffs. | Only meeting-list presentation; data, status mapping and mutations live in core. |
| `measurement` | The Måling page: compare a user's sample against a reference text, per model. | Only this page. |
| `about` | Offline build metadata, an update-status snapshot, allowlisted diagnostics and bundled legal documents. | The Om page and copying diagnostics. Opening the page never starts an update check. |
| `probe` | Describes what the keyboard produced (code points, uncomposed dead keys) for the test field on the Diktering page. | Manual input tests. |
| `assets` | Embedded fonts and icons (`Assets`). | Adding a font or an icon. |
| `startup` | Startup clock, first-frame budget log, database open with Danish error classification (`DbFailure`). | Timing and database start. Not for rendering. |
| `window_state` | Saves and restores window position and size (`SavedWindow`). | Window geometry only. |
| `console` | On Windows, attaches to the launching terminal for CLI output, then frees it once startup is decided and clears the console's standard handles. | Process-level behaviour. |
| `single_instance` | One app per user over a named pipe or socket; a second launch shows the first window or queues a core command; parsing and transport live in core. | Process-level behaviour. |
| `args` | Command line (`StartupArgs`): `--hidden`, `--page`. | Start options. |
| `crash` | Panic hook that turns graphics failures into a log entry and a native error dialog. | Never a place for UI. |
| `update` | Velopack installer hooks, background update check and download, applying an update at next start. | Update behaviour. The update screen is not built yet. |
| `proposals` | Review of AI proposals for a meeting's notes and summary: the band "N ventende ændringer", a diff per proposal and Anvend / Afvis / Fortryd. Changes go only through `ProposalStore::apply/undo` on a click. Not mounted in `ui.rs` until the meeting document exists. | AI proposals. Never write notes or summary directly. |

### Controls (in `controls.rs`)

| Name | Responsibility | Use it for / do not use it for |
|---|---|---|
| `button` | A gpui-component `Button` 44 px high, 16 px padding, 14 px medium text, content width, with the shadow of its `Variant` (`Primary`, `Outline`, `Secondary`, `Ghost`, `Danger`, `Success`, `Link`). | Every button. It does not stretch in a column; in a row with taller neighbours add `.self_center()`. |
| `small_button` | Tauri's `size="sm"`: 32 px, 12 px text. | Toolbars and compact rows. |
| `icon_button` | A 36 px square icon button whose label is the accessible name and the tooltip. | Icon-only actions. `small_icon_button` is the 32 px row variant (rename, delete). |
| `input` | A text input 36 px high. | Single-line text fields. |
| `switch` | gpui-component's `Switch` (36×20, primary when on, muted when off). | Boolean settings. |
| `segmented` | A segmented control: card with an `input` border, 36 px items, the selected one primary, with radio-group semantics. | Choosing one of a few options (Udseende). |
| `pill_button` | The fully rounded 44 px recording pill button: `Start` primary, `Pause` outline on the card, `Stop` foreground fill. | The recording pill only. |

### Page building blocks (in `ui.rs` and `pages`)

| Name | Responsibility | Use it for / do not use it for |
|---|---|---|
| `mcp_settings` | MCP access, consent, data controls and audit status. | MCP controls on the Indstillinger page; reuse the shared settings and text blocks. |
| `meeting_sharing` | MCP sharing of the open meeting, with loading/error states (`pages/meeting.rs`). | Choosing whether MCP may read this meeting. Moves into the meeting document header (Job 4). |
| `meeting_page` | The open meeting until the document is ported: title, time and duration, job status, "Åbn lydmappe" and `meeting_sharing`. | Replaced by the meeting document (docs/design-paritet.md, Job 4). |
| `home_page` | The idle home screen: hero "Klar til dit næste møde" and the floating "Start optagelse" pill (`shadow::md`), with the interruption card, recovery card and the "not ported yet" notice above it (`pages/home.rs`). | The default content. Recording itself is Job 3. |
| `Shell` | The window content: sidebar plus the current page, or the database error page instead of both. Owns page selection, sidebar collapse, the home notice, appearance observer and the debounced window-position save. | The one root view. Do not add a second root. |
| `Page` | What the content area shows: `Home` (or the open meeting), Diktering, Indstillinger, Om, and Måling (not in the sidebar, `--page måling`), with their `--page` names (`møder` opens Home). `Page::ALL` is the screenshot inventory. | Adding a page: add a variant, `from_arg`, the arm in `Shell::render`, a sidebar entry if it needs one, and goldens. |
| `AppModel` | Shared state: database state (`DbState`: loading, not created, ready, failed), data folder, log path, runtime handle. | Reading state in a page. Core is only called on its runtime, never on the UI thread. |
| `sidebar` | The Tauri sidebar (`sidebar.rs`): logo mark and wordmark (opens Om), "Ny optagelse" or "Optagelse i gang", the search field, `MeetingsView`, footer rows Diktering / Importér transskript / Importér lyd / Indstillinger and ⓘ (Om); collapses to 64 px icons with tooltips. Focus order: record, search, list, footer, collapse handle. | Navigation. Imports explain that they are not ported yet. |
| `sidebar_header` | Logo, record button and search field (px 16, pt 20). | Only in `sidebar`. |
| `logo` | Logo mark (24 px, 28 px collapsed, primary) and the 20 px semibold Literata wordmark; a click opens Om. Not a tab stop (ⓘ is). | Only in `sidebar`. |
| `record_button` | "Ny optagelse" (primary, 44 px, `rounded-lg`), or the "Optagelse i gang" status with a recording dot on an 8 % recording tint while a meeting records. | Only in `sidebar`. |
| `sidebar_footer` | Footer rows (44 px, 16 px icons, `bg-accent` on the current page) and ⓘ. | Only in `sidebar`. |
| `collapsed_column` | The 64 px column: logo, round record button, Møder (unfolds), imports, Diktering, Indstillinger, ⓘ; 44 px squares with 20 px icons and tooltips. | Only in `sidebar`. |
| `collapse_handle` | The 24 px round chevron on the sidebar's right edge; drawn last by the shell so it sits over the border. | Folding the sidebar in and out. |
| `page` | Frame around every page: scroll area, 30 px page title, optional subtitle and the page's content width (`layout::*_MAX`). | Every page body. Do not rebuild the frame. |
| `heading` | A Literata heading at one of the Tauri levels (`Level::Page`, `Document`, `Hero`, `Section`). | Page, document and section titles. |
| `section_title` | 18 px semibold Literata title for a block inside a page (Tauri h3). | Grouping within a page. |
| `paragraph` | Body text in the foreground color. | Running text. |
| `muted` | Smaller secondary text in the muted color. | Help texts and details. |
| `setting_group` | Card with a title and muted lines. | Read-only groups on the Indstillinger page. Editable settings need their own component when the first one is built. |
| `language_group` | Card on Indstillinger: meeting language buttons (primary and extra languages), whether reports are written in the primary language, and the save error. | The meeting and report language settings (#209). Reuses gpui-component buttons. |
| `loading` | Spinner with a text. | Anything waiting for core (the sidebar list while the database opens). |
| `interrupted_card` | Dismissible alert with the reason, time and audio status of an interrupted recording, and a reminder that recording does not restart automatically. | The home screen after sleep or logoff interrupts a recording. Uses the persisted `InterruptNotice`. |
| `MeetingsView` | The meeting list in the sidebar: GPUI virtual `list` over core's `meeting_list::List` (borrowed through row indices), day groups ("I dag", "I går", "mandag 26. oktober"), Tauri rows (title, time, short job status, search snippet), hover/focus actions Omdøb and Slet with the "Omdøb møde" and "Bekræft sletning" dialogs, full-text search through core (`search::search`, title filter at once), empty/loading/error/no-match states and the recovery card for Home. | Every list without content says what to do next in its own slot (Tauri text), not a card. |
| `meeting_row` | One row of the virtual list: a day heading (12 px medium muted) or a meeting (44 px min, `rounded-md`, `bg-accent` when open, accent/60 on hover) with its action chip. | Only in `MeetingsView`. |
| `list_message` | Muted 14 px text in the list slot, optionally with a title (empty list, no match, load errors). | Only in `MeetingsView`. |
| `search_field` | The sidebar search input: search icon, clear X. | Only in the sidebar. |
| `recovery_card` | Card with "Gendan optagelse" buttons for interrupted recordings. | Shown on Home. |
| `dictation_page` | Diktering: placeholder text plus the input test field. | |
| `settings_page` | Indstillinger: read-only groups (appearance, data folder and database, log) and the buttons "Åbn logmappen" and "Afslut". | |
| `about_page` | Om: build metadata, update status, copying diagnostics with progress and result feedback, and bundled legal documents. | Reuses the page frame, text helpers and gpui-component buttons. |
| `database_error_page` | Full-window error: icon, summary, advice, technical error, paths, "Åbn logmappen" and "Prøv igen". | The model for any blocking error: say what happened, what to do, where the log is. |
| `MeasurementView` | The view behind the Måling page (inputs, model choices, result). | |
| `ProposalReview` | The view behind `proposals`: pending proposals for one meeting with their diffs and buttons. | |
| `card` | One proposal in `ProposalReview`: title, diff and the buttons for its state. | Only inside `ProposalReview`. |

### From gpui-component (use as they are)

`Textarea`, `Sidebar`/`SidebarMenu`, `Spinner`, `Icon`, `Root`, and `Button`,
`Input` and `Switch` through `controls`. Wrap one only when the wrapper adds a
Tingskriver rule (a token, a Danish default); do not restyle one inline in a
page.

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
| Start | `startup`, `Shell`, `home_page`, `loading` | First frame within 500 ms; the database opens in the background. |
| Look at meetings | `sidebar`, `MeetingsView`, `meeting_page`, `loading` | The list is always in the sidebar; search by title and content; open a meeting (title, status, audio folder, MCP sharing), rename and delete in dialogs, restore interrupted audio from Home. Recording and import explain the pending ports. |
| See why a recording stopped | `home_page`, `interrupted_card` | After sleep or logoff, shows the interruption notice on resume or next launch until dismissed. Recording does not restart automatically. |
| Test Danish input | `dictation_page`, `probe` | Manual Windows test 2 and 3 in [gpui.md](gpui.md). |
| Compare transcription on own sample | `MeasurementView`, `Page` | See [measurement.md](measurement.md). |
| See app settings | `settings_page`, `setting_group` | Read-only. |
| Choose meeting and report languages | `settings_page`, `language_group` | Saved through core's language settings; reports default to Danish. |
| See app information and copy diagnostics | `about_page`, `about`, `page`, `paragraph`, `section_title` | Offline metadata and legal documents; diagnostics include only build information and catalogue-owned model names. Update status is a snapshot. |
| Database cannot open | `database_error_page` | Replaces sidebar and page. |
| Second launch, hidden start | `single_instance`, `args` | Shows the existing window, or dispatches CLI/URL commands while hidden; tray is the host's. |
| Dictate | pill (overlay), host | Not a GPUI screen. |

## Naming

- A component is named for what it is, not where it sits: `recovery_card`, not
  `home_restore_box`. Page-level methods are `<page>_page`.
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
