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
| `theme` | Tokens (`Palette`, `space`, `typography`, `layout`, `icon_size`, `shadow`, radii, hover alphas and `over`), mapping onto gpui-component's theme (`apply`), live light/dark (`sync_with_window`), and the Udseende choice that can pin light or dark (`choice`, `set_choice`, `mode_for_choice`). | All visual values. Not for page structure. |
| `controls` | Buttons, inputs, switches, the segmented control and the recording pill button with the Tauri sizes (see [Controls](#controls-in-controlsrs)). | Every button and text input. Not for page layout. |
| `ui` | Main window, `Shell` (sidebar plus the page in the window), shared state (`AppModel`) and the page building blocks below. | The shell and blocks shared by several pages. |
| `pages` | One file per page, each an `impl Shell` block: `settings` (`settings_page` with its tabs, `mcp_settings`, `language_section`), `settings_tabs` (Optagelser, Transskription, Opsummering, Beta, Diktering), `dictation`, `database_error`. | A page's own code, so each page port owns its file. |
| `meetings` | Virtual meeting list backed by the core list model, inline rename/delete confirmation, the interrupted recordings found on disk (`recovery()`) and navigation handoffs. | Only meeting-list presentation; data, status mapping and mutations live in core. |
| `measurement` | The Måling sub-page (`MeasurementView`, `measurement_page`): compare a user's sample against a reference text, per model; Tauri section style, one checkbox row per provider and model with the tariff inline, results as a card. | Only this page. |
| `about` | Offline build metadata, an update-status snapshot, allowlisted diagnostics and bundled legal documents. | The About dialog and copying diagnostics. Opening it never starts an update check. |
| `about_dialog` | "Om Tingskriver" as a dialog over the current screen (Tauri `Info.tsx`, `About.tsx`), the update dialog (`UpdateDialog.tsx`) and the "Opdatering klar" notification (`UpdateNotification.tsx`). | ⓘ, the logo, `--page om` and the tray open it; `Page::About` is not a page. Not for other dialogs. |
| `probe` | Describes what the keyboard produced (code points, uncomposed dead keys) for the test field on the Diktering page. | Manual input tests. |
| `assets` | Embedded fonts and icons (`Assets`). | Adding a font or an icon. |
| `startup` | Startup clock, first-frame budget log, database open with Danish error classification (`DbFailure`). | Timing and database start. Not for rendering. |
| `window_state` | Saves and restores window position and size (`SavedWindow`). | Window geometry only. |
| `console` | On Windows, attaches to the launching terminal for CLI output, then frees it once startup is decided and clears the console's standard handles. | Process-level behaviour. |
| `single_instance` | One app per user over a named pipe or socket; a second launch shows the first window or queues a core command; parsing and transport live in core. | Process-level behaviour. |
| `args` | Command line (`StartupArgs`): `--hidden`, `--page`. | Start options. |
| `crash` | Panic hook that turns graphics failures into a log entry and a native error dialog. | Never a place for UI. |
| `update` | Velopack installer hooks, background update check and download, a manual check (`check_now`) for About, applying an update at next start. | Update behaviour. Its UI is in `about_dialog`. |
| `recording` | The recording screen of Home (`HomeState`): status bar, transcript lines, the floating pill, saving overlays and the start error, copied from Tauri `RecordingControls.tsx`, `RecordingStatusBar.tsx`, `VirtualizedTranscriptView.tsx` and `StatusOverlays.tsx`. `HomeState::from_recorder` draws the state of `tingskriver_host::Recorder`, which reads core (snapshot at startup, events, time and levels while recording) and saves and transcribes the meeting after stop; `show_notice` turns its messages into notifications and opens the finished meeting. | Home only. Ask the recorder; never show a state core has not reported. |
| `recovery` | "Gendan afbrudte møder" (Tauri `TranscriptRecovery.tsx`): `RecoveryView` in a dialog, the card on Home that opens it, and opening it once per session by itself. Restores through `recover::restore` and transcribes in the background; "Slet" uses `recover::discard` after a confirmation. | Interrupted recordings only. Not while a recording runs or is saved. |
| `proposals` | Review of AI proposals for a meeting's notes and summary: the band "N ventende ændringer", a diff per proposal and Anvend / Afvis / Fortryd. Changes go only through `ProposalStore::apply/undo` on a click. Mounted in `MeetingView` as a card between the toolbar and the summary, hidden while loading and when empty. | AI proposals. Never write notes or summary directly. |
| `onboarding` | First start (Tauri `components/onboarding`): replaces sidebar and page until onboarding is done (status in `onboarding-status.json`, shared with Tauri). Step 1 welcome, step 2 fetches the transcription and summary models through `tingskriver_core::onboarding`, then sets up the database. | Only first start. Download, status and completion logic stay in core. |

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
| `mcp_settings` | AI-værktøjer (MCP) on Generelt: access level as a select (Tauri), consent text, setup to copy with the text shown below each button, status, data parts as switch rows, test, the server's tools and the call log. | MCP controls on the Indstillinger page. |
| `meeting_page` | Opens the meeting document for the meeting selected in the sidebar: creates a `MeetingView` when another meeting opens and tells it the content width (`pages/meeting.rs`). | The content area when a meeting is open. |
| `MeetingView` | The meeting document (Tauri `MeetingDocument`): a 736 px article centered in the content area, px 32, pt 40, pb 112, the whole page scrolling. Loads `tingskriver_core::meeting_document` on the runtime and reloads when the sidebar status of the meeting changes. Summary is read-only rendered Markdown; editing (BlockNote in Tauri) is a separate functional issue. | One open meeting. Never write notes or summary from here; proposals go through `ProposalReview`. |
| `header` | Title (36 px semibold Literata) with a 32 px pencil that opens the "Omdøb møde" dialog, the date line "29. okt. 2026 kl. 09.30" (14 px muted), `sharing` and the `toolbar`. | Only in `MeetingView`. |
| `sharing` | "Må deles med AI-værktøjer" checkbox (Tauri `MeetingSharing`), saving through `mcp::set_shared`; "Opdatér mødelisten" when the meeting is not yet in the sharing list; errors as alerts. Replaces the old `meeting_sharing` block. | Per-meeting MCP sharing. MCP access itself is in Indstillinger. |
| `toolbar` | Two joined outline button groups (32 px segments, 12 px text, 16 px icons): [Lav om / Lav opsummering, Automatisk ▾, AI-model, Skabelon] and [Gem, Kopiér]. Labels collapse to icons below 544 px (language below 640 px). Only Kopiér works; the rest are disabled with the note "Opsummeringer laves og ændres endnu i den nuværende app." | Summary actions. Enable a segment when its function is ported. |
| `summary` | The summary area: "Skriver opsummeringen…" while running, the empty state (64 px FileQuestionMark, 18 px serif title, disabled "Lav opsummering"), or the blocks with the `bg-destructive/10` error box after a failed run. | Only in `MeetingView`. |
| `summary_block` | One rendered Markdown block in BlockNote's look: Literata headings 32/30/20 px, 16 px text on 28 px lines, 560 px text column, square bullets, numbers and task boxes in a 24 px column. | Read-only summary text. |
| `delivery` | "Aflever referatet til" with the note that delivery is not ported yet, below the summary. | Replaced by the real approval (target, consent) when delivery is ported. |
| `transcript_section` | The folded transcript: top border, 44 px disclosure row (chevron, "Transskript" 16 px semibold, "10 linjer", Skjul/Vis) with `aria-expanded`, Enter/Space, and a 70 % window-height card when open. | Only in `MeetingView`. |
| `transcript_toolbar` | Right-aligned button group [Kopiér, Optagelse (opens the audio folder), Forbedr (disabled)], or "Mødet har ingen lyd, kun et importeret transskript." without a folder. | Only in `transcript_section`. |
| `transcript_lines` | Virtualized transcript lines: 56 px muted time column (MM:SS) and 16 px text on 1.65 lines; empty state "Ingen transskription". | Only in `transcript_section`. |
| `home_page` | Home (`pages/home.rs`): the hero "Klar til dit næste møde", or the transcript of a recording, with the interruption card, recovery card and the import notice above it and the recording pill at the bottom. While recording cannot start here, the hint line under the lead says so instead of "Tryk på \"Start optagelse\"". | The default content. |
| `Shell` | The window content: sidebar plus the current page, or the database error page instead of both; on first start `Onboarding` replaces both until it is done. Owns page selection, sidebar collapse, the home notice, appearance observer and the debounced window-position save. | The one root view. Do not add a second root. |
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
| `language_section` | Sprog on the Transskription tab: small language buttons with a check mark on the chosen ones (primary and extra languages), a switch row for writing reports in the primary language, and the save error. | The meeting and report language settings (#209). Reuses gpui-component buttons. |
| `loading` | Spinner with a text. | Anything waiting for core (the sidebar list while the database opens). |
| `interrupted_card` | Dismissible alert (`pages/home.rs`) in the status-bar style with a warning border and icon: the reason, time and audio status of an interrupted recording, a reminder that recording does not restart automatically, and "Forstået". | The home screen after sleep or logoff interrupts a recording. Uses the persisted `InterruptNotice`. |
| `transcript_view` | The transcript column of Home (704 px): "Kopiér" (outline sm, copies `[MM:SS] tekst` lines), `status_bar` and `transcript_line`s, and "Lytter …" while lines stream in; without lines, `empty_transcript`. No transcript is made while recording (the audio is saved first), so a recording shows the empty state. Tauri's "Sprog" button (local Whisper only) is left out, since nothing transcribes during the recording. | Home while a recording runs or is being saved. |
| `status_bar` | Card above the transcript: state dot (`recording`, or `warning` when paused), "Optager"/"På pause", mm:ss, "Mikrofon: … · Systemlyd: …", and `device_notice`. Role status. | The running recording only. |
| `storage_notice` | Destructive notice inside `status_bar` while the audio is not being saved ("Lyden gemmes ikke: disken er fuld"), with an alert icon. Role alert. It stays until core reports the storage working again; a sticky notification says the same. | Storage errors during a recording. |
| `device_notice` | Notice inside `status_bar` when the microphone changed: information (primary tint) or warning (warning tint), with "Luk beskeden". | Device changes during a recording. |
| `empty_transcript` | Centered 12 px dot (primary, or warning when paused), 14 px "Optager mødet …" / "Optagelsen er sat på pause" and a 12 px hint, all muted. Tauri says "Lytter efter tale …" (`VirtualizedTranscriptView.tsx:262-276`), but no transcript is made while recording, so the hint says it is made after "Stop og gem". | A recording without transcript lines. |
| `transcript_line` | Timestamp (14 px muted) and 16 px text at line height 1.65; the newest line while recording sits on `secondary`; unsure words are underlined in the muted color with a tooltip (solid, since gpui has no dotted underline like Tauri's). | Transcript lines on Home. |
| `recording_pill` | The floating pill (card, border, `rounded-full`, `shadow::md`): "Start optagelse" ("Gør klar…" with a spinner while core starts), or Pause/Genoptag, "Stop og gem" and `level_bars`, or a spinner with "Gemmer optagelsen…"; the overlays "Gemmer mødet…" and "Gør transskriptionen færdig…" replace it. `start_error` hangs under it. The buttons call the recorder. | Home only. |
| `level_bars` | Three 4 px bars (microphone, both, system audio) whose height follows core's measured peaks on a −50 to 0 dB scale, falling over a few 200 ms ticks (Tauri draws random heights); 4 px `warning` dots at 60 % when paused. They never animate on their own, so reduced motion needs no special case. | The recording pill. |
| `start_error` | Destructive alert under the pill (448 px): title, the lines of the Tauri message and "Luk besked". Role alert. | Why a recording could not start. |
| `dot` | A round color dot (status bar 10 px, "Lytter …" 8 px). | Small state markers on the recording screen. |
| `MeetingsView` | The meeting list in the sidebar: GPUI virtual `list` over core's `meeting_list::List` (borrowed through row indices), day groups ("I dag", "I går", "mandag 26. oktober"), Tauri rows (title, time, short job status, search snippet), hover/focus actions Omdøb and Slet with the "Omdøb møde" and "Bekræft sletning" dialogs, full-text search through core (`search::search`, title filter at once), empty/loading/error/no-match states and the recovery card for Home. | Every list without content says what to do next in its own slot (Tauri text), not a card. |
| `meeting_row` | One row of the virtual list: a day heading (12 px medium muted) or a meeting (44 px min, `rounded-md`, `bg-accent` when open, accent/60 on hover) with its action chip. | Only in `MeetingsView`. |
| `list_message` | Muted 14 px text in the list slot, optionally with a title (empty list, no match, load errors). | Only in `MeetingsView`. |
| `search_field` | The sidebar search input: search icon, clear X. | Only in the sidebar. |
| `recovery_card` | Card on Home while interrupted recordings exist: "En optagelse blev afbrudt …", how many, and "Gendan afbrudte møder…", which opens `RecoveryView`. Hidden while a recording runs or is saved. | Shown on Home. |
| `RecoveryView` | "Gendan afbrudte møder" (896 px dialog, 80 % of the window high): the count, "Afbrudte møder" (a third, rows are buttons with name, "Sidst gemt …" and length; the chosen one has a primary border and tint), "Uddrag" (name, last saved, length, tracks, "Lyd findes"; instead of a transcript excerpt it says the audio is transcribed after restoring, since core's recovery has only raw audio), and Annullér, Slet (asks first, inline) and Gendan. | Only for interrupted recordings. |
| `recovery_list` | The left column of `RecoveryView`. | Only in `RecoveryView`. |
| `recovery_preview` | The right column of `RecoveryView`. | Only in `RecoveryView`. |
| `dictation_page` | Diktering (Tauri `DictationHistory.tsx`): 768 px column centred with 32 px padding; header, history and "Test tastatur". Reads history, settings and status from core's dictation service when shown and again on `dictation-history-changed`. | |
| `dictation_header` | "Diktering" (30 px serif) with the outline "Indstillinger" button (opens the Diktering tab), the shortcut explanation with the shortcut on a muted chip, and the status line (muted, warning or destructive). | Only on the Diktering page. The chip is a text highlight: GPUI text runs have no border or padding. |
| `dictation_history` | "Seneste dikteringer" (20 px serif) with "Ryd historik" (ghost sm; a second click confirms, red), the history-off line, three muted loading cards, the dashed empty card (keyboard icon) or the list. | Only on the Diktering page. |
| `dictation_entry` | One history card (`rounded-lg`, border, card, p 16): text, "dato · program · udbyder · varighed" (latency tooltip, "reserve brugt" in warning with the reason as tooltip), Kopiér (ghost sm) and Slet (32 px icon button). | Only in `dictation_history`. |
| `dictation_probe` | "Test tastatur" (GPUI only): a ghost disclosure under a top border that unfolds the `probe` fields. | Manual Windows tests 2 and 3 in [gpui.md](gpui.md). |
| `dictation_settings_tab` | Indstillinger, Diktering (`settings_tabs/dictation.rs`, Tauri `DictationSettings.tsx`): "Diktering" (24 px serif) and the intro, then the sections below. Reads settings, status, dictionary, snippets, microphones and hook scripts from core's dictation service each time the tab opens; every change saves through core at once (toast), a failed save reads the saved settings back. The red box when the service cannot be read; two muted bars while loading. | Only on the Diktering tab. Fixture: `settings-dictation-*`. |
| `section` | A Diktering-tab section: 20 px regular serif title (`subheading_regular`), 14 px muted description, controls 16 px apart, no rule between sections (`space-y-10`). | Only on the Diktering tab; Generelt uses `settings_section`. |
| `shortcut_section` | Genvej: "Diktering med genvej" switch, Tastekombination (input + outline "Gem genvej", off until changed), Genvejstilstand select, the hybrid threshold (saved on blur, 100-1000 ms) for Hybrid. | Diktering tab. |
| `microphone_section` | Mikrofon og lyd: microphone and channel selects side by side (a saved microphone that is not connected stays as "(ikke tilsluttet)"), "Hold mikrofonen åben", "Lyd ved start og stop" and the Lydstyrke slider (saved on release). | Diktering tab. |
| `transcription_section` | Transskription: engine select with the "ingen lokal Parakeet-model" warning; with Syv.ai, the key field (masked, emptied after saving, never shown), "Gem nøgle", "Test", "Fjern" and the fallback select. | Diktering tab. The key goes only to Windows Credential Manager. |
| `cleanup_section` | Renskrivning: "Prøv det" (input + "Vis", result in a muted box), "Saml gentagelser", "Polering med sprogmodel" with Udbyder, Model and the Ollama address. | Diktering tab. |
| `hook_section` | Tekst-hook: switch, script select (a missing script stays as "(findes ikke)") and the folder line. | Diktering tab. |
| `styles_section` | Stil pr. program: rows of program input, Rå/Normal select and trash icon; "Tilføj program" and "Gem programmer" (off until the rows change). | Diktering tab. |
| `words_section` | Ordbog: Ord and "Høres som" side by side with "Tilføj"/"Gem", then `list_box` rows "ord · høres som …" with Ret and Slet. | Diktering tab. |
| `snippets_section` | Snippets: Udløser, Tekst (3-row textarea), "Tilføj snippet"/"Gem snippet", then `list_box` rows with the text clamped to three lines. | Diktering tab. |
| `history_section` | Historik og privatliv: "Gem historik", "Gem lyden i 24 timer" (off while history is off) and the privacy note. | Diktering tab. |
| `toggle` | A `switch_row` that saves one dictation flag. | Diktering tab. |
| `small_labelled` | A 14 px medium label above its control (`text-sm font-medium`). | Paired fields on the Diktering tab. |
| `list_box` | A bordered, rounded list with dividers (`divide-y rounded-lg border`). | Ordbog and Snippets. |
| `list_row` | A `list_box` row: content left, ghost Ret (pencil) and Slet (trash) icon buttons right, px 16 py 8. | Ordbog and Snippets. |
| `settings_page` | Indstillinger as in Tauri (`app/settings/page.tsx`): fixed header (`sub_page_header`) with "Tilbage" (to the page it was opened from, with the meeting that was open; Home if unknown) and the 30 px title, underline tabs, the active tab's panel in a scroll area (`SettingsView` holds the tab, the tab focus handles and the MCP select). `--page indstillinger` opens Generelt. | Settings. Each tab's content is its own method; Optagelser, Transskription, Opsummering and Beta (Job 6) and Diktering (Job 7) are in `pages/settings_tabs`. |
| `tab_bar` | The settings tabs: 16 px icon and 14 px medium label, px 24 py 16, active in primary with a 2 px primary bar, others muted (foreground on hover), bottom border on the list; arrows, Home and End move between tabs. | Only on `settings_page`. The bar jumps instead of sliding (no layout animation in GPUI). |
| `general_tab` | Generelt: Udseende (segmented, saved through core `appearance`), Notifikationer (disabled until recording is ported), Placering af data (`storage`), Aflevering af referat (`delivery_settings`), AI-værktøjer (`mcp_settings`), "Afslut Tingskriver" last. | |
| `storage` | Placering af data: recordings folder with "Åbn mappen", Logfiler with "Åbn logmappe", "Gem nedbrudsfiler" on Windows, the data folder and database state. | Only on Generelt. |
| `tab_column` | A tab's 672 px column (`max-w-2xl`), sections 40 px apart. | Every settings tab. |
| `settings_section` | A Tauri settings section: title, 14 px muted description, controls; a top border and 32 px above all but the first. | Every block on a settings tab. |
| `subheading` | 20 px semibold Literata title (Aflevering af referat, AI-værktøjer). | Settings sections whose Tauri title is `text-xl font-semibold`. |
| `field_label` | A control's name in medium weight (`font-medium text-foreground`). | Above or beside a settings control. |
| `switch_row` | Label and hint on the left, `switch` on the right (`justify-between`). | Boolean settings. |
| `alert` | An error line in the destructive color with the alert role. | Failed saves on the settings page. |
| `recording_settings_tab` | Optagelser (`pages/settings_tabs/recording.rs`, Tauri `RecordingSettings.tsx`): "Gemmes i" with core's recordings folder and "Åbn mappen", the always-saved note, the start reminder switch and "Standardenheder til lyd" (Mikrofon, Systemlyd). The reminder (shown on, Tauri's default) and devices are disabled with the reason until recording is ported (#82). | Job 6. |
| `transcription_settings_tab` | Transskription (`settings_tabs/transcription.rs`, Tauri `TranscriptSettings.tsx`): the engine choice and model cards (`engine_body`), then `language_section`, then the Måling entry "Sammenlign modeller på din egen stemme". | Job 6, #174. The saved engine and model are in `transcript_settings`, as Tauri saves them. |
| `engine_body` | The engine as a segmented control (Parakeet, Whisper; Tauri has a select for the same two values) over the model list of that engine. | Transskription. Syv.ai is a dictation engine and stays on the Diktering tab. |
| `model_list` | The cards of one engine, a loading line, the "ingen model" hint, a notice that belongs to no card, and "Bruger … til transskription". The list only reads the models folder; nothing is fetched by drawing it. | `models::State`. |
| `model_card` | One model: name, "Valgt" and "Anbefalet" badges, tagline, size, source, licence and revision (unknown shown as "ukendt"), the checksum line (verified only after a checked download or "Kontrollér"), and the message line (failure, corruption, why delete is blocked). Selected = `border-2 border-primary bg-primary/10`. | Tauri `ModelCard`. |
| `card_actions` | "Klar" with "Vælg", "Kontrollér" and "Slet" (blocked, with the reason, while recording, dictating or transcribing); "Hent"; "Prøv igen"; "Slet" with "Hent igen". "Slet" opens a confirmation. | Tauri buttons plus "Vælg" and "Kontrollér" for the keyboard. |
| `progress_block` | The bar of a running download with percent, MB done and total, and "Annullér" (then "Annullerer…" and "Annullering igangsat"). A progress-indicator role names the model. | Onboarding bar style. |
| `alert_line` | A small destructive-colored line under a card, announced as an alert. | `model_card`. |
| `models` | `src/models.rs`: `State` (rows, saved choice, every transition, tested without network), `Models` (the GPUI entity and its core calls: list, download, cancel, delete, save, verify) and the download events. | Reuses core's engines; disk space is checked before a download. |
| `summary_settings_tab` | Opsummering (`settings_tabs/summary.rs`, Tauri `SummaryModelSettings.tsx`): three cards, Automatisk opsummering (disabled switch, shown on as Tauri's default; runs when recording stops, #82), Opsummeringssprog (quick choice follows #209) and Model til opsummering (read-only provider and model selects with the saved values; no open issue yet for choosing the model in this app), each with the reason. | Job 6. |
| `beta_settings_tab` | Beta (`settings_tabs/beta.rs`, Tauri `BetaSettings.tsx`): warning banner, the import feature card (switch disabled and shown on as Tauri's default, import is not ported, #89), the Stemmeaftryk card (GPUI, #198) and the primary info box. | Job 6. |
| `feature_title` | A Beta card's title row: flask icon, 18 px semibold serif title and the warning "Beta" badge. | Beta cards only. |
| `settings_card` | A Tauri settings card: `bg-card rounded-lg border p-6 shadow-sm`, group role. | Opsummering and Beta. |
| `card_title` | 18 px semibold serif title of a settings card (`h3 text-lg font-semibold`) with an optional muted 20 px icon. | Opsummering and Beta cards. |
| `subheading_regular` | 20 px regular serif sub-section title (`h3 text-xl`). | "Standardenheder til lyd" and the Diktering-tab `section` titles. |
| `disabled_select` | A select trigger this app cannot change yet: 36 px, `input` border, half opacity, chevron; combobox role with value and the reason as description; not focusable. | Instead of a select that would do nothing (Optagelser, Transskription, Opsummering). |
| `tinted_box` | A rounded box tinted 10 % with a 30 % border in a status color, optional icon (`bg-warning/10`, `bg-primary/10`). | The Beta banner and info box. |
| `sub_page_header` | The settings header (bottom border, px 32 py 24, muted "Tilbage" with ArrowLeft and the 30 px title). | `settings_page` and its sub-page `measurement_page`. |
| `measurement_page` | Måling as a sub-page of Indstillinger > Transskription: `sub_page_header` ("Tilbage" to the Transskription tab) and `MeasurementView` in the settings column. | Opened from the Transskription tab or `--page måling`; the sidebar marks Indstillinger. |
| `delivery_settings` | Aflevering af referat as Tauri's `DeliverySettings.tsx`: Privatliv checkboxes, "Dine mål" with "Test mål" and "Fjern mål" (confirm dialog), the "Tilføj mål" form (Mappe, Webhook with masked address, SSH med SFTP) and "Afleveringskø" with "Prøv igen". Uses the host's queue (`Host::delivery`); without it, the "kunne ikke læses" line and "Opdatér". | Settings for sending approved meeting documents. |
| `delivery_privacy` | "Privatliv": the "Kun lokal" and "Privat tilstand" checkboxes, 44 px rows. | Part of `delivery_settings`. |
| `delivery_targets` | "Dine mål": one row per target with "Test mål" (off while blocked) and "Fjern mål". | Part of `delivery_settings`. |
| `delivery_form` | "Tilføj mål": name, type select and the fields of the chosen type, 44 px controls, "Gem mål". | Part of `delivery_settings`. |
| `delivery_queue` | "Afleveringskø": job line, error, next attempt and "Prøv igen". | Part of `delivery_settings`. |
| `group_title` | 18 px semibold title inside a settings block (`text-lg font-semibold`). | "Dine mål", "Tilføj mål", "Afleveringskø". |
| `labelled` | A label above its control (`label > span + input`). | Form fields in `delivery_form`. |
| `AboutView` | The About dialog's content: the Tauri layout (`hero`, `features`, `footer`) and, collapsed below it, the build details and the legal documents. Scrolls beyond 80 % of the window height. | Only inside the dialog `about_dialog::open_about` opens. |
| `hero` | Logo mark (56 px), "Tingskriver" 20 px semibold serif, version, tagline, the outline 44 px button "Søg efter opdateringer" and "Opdatering klar: version x". | Only in `AboutView`. |
| `features` | "Det adskiller Tingskriver": 16 px semibold serif title and four term/description pairs (14 px medium / muted). | Only in `AboutView`. |
| `footer` | Top border, 12 px muted centered note with an underlined NOTICE.md and the links to the website and GitHub. | Only in `AboutView`. |
| `footer_link` | A focusable, underlined 12 px muted link that opens a URL. | Footer links in a dialog. Not for buttons. |
| `disclosure` | A 44 px row with a chevron and a medium title that shows or hides a section (`aria-expanded`). | GPUI-only extras inside the About dialog. |
| `build_details` | Build metadata, update status and "Kopiér diagnoseoplysninger" with its status line. | Only in `AboutView`. |
| `legal` | "Privatlivspolitik", "Kontakt" and the four bundled documents in 200 px scroll boxes (arrow keys, Page Up/Down, Home/End). | Only in `AboutView`. |
| `database_error_page` | Full-window error: icon, summary, advice, technical error, paths, "Åbn logmappen" and "Prøv igen". | The model for any blocking error: say what happened, what to do, where the log is. |
| `MeasurementView` | The view behind the Måling page (inputs, model choices, result). | |
| `ProposalReview` | The view behind `proposals`: pending proposals for one meeting with their diffs and buttons. | |
| `card` | One proposal in `ProposalReview`: title, diff and the buttons for its state. | Only inside `ProposalReview`. |
| `Onboarding` | The view behind `onboarding`: step, the two download cards' state (core's `Download`, moved by host events), "Kom i gang" and "Færdig". | Only first start. Shown by `Shell::render` via `onboarding::shown`. |
| `progress` | "Trin 1 af 2" (14 px muted) and one 40×6 px segment per step, primary up to the current step. | Only in `Onboarding`. |
| `welcome` | Step 1: the note and the full-width (max 320 px) primary "Kom i gang". | Only in `Onboarding`. |
| `downloads` | Step 2: the two cards (max 512 px), the note while the summary model continues, and "Henter model…" / "Gør klar…" / "Færdig" (loading button dimmed to 60 % while not ready). | Only in `Onboarding`. |
| `download_card` | One model card (`rounded-xl`, p 20): 40 px icon circle, Literata title, size, status (Venter… / spinner / check / Mislykkedes / Annulleret), progress bar with MB and percent, and the error box with "Prøv igen". | Only in `Onboarding`. |
| `status_text` | The 14 px status word at the right of a card. | Only in `download_card`. |

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
| Meeting document | mødedokument | One meeting's page: title and time, MCP sharing, summary (read-only for now), pending AI proposals and the folded transcript. Notes and delivery approval are not ported yet. | `MeetingView` (`pages/meeting.rs`); data in `tingskriver_core::meeting_document`, `proposals` |
| Queue | kø | Persistent job queue for saved audio that is transcribed after the recording, with retries and back-off. A delivery queue with the same shape exists for delivery. Neither runs during recording. | [transcription-queue.md](transcription-queue.md), `transcription_queue`; [delivery.md](delivery.md) |
| Delivery | aflevering | Sending an approved meeting document to a target (folder, and others as described in delivery.md). Opt-in, blocked by "Kun lokal" and "Privat tilstand". Never sends on save. | [delivery.md](delivery.md), `tingskriver_core::delivery` |
| Microphone track / system track | mikrofonspor / systemspor | The user's own voice / what the PC plays. Always called "microphone" and "system" in code. | [SPEC.md](SPEC.md) |

## Experiences

What a user can do in the GPUI app today and which components make it up.
Change a row when the experience changes.

| Experience | Components | Notes |
|---|---|---|
| Start | `startup`, `Shell`, `home_page`, `recording_pill`, `loading` | First frame within 500 ms; the database opens in the background. |
| First start | `Onboarding`, `progress`, `welcome`, `downloads`, `download_card` | Replaces sidebar and page until done; the transcription model must be on disk before "Færdig", the summary model may continue in the background. No macOS permissions step (Tauri has one on macOS only). |
| Look at meetings | `sidebar`, `MeetingsView`, `meeting_page`, `MeetingView`, `loading` | The list is always in the sidebar; search by title and content; open a meeting as a document (summary, transcript, audio folder, MCP sharing, copy), rename and delete in dialogs, restore interrupted audio from Home. Recording and import explain the pending ports. |
| Record a meeting | `home_page`, `recording_pill`, `transcript_view`, `status_bar`, `start_error` | The screen is ported (Tauri `/`), but starting a recording is not wired to core yet: "Start optagelse" is disabled and the hint says to use the current app. "Ny optagelse" opens Home. |
| See why a recording stopped | `home_page`, `interrupted_card` | After sleep or logoff, shows the interruption notice on resume or next launch until dismissed. Recording does not restart automatically. |
| Test Danish input | `dictation_probe`, `probe` | Manual Windows test 2 and 3 in [gpui.md](gpui.md). |
| Compare transcription on own sample | `MeasurementView`, `Page` | See [measurement.md](measurement.md). |
| Change app settings | `settings_page`, `tab_bar`, `general_tab`, `storage`, `delivery_settings`, `mcp_settings` | Udseende (system, light, dark) applies at once and is saved; folders open in the file manager; crash dumps on Windows; MCP access, data parts, test and log. Delivery targets, privacy and queue as in Tauri. Notifications say they are not ported yet. Optagelser, Opsummering and Beta show Tauri's sections; what core cannot do from this app yet is disabled with the reason, and the saved summary model is shown read-only. Transskription chooses the engine and handles the models (#174). |
| Choose meeting and report languages | `settings_page`, `language_section` | Saved through core's language settings; reports default to Danish. |
| See app information and copy diagnostics | `about_dialog`, `AboutView`, `disclosure`, `build_details`, `about` | A dialog over the current screen. Offline metadata and legal documents; diagnostics include only build information and catalogue-owned model names. Update status is a snapshot until "Søg efter opdateringer". |
| Update the app | `about_dialog`, `update` | A found update shows the update dialog (current and new version, installs at the next start); other results are notifications. The background updater shows "Opdatering klar" with "Se detaljer". |
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
