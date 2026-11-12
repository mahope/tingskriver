# Parity map: Tauri desktop UI -> GPUI app

Source: `origin/main` at `7f8e1c70` (Release 0.5.3-dev.1). Tauri = `frontend/src`, GPUI = `crates/tingskriver-app/src`.
Tauri reference images: `docs/screenshots/*.png` (1280x800, 1x). GPUI goldens: `crates/tingskriver-app/tests/golden/linux/*.png` (1100x760 logical, rendered at 2x).

## 0. Read this first: which "old" app?

The owner asks for "a 1:1 copy of the old one". There are two candidate "old" looks, and the current premise ("GPUI has its own style that must be replaced by the Tauri look") is only half true:

| | Tauri `>= v0.5.0-dev.2` (shipped look, what users run today) | Tauri `<= v0.5.0-dev.1` (pre-rebrand, Meetily-derived) |
|---|---|---|
| Introduced | `c6fac4a2` / `9943690e` (2026-10-29, "add design tokens, self-hosted fonts", "redesign sidebar, recording page and meeting document") | before 2026-10-29 |
| Palette | warm paper + ink blue (light), cool slate (dark) | shadcn "neutral": white/near-black, no accent hue |
| Fonts | Hanken Grotesk (UI) + Literata (headings) | Source Sans 3 only |
| Same as GPUI today? | **Palette and fonts: yes, value for value.** `theme.rs:1` says so ("Design tokens from the 0.5.0-dev.2 design"). | No |

So the warm paper / Literata / ink-blue style **is** the Tauri app's style. GPUI copied the tokens on 2026-10-30 (`e91dd80b`) but not the layout, type scale, control sizes, shadows or screen structure. That is where the visible difference comes from (compare `docs/screenshots/moedeliste-lys.png` with `golden/linux/meetings-light.png`).

**Recommendation:** target the shipped Tauri look (left column). Every job below assumes this. If the owner instead means the pre-rebrand look, only Job 1's palette and font rows change (values in section 1.6); all layout jobs stay the same. Get that answer before Job 1 merges.

---

## 1. Tokens

### 1.1 Colors (identical in Tauri and GPUI today)

Tauri: `frontend/src/app/globals.css:82-110` (light), `:112-139` (dark); Tailwind mapping `frontend/tailwind.config.js:19-63`. GPUI: `theme.rs:47-101`.

| Token | Light HSL | Light hex | Dark HSL | Dark hex |
|---|---|---|---|---|
| background | 40 14% 96% | #F6F5F3 | 222 16% 9% | #13151B |
| foreground | 222 24% 13% | #191E29 | 40 12% 91% | #EBE9E5 |
| card / popover | 40 25% 99% | #FDFDFC | 222 15% 12% / 13% | #1A1D23 / #1C1F26 |
| card-/popover-foreground | 222 24% 13% | #191E29 | 40 12% 91% | #EBE9E5 |
| primary | 216 58% 30% | #204479 | 214 75% 72% | #82B0ED |
| primary-foreground | 40 25% 99% | #FDFDFC | 222 30% 9% | #10141E |
| secondary = muted | 40 10% 92% | #EDEBE9 | 222 13% 16% | #23272E |
| muted-foreground (also `tertiary`) | 222 9% 36% | #545864 | 222 8% 67% | #A4A8B2 |
| accent | 40 12% 89% | #E6E4E0 | 222 13% 20% | #2C303A |
| destructive | 0 62% 40% | #A52727 | 4 75% 62% | #E75F55 |
| success | 150 50% 24% | #1F5C3D | 150 45% 56% | #5CC18F |
| warning | 32 85% 26% | #7B460A | 38 85% 62% | #F0B44C |
| recording | 4 68% 42% | #B42C22 | 4 75% 62% | #E75F55 |
| border | 40 9% 85% | #DCDAD5 | 222 12% 22% | #31353F |
| input | 40 6% 62% | #A4A098 | 222 10% 40% | #5C6270 |
| ring | 216 70% 45% | #2263C3 | 214 80% 70% | #75AAF0 |
| scrollbar | 40 6% 72% | #BCB9B3 | 222 10% 28% | #40454F |
| *-foreground on destructive/success/warning | = primary-foreground | | = primary-foreground | |

`--radius: 0.5rem` (`globals.css:108`). Selection: `primary / 0.22` (`globals.css:237-239`). Focus: `outline 2px ring, offset 2px` (`globals.css:232-235`).

**theme.rs palette changes: none** (unless the pre-rebrand look is chosen, 1.6). Small derived-color differences to fix:

| GPUI today | Tauri | Change in `theme.rs` |
|---|---|---|
| `c.selection = p.primary.opacity(0.25)` (`theme.rs:221`) | `primary / 0.22` (`globals.css:238`) | `opacity(0.22)` |
| Hover/pressed = lightness +/-5 % (`emphasize`, `theme.rs:172-180`) | `bg-primary/90`, `bg-destructive/90`, `bg-secondary/80` (`ui/button.tsx:13-25`); list hover `bg-accent/60`, active `bg-accent` (`Sidebar/index.tsx:483-485`) | Replace `emphasize` with `over(color, background, alpha)` (alpha-blend onto the surface): primary/danger/success hover 0.90, secondary hover 0.80, list hover = accent at 0.60. Keep a pressed step (0.80) since Tauri has none. Rewrite `hover_states_stay_readable` (`theme.rs:340-351`) and `emphasize_moves_away_from_the_surface` for the new function; the AA test `text_meets_wcag_aa_in_both_modes` (`theme.rs:327-338`) must stay green unchanged. |
| `theme.shadow = false` (`theme.rs:212`) | buttons and the recording pill have shadows (1.4) | `theme.shadow = true`, plus explicit shadow tokens |
| `c.list_active_border = p.ring` | Active meeting row: `bg-accent`, no border | `list_active_border = p.accent` (or transparent) |

### 1.2 Fonts

| | Tauri | GPUI |
|---|---|---|
| UI | Hanken Grotesk, `frontend/src/fonts/HankenGrotesk.woff2`, variable `--font-hanken` (`app/layout.tsx:32-38`, `tailwind.config.js:12-14`) | Hanken Grotesk Regular/Medium/SemiBold/Bold TTF, `assets.rs:14-27` |
| Headings (`h1, h2, h3` all serif, `letter-spacing: -0.01em`, `globals.css:226-229`) | Literata 400-700 + italic (`layout.tsx:40-45`) | Literata Regular/SemiBold/Italic TTF (`assets.rs` + `assets/fonts/`) |
| Licence | SIL OFL 1.1 (comment `layout.tsx:32`) | `crates/tingskriver-app/assets/fonts/OFL.txt` already present |

No new font must be embedded for the shipped look. (Pre-rebrand would need Source Sans 3, also OFL 1.1; add TTFs + OFL notice.) Literata is used at weight 400 for most headings, so make sure `Literata-Regular.ttf` is the one picked when `FontWeight::NORMAL` is set on serif text.

### 1.3 Type scale

Tauri body: `font-size: 15px; line-height: 1.55` (`globals.css:148-156`). Everything else uses Tailwind steps (size/line-height): `text-xs` 12/16, `text-sm` 14/20, `text-base` 16/24, `text-lg` 18/28, `text-xl` 20/28, `text-2xl` 24/32, `text-3xl` 30/36, `text-4xl` 36/40. Note `text-base` is 16 px, not the 15 px body.

| Tauri use | Size / weight / family | Source |
|---|---|---|
| Sidebar wordmark | 20 semibold serif, tracking-tight | `components/Logo.tsx:45` |
| Sidebar group label (I dag, I går) | 12 medium sans, muted | `Sidebar/index.tsx:701` |
| Sidebar meeting title / time | 14 medium (active: semibold) / 12 muted tabular | `Sidebar/index.tsx:487-489` |
| Sidebar nav items, buttons | 14 (`text-sm`), "Ny optagelse" semibold | `Sidebar/index.tsx:642, 713-742` |
| Home empty title | 30 semibold serif | `app/_components/TranscriptPanel.tsx:72` |
| Meeting title | 36 semibold serif, leading-tight | `MeetingDetails/MeetingHeader.tsx:80` |
| Settings / Diktering page title | 30 **regular** serif | `app/settings/page.tsx:89`, `dictation/DictationHistory.tsx:122` |
| Settings section title (Udseende, Notifikationer ...) | 24 regular serif | `PreferenceSettings.tsx:144,153,177` |
| Sub-section titles | 18 semibold serif (h3) or 20 regular (h3 `text-xl`) | `SummaryModelSettings.tsx:130`, `RecordingSettings.tsx:196` |
| Diktering "Seneste dikteringer" | 20 regular serif | `DictationHistory.tsx:138` |
| Onboarding title | 30 semibold serif | `onboarding/OnboardingContainer.tsx:25` |
| Database error title | 24 regular serif | `DatabaseGate.tsx:103` |
| Summary headings (BlockNote) | Literata | `globals.css:279-281` |
| Help text | 14 muted (`text-sm text-muted-foreground`) | throughout |

**Change `theme.rs:133-139` (`typography`) to:**

| Const | Now | New | Tailwind |
|---|---|---|---|
| `XS` | 12 | 12 | text-xs |
| `SM` | 13 | **14** | text-sm |
| `BASE` | 15 | 15 | body |
| `MD` (new) | - | **16** | text-base |
| `LG` | 17 | **18** | text-lg |
| `XL` | 20 | 20 | text-xl |
| `XXL` | 26 | **24** | text-2xl |
| `XXXL` (new) | - | **30** | text-3xl |
| `DISPLAY` (new) | - | **36** | text-4xl |
| `LINE_HEIGHT` | 1.55 | 1.55 (body); add `LINE_HEIGHT_TIGHT` 1.25 for headings | |

`heading()` (`ui.rs:1669-1675`) becomes `heading(text, Level)` with: `Page` = 30 regular serif, `Document` = 36 semibold serif, `Hero` = 30 semibold serif, `Section` = 24 regular serif. `section_title()` (`ui.rs:1678-1683`, 17 semibold sans) becomes 18 semibold **serif** (Tauri h3). Apply `-0.01em` letter spacing if GPUI exposes it; otherwise note it in design.md as a known deviation.

### 1.4 Spacing, radii, sizes, shadows

| Area | Tauri value | Source | GPUI now | Change |
|---|---|---|---|---|
| Spacing | Tailwind 4 px grid, also uses 2, 6, 10, 20, 40 px (`gap-0.5`, `py-1.5`, `mt-1.5`, `px-5`, `pt-10`) | many | `space::XS..XXL` = 4, 8, 12, 16, 24, 32 (`theme.rs:113-122`) | add `XXS` 2, `XS2` 6 (name freely), `XL2` 20, `XXXL` 40 |
| Radius sm / md / lg | 4 / 6 / 8 (`tailwind.config.js:64-68`) | | `RADIUS` 6, `RADIUS_LG` 8 (`theme.rs:167-168`) | add `RADIUS_SM` 4, `RADIUS_FULL` (pill, switch, avatar dots) |
| Button default | h 44 (`h-11 min-h-[44px]`), px 16, text 14 medium, radius 6, icon 16, gap 8 | `ui/button.tsx:8,28` | gpui-component Medium (~32-36) | add `layout::CONTROL_HEIGHT` 44, wrap `Button` in a Tingskriver helper that sets it |
| Button sm | h 32, px 12, text 12 | `ui/button.tsx:29` | `Size::Small` 30 | `CONTROL_HEIGHT_SM` 32 |
| Icon button | 36x36 (`size icon`) / 32x32 sidebar row actions | `button.tsx:31`, `Sidebar/index.tsx:471` | - | `ICON_BUTTON` 36, `ICON_BUTTON_SM` 32 |
| Input | h 36, px 12, radius 6, border `input`, text 14 | `ui/input.tsx:11` | gpui-component default | `INPUT_HEIGHT` 36 |
| Switch | 36x20 track, 16 px thumb, checked `primary`, unchecked `muted` | `ui/switch.tsx:14-22` | gpui-component Switch | map colors in `apply` |
| Sidebar | 256 px (`w-64`), collapsed 64 px (`w-16`), bg `card`, right border | `Sidebar/index.tsx:620`, `MainContent/index.tsx:15-17` | 256 px, not collapsible (`ui.rs:684-685`) | add `SIDEBAR_COLLAPSED_WIDTH` 64 |
| Sidebar collapse handle | 24 px circle at right edge, top 20 px, card bg, border | `Sidebar/index.tsx:627` | - | new |
| Content widths | home transcript 704 (`max-w-[44rem]`); meeting document 736 (`max-w-[46rem]`, px 32, pt 40, pb 112); settings frame 1152 (`max-w-6xl`, p 32) with sections 672 (`max-w-2xl`); Diktering 768 (`max-w-3xl`, px 32 py 32); onboarding text 448 (`max-w-md`); database error 576 (`max-w-xl`) | `TranscriptPanel.tsx:83`, `MeetingDocument.tsx:35`, `settings/page.tsx:79,96`, `PreferenceSettings.tsx:141`, `DictationHistory.tsx:119`, `OnboardingContainer.tsx:27`, `DatabaseGate.tsx:102` | one `CONTENT_MAX_WIDTH` 760, `NOTICE_MAX_WIDTH` 600 (`theme.rs:146-156`) | replace with per-page widths: `TRANSCRIPT_MAX` 704, `DOCUMENT_MAX` 736, `SETTINGS_MAX` 1152, `SETTINGS_SECTION_MAX` 672, `DICTATION_MAX` 768, `ONBOARDING_TEXT_MAX` 448, `NOTICE_MAX` 576 |
| Icons | lucide, 16 px in rows/buttons, 20 px in collapsed sidebar | `button.tsx:8`, `Sidebar/index.tsx:536-605` | 28/32 px special sizes (`theme.rs:159-164`) | add `icon_size::SM` 16, `MD` 20 |
| Window | 1100x700 default | `src-tauri/tauri.conf.json:17-18` | 1100x720 (`theme.rs:148-149`) | 700 for 1:1 (optional) |
| Shadow `shadow-sm` | 0 1px 2px 0 rgb(0 0 0 / .05) | outline/secondary buttons | none | `shadow::SM` |
| Shadow `shadow` | 0 1px 3px 0 rgb(0 0 0 / .1), 0 1px 2px -1px rgb(0 0 0 / .1) | default (primary) button | none | `shadow::BASE` |
| Shadow `shadow-md` | 0 4px 6px -1px rgb(0 0 0 / .1), 0 2px 4px -2px rgb(0 0 0 / .1) | recording pill, status toasts (`RecordingControls.tsx:336`, `StatusOverlays.tsx:29`) | none | `shadow::MD` |

### 1.5 Button variants (`frontend/src/components/ui/button.tsx:12-25`)

| Variant | Look | GPUI mapping |
|---|---|---|
| default / blue | `bg-primary`, primary-foreground, `shadow`, hover primary/90 | `Button::primary()` + shadow |
| destructive / red | `bg-destructive`, hover /90 | `Button::danger()` |
| outline | 1 px `input` border, `bg-background`, `shadow-sm`, hover `bg-accent` | `Button::outline()`; set border to `input`, not `border` |
| secondary / gray | `bg-secondary` (gray also has `input` border) | `Button::secondary()` |
| ghost | transparent, hover `bg-accent` | `Button::ghost()` |
| link | primary text, underline on hover | `Button::link()` |
| green | `bg-success` | `Button::success()` |
| Button group (meeting toolbars) | joined outline buttons, radius 6 outer only, size sm, labels hide below container widths 34rem / 22rem | `SummaryGeneratorButtonGroup.tsx`, `TranscriptButtonGroup.tsx` | gpui-component `ButtonGroup` |
| Segmented control (Udseende) | `rounded-lg border-input p-0.5 bg-card`, items 36 px high, active = primary fill | `ThemeSelector.tsx:16-30` | `ButtonGroup` with selected = primary |
| Recording pill buttons | `rounded-full`, h 44, px 20, 14 semibold; Start = primary, Pause = outline on card, "Stop og gem" = `bg-foreground text-background` | `RecordingControls.tsx:331-390` | new `pill_button` helper |

**GPUI layout bug to fix with the tokens:** in every golden, buttons stretch to the full content width ("Om", "Kopiér diagnoseoplysninger", "Genindlæs udbyderindstillinger", "Kopiér opsætning ..."). Tauri buttons are content-width (`inline-flex`). Fix centrally (wrap button rows in `h_flex()` / no `w_full()` on buttons), not per page.

### 1.6 Pre-rebrand alternative (only if the owner picks it)

Tauri `v0.5.0-dev.1:frontend/src/app/globals.css:81-127`, font Source Sans 3 (`layout.tsx:4,33`), no dark mode provider at that tag. Light: background #FFFFFF, foreground #0A0A0A, primary #171717, primary-foreground #FAFAFA, secondary/muted/accent #F5F5F5, muted-foreground #737373, destructive #EF4444, border/input #E5E5E5, ring #0A0A0A. Dark: background/card #0A0A0A, foreground #FAFAFA, primary #FAFAFA, primary-foreground #171717, secondary/muted/accent/border/input #262626, muted-foreground #A3A3A3, destructive #7F1D1D, ring #D4D4D4. No success/warning/recording tokens existed; keep the current ones. Check: light destructive #EF4444 on #FAFAFA is about 3.8:1 and fails the AA test, so it would need adjusting.

---

## 2. Screens

Legend: **Port** = exists in Tauri, missing in GPUI. **Restyle** = exists in both, looks different. **New** = GPUI only, layout may be adapted.

### 2.1 Shell and sidebar (Restyle, structural)

Tauri (`components/Sidebar/index.tsx:617-760`, `app/layout.tsx:273-279`; image `moedeliste-lys.png`): fixed 256 px column, `bg-card`, right border.
1. Header (px 16, pt 20): logo mark SVG (24 px, primary, `public/tingskriver-mark.svg`) + "Tingskriver" 20 semibold serif (`Logo.tsx:38-45`). Collapse handle (chevron) on the right edge.
2. "Ny optagelse" full-width button, 44 px, radius 8 (`rounded-lg`), mic icon, primary. While recording: `bg-recording/10`, recording text, pulsing 10 px dot, "Optagelse i gang" (`:642-656`).
3. Search input group (36 px, search icon, clear X), placeholder "Søg i møder" (`:661-680`), "Søger…" status.
4. Scrollable meeting list (px 8): date groups ("I dag", "I går", weekday + date, "Tidligere") with 12 px medium muted labels; rows 44 px min, radius 6, title 14 medium (2-line clamp) + time 12 muted; active row `bg-accent` + semibold; hover shows rename (pencil) and delete (trash) 32 px icon buttons on an accent chip (`:471-517`). Empty: "Ingen møder endnu" + "Tryk på \"Ny optagelse\", så dukker dit første møde op her." (`:693-694`).
5. Footer (top border, p 8): rows 44 px with 16 px icons: "Diktering" (AudioLines, shown when enabled), "Importér transskript" (FileText), "Importér lyd" (Upload), then "Indstillinger" (Settings) with an ⓘ icon button at its right that opens the About dialog (`Info.tsx`). Active row = `bg-accent`.
6. Collapsed (64 px): icon-only column with tooltips: record (round 44 px), Møder, import transcript, import audio, Diktering, Indstillinger (`:527-608`).
7. Dialogs: rename ("Omdøb møde", `:764-805`), delete confirmation (`ConfirmationModel`, text at `:127` of the footer block).

GPUI (`ui.rs:681-726`, `golden/linux/*-light.png`): wordmark only (no mark), a nav menu of pages (Møder, Diktering, Måling, Indstillinger), footer with a full-width "Om" button and "Version x". Meetings are a separate page.

Differences: no logo mark; no record button; no search; meeting list is not in the sidebar; nav-menu model instead of Tauri's "list + footer actions"; no collapse; "Om" is a button + page instead of an ⓘ dialog; version text in sidebar does not exist in Tauri (Tauri shows it in About).

### 2.2 Home / recording (Port)

Tauri route `/` (`app/page.tsx:184-251`, `_components/TranscriptPanel.tsx`, `RecordingControls.tsx`, `RecordingStatusBar.tsx`, `_components/StatusOverlays.tsx`).
- Idle (`tom-tilstand-lys.png`): centered block (max 448): "Klar til dit næste møde" 30 semibold serif; 16 muted lead "Tingskriver optager og skriver referat på din computer. Lyden bliver her, og du kan læse noterne bagefter."; 14 muted "Tryk på \"Start optagelse\" nedenfor. Transskriptionen vises her, mens du optager." Bottom-centered floating pill (card, border, `rounded-full`, `shadow-md`, p 10/8) with "Start optagelse" (mic, primary pill button).
- Recording (`optagelse-i-gang-lys.png`): top-right "Kopiér" (outline sm) + "Sprog" button; status bar card (radius 8, border, px 16 py 12): pulsing recording dot, "Optager" semibold, mm:ss tabular, "Mikrofon: <name> · Systemlyd: <name>"; transcript lines with timestamp column (muted, tabular) and 16 px text, the live line on an accent background with dotted underline; "Lytter …" indicator. Pill: "Pause" (outline pill), "Stop og gem" (foreground pill), animated level bars (recording color, warning when paused).
- Paused (`optagelse-pause-*.png`): "På pause", warning dot, "Genoptag".
- Device switched (`optagelse-enhed-skiftet-*.png`): notice row inside the status bar (warning triangle or info icon, dismiss X) (`RecordingStatusBar.tsx:70-87`).
- Saving: spinner pill "Gemmer optagelsen…"; overlays "Gør transskriptionen færdig…", "Gemmer mødet…" (`StatusOverlays.tsx:29-57`).
- Errors: destructive alert under the pill with title and bullet text, close X (`RecordingControls.tsx:416-431`); titles "Mikrofonen er ikke tilgængelig", "Systemlyd er ikke tilgængelig", "Tilladelse mangler", "Optagelsen kunne ikke starte" (`:120-136`).
- Recovery dialog "Gendan afbrudte møder" (two panes: list + excerpt) (`TranscriptRecovery/TranscriptRecovery.tsx:121-177`).

GPUI: no home screen and no recording UI; "Ny optagelse" on the Møder page only explains that recording comes later (`strings::MEETING_RECORDING_LATER`, since replaced by `strings::RECORDING_NOT_IN_APP`). `interrupted_card` (sleep/logoff notice) and startup recovery live on the Møder page. Recording itself is a functional port tracked elsewhere; the parity job builds the screen and states with the existing core API where available and a disabled/explaining state otherwise.

### 2.3 Meeting list (Restyle -> move)

Tauri: the list **is** the sidebar (2.1). There is no meetings page.
GPUI: `Page::Meetings` -> `MeetingsView` (`meetings.rs`, 444 lines): heading "Møder", "Ny optagelse" + "Opdater" buttons, empty-state card (inbox icon, "Ingen møder endnu", "Når du har optaget dit første møde, står det her."), virtual list with date groups, inline rename/delete, recovery, MCP sharing block below (`ui.rs:728-756`).
Differences: move the virtual list into the sidebar slot with Tauri row styling; drop the page heading and the "Opdater" button (Tauri refreshes on events); inline rename becomes the "Omdøb møde" dialog or stays inline (allowed improvement, but keep Tauri's hover icons); empty text becomes Tauri's sidebar text; the per-meeting MCP sharing block moves to the meeting document (Tauri has `MeetingSharing` in `MeetingHeader.tsx:8`).

### 2.4 Meeting document (Port)

Tauri `app/meeting-details/page.tsx`, `page-content.tsx`, `MeetingDetails/MeetingDocument.tsx:33-76` (`moedeside-lys.png`, `moedeside-transskript-lys.png`, `moedeside-uden-opsummering-*.png`):
- Article column 736 max, px 32, pt 40, pb 112, scrolls the whole page.
- Header: title 36 semibold serif with hover pencil (inline rename), "29. okt. 2026 kl. 09.30" 14 muted; MCP sharing control (`MeetingHeader.tsx:80-102`).
- Toolbar (two outline sm button groups): [Lav om (Sparkles, primary text) | language dropdown "Automatisk" (Languages + chevron) | AI-model (Settings) | Skabelon (FileText)] and [Gem | Kopiér] (`SummaryGeneratorButtonGroup.tsx`, `SummaryPanel.tsx:107-111`). Labels collapse to icons in narrow containers.
- Summary: BlockNote editor, headings Literata (e.g. "Kort fortalt", "Beslutninger", "Handlepunkter", "Åbne spørgsmål" ~30 semibold), bullets 16. Loading: "Skriver opsummeringen…" + "Det kan tage et par minutter for lange møder." Error: `bg-destructive/10` box (`SummaryPanel.tsx:288-318`). Delivery approval under the summary (`delivery/DeliveryApproval.tsx`): target select, consent checkbox, blocked texts.
- Without summary: empty state (`EmptyStateSummary.tsx`) with "Lav opsummering".
- Transcript section: top border, 44 px disclosure row (chevron, "Transskript" 16 semibold, "10 linjer" muted, "Skjul/Vis" right), panel 70vh card with toolbar [Kopiér | Optagelse | Forbedr] and virtualized lines (timestamp + text) (`MeetingDocument.tsx:40-72`, `TranscriptButtonGroup.tsx`). Dialogs: model settings, retranscribe (`RetranscribeDialog.tsx`), audio player.

GPUI: none. `proposals.rs` (`ProposalReview`, band "N ventende ændringer", diff cards, Anvend/Afvis/Fortryd) is built but not mounted, waiting for this screen. **New** in GPUI: proposals band (place it between toolbar and summary, styled as the Tauri status-bar card).

### 2.5 Settings (Restyle + Port)

Tauri `app/settings/page.tsx:76-151` (`indstillinger-lys.png`, `indstillinger-diktering-lys.png`):
- Sticky header with bottom border, px 32 py 24: "Tilbage" (ArrowLeft 20 px, muted ghost) + "Indstillinger" 30 regular serif.
- Underline tabs (px 24 py 16, 16 px icon + label, active = primary text + 2 px primary bar sliding under, inactive muted, bottom border on the list): Generelt (Settings2), Optagelser (Mic), Transskription (Database), Opsummering (Sparkle), Diktering (AudioLines), Beta (FlaskConical).
- Sections: 24 regular serif title, 14 muted description, controls; sections separated by `border-t pt-32` within 672 max width.

| Tab | Tauri content (file) | GPUI today |
|---|---|---|
| Generelt | Udseende (segmented Følg systemet / Lyst / Mørkt), Notifikationer (switch row), Placering af data (Mødeoptagelser path + "Åbn mappen", Logfiler + switch), Aflevering af referat (privacy checkboxes, targets, add-target form, queue), AI-værktøjer (MCP) (access level select, copy setup buttons, `pre` blocks) (`PreferenceSettings.tsx`, `delivery/DeliverySettings.tsx`, `mcp/McpSettings.tsx`) | One scrolling page: an "Om" button, intro, MCP block (button rows), appearance (read-only text), languages, voiceprint, data folder, log, "Åbn logmappen", "Afslut" (`ui.rs:1009-1093`, `settings-light.png`). Appearance is read-only (no override); notifications and delivery missing. |
| Optagelser | "Gemmes i" folder, start-notice switch, default devices (mic/system selects) (`RecordingSettings.tsx`) | missing |
| Transskription | engine select (Parakeet/Whisper), model managers (download progress cards), language select + warnings (`TranscriptSettings.tsx`, `LanguageSelection.tsx`, `ParakeetModelManager.tsx`, `WhisperModelManager.tsx`) | `language_group` (meeting languages, report language) exists, rest missing |
| Opsummering | auto-summary switch, model provider/model, summary language, templates (`SummaryModelSettings.tsx`, `SummaryLanguageSettings.tsx`) | missing |
| Diktering | Genvej (switch, shortcut input + "Gem genvej"), Transskription (provider select, key field with Gem/Test/Fjern, fallback timeout), Renskrivning (try field, polish switch), Stil pr. program (rows exe + select + trash, add/save), Ordbog (word/heard-as, list with edit/trash), Snippets, Historik og privatliv (`dictation/DictationSettings.tsx`) | missing |
| Beta | feature switches with h3 titles (`BetaSettings.tsx`) | missing |

New in GPUI (adapt layout, keep Tauri section style): language group (meeting + report language, #209) -> Transskription tab; voiceprint explanation -> Beta tab (it is an off-by-default experiment); "Afslut" button -> Generelt, last section; MCP is not new (Tauri has it), but the GPUI version has more controls (data parts, test connection, tool list) -> keep them, styled as Tauri sections (select instead of four stretched buttons, toggle chips -> switches).

### 2.6 Diktering (Restyle)

Tauri `/diktering` (`dictation/DictationHistory.tsx:119-199`, `diktering-lys.png`, `diktering-tom-*.png`): 768 max, px 32 py 32. Header row: "Diktering" 30 regular serif + outline "Indstillinger" button (deep-links to settings tab). Text "Hold [Ctrl+Win] , tal, og slip. …" with a `kbd` chip (muted bg, border, radius 4). Status line (14 muted, e.g. "Genvejen er klar. Transskriberes med Syv.ai (lokal model som reserve)."). "Seneste dikteringer" 20 regular serif + ghost sm "Ryd historik". Cards (radius 8, border, card bg, p 16): text, meta line "dato · program · udbyder · varighed" (14 muted, "reserve brugt" in warning), Kopiér + trash. Empty: dashed card "Ingen dikteringer endnu"; loading: pulsing muted skeletons; history-off text.
GPUI `dictation_page` (`ui.rs:856-895`, `dictation-light.png`): placeholder text "comes later" plus the key-probe test field.
Differences: whole screen to build; keep the probe field as a collapsible "Test tastatur" section at the bottom (needed for the manual Windows tests in `docs/gpui.md`).

### 2.7 About (Restyle: page -> dialog)

Tauri: ⓘ next to Indstillinger opens a dialog "Om Tingskriver" (`Info.tsx:13-36`, `About.tsx:49-130`): centered logo, "Tingskriver" 20 semibold, "Version x" 14 muted, tagline, outline 44 px "Søg efter opdateringer" button (CheckCircle2 icon; spinner / check states, toasts "Du har den nyeste version"), "Det adskiller Tingskriver" with four dt/dd pairs, footer links (NOTICE.md, website, GitHub) 12 muted underlined. Update dialog `UpdateDialog.tsx`, toast `UpdateNotification.tsx` ("Opdatering klar").
GPUI `about_page` (`ui.rs:897-1007`, `about.rs`, `about-light.png`): page "Om" with build lines (version, build date, commit, channel, platform), "Opdateringer" status, stretched buttons "Kopiér diagnoseoplysninger", "Privatlivspolitik", "Kontakt", full licence text and third-party licences.
Plan: Tauri dialog layout on top; GPUI extras (build details, copy diagnostics, legal documents) as collapsed sections inside the dialog (max height 80 % of window, scroll). Keep `--page om` working by opening the dialog. Update screen (`update.rs` says "not built yet") follows Tauri `UpdateDialog`.

### 2.8 Onboarding (Port)

Tauri `onboarding/*` (`onboarding-lys.png`, `onboarding-model-lys.png`): full window, no sidebar. Top: "Trin 1 af 2" 14 muted + segmented progress bar (40x6 px rounded segments, gap 6, primary / muted) (`shared/ProgressIndicator.tsx`). Step 1 Welcome: "Velkommen til Tingskriver" 30 semibold serif, 16 muted lead (max 448), 14 muted note, full-width (max 320) primary "Kom i gang". Step 2 model download: cards "Transskriptionsmodel" / "Opsummeringsmodel" with icon, progress bar, states, retry, "continue in background" toast (`steps/DownloadProgressStep.tsx`). macOS-only permissions step (`steps/PermissionsStep.tsx`, permission rows).
GPUI: none.

### 2.9 Empty, loading and error states

| State | Tauri | GPUI | Difference |
|---|---|---|---|
| No meetings | sidebar text (2.1.4) + home idle hero | card with inbox icon on Møder page | follow Tauri |
| Loading meetings | "Søger…" / list appears; settings "Henter indstillinger…" | `loading` spinner + text (`loading-light.png`) | keep spinner, restyle to 14 muted, place in sidebar list slot |
| Database cannot open | `DatabaseGate.tsx:102-140`: left-aligned block max 576, 24 regular serif title, explanation, `dl` (Databasefil, Kopi af databasen, Logfil, Teknisk fejl with mono block), buttons "Åbn logmappen" + retry | `database_error_page` (`ui.rs:1506-1572`, `database-error-light.png`): 32 px red triangle, 26 semibold title, lines, mono block, primary + outline buttons | close already; change title to 24 regular, width 576, paths as dt/dd pairs, add the "Kopi af databasen" row if core reports it; keep the icon (allowed improvement) |
| Summary failed | `bg-destructive/10` box (2.4) | - | with 2.4 |
| Recording errors | alert under pill (2.2) | - | with 2.2 |
| Toasts | Sonner top-right, card bg, border, close button (`layout.tsx:296-303`) | gpui-component notifications | style notifications with card/border tokens in Job 1 |
| Empty diktering | dashed card | placeholder | with 2.6 |

### 2.10 GPUI-only screens

| GPUI screen | Tauri counterpart | Placement proposal |
|---|---|---|
| Måling (`measurement.rs`, `MeasurementView`, `measurement-light.png`) | none | Not a sidebar item (Tauri sidebar has no page list). Open from Indstillinger > Transskription ("Sammenlign modeller på din egen stemme") as a sub-page with the Tauri settings header ("Tilbage" + 30 regular title). Restyle: inputs 36 px, provider choices as checkbox rows instead of stretched buttons, tariff input inline per row, results as a card table. |
| MCP settings (`mcp_settings`) | Generelt > AI-værktøjer (MCP) | see 2.5 |
| Per-meeting MCP sharing (`meeting_sharing`) | `MeetingSharing` in meeting header | move into 2.4 |
| Language group (#209) | partial (`LanguageSelection.tsx` transcription language) | Transskription tab |
| Voiceprint text | none | Beta tab |
| Interrupted recording card (`interrupted_card`) | recovery dialog + toasts | show as a dismissible card above the home hero (status-bar card style), not on a meetings page |
| Proposals (`proposals.rs`) | none | in 2.4 |
| Pill overlay (`tingskriver-overlay`) | `diktering-pille*.png` | out of scope (not GPUI), already native |

Screen count: 9 Tauri screen areas (shell/sidebar, home/recording, meeting list, meeting document, settings with 6 tabs, Diktering, About, onboarding, empty/loading/error states) = 14 distinct screens counting tabs; 4 are missing entirely in GPUI (home/recording, meeting document, onboarding, 5 of 6 settings tabs), 6 GPUI-only items need placement.

---

## 3. Job plan

Shared rules for every job (acceptance criteria common to all):
- Branch from `origin/main` after the jobs it depends on have merged.
- No literal color/size in pages; new values go to `theme.rs` (Job 1 owns `theme.rs`; later jobs may only **add** constants in a clearly marked block, one block per job, to avoid conflicts).
- `cargo test -p tingskriver-app` green, including `theme::tests::text_meets_wcag_aa_in_both_modes`; any new text/background pair gets a row in `text_pairs`.
- Goldens: run the CI job "GPUI screenshots", download artifact `gpui-screenshots-linux`, inspect actual vs expected, then commit the new PNGs for **only the fixtures the job owns** (update path: `docs/gpui-screenshots.md#run-and-update`, `--update` locally under Xvfb is equivalent). Never `--update` to hide an unexplained diff. Compare side by side with the matching `docs/screenshots/*-lys.png` / `*-moerk.png` and attach both to the PR.
- `docs/design.md`: update Foundation (Job 1) or the job's component/experience rows; `python3 scripts/check-design-doc.py` green.
- Danish strings from `tingskriver_core::strings`, copied verbatim from the Tauri source (file:line in the PR).
- Light and dark both checked.
- `scripts/check-gpui-lock.py` green; no new gpui dependency versions.

### Job 1: Foundation: tokens, controls, file split (sequential, first)
Owns: `crates/tingskriver-app/src/theme.rs`, `src/assets.rs`, `assets/` (icons, logo mark), new `src/controls.rs` (button/input/switch/segmented/pill helpers), `src/ui.rs` (only `page`, `heading`, `section_title`, `paragraph`, `muted`, `setting_group` helpers and the file split below), `scripts/test-gpui-color-gate.py` (only if the light background token text changes), `docs/design.md` Foundation + Components tables, **all** goldens.
Work:
- Type scale, spacing, radii, control heights, content widths, icon sizes, shadow tokens per 1.3-1.5; `theme.shadow = true`; selection 0.22; alpha-blend hover replacing `emphasize` (tests rewritten); outline border = `input`.
- `heading(Level)` and serif `section_title`; content-width buttons (no full-width stretch).
- Add `public/tingskriver-mark.svg` and the lucide icons the jobs need (AudioLines, FileText, Upload, Search, X, ChevronLeft/Right, Pencil, Trash2, Copy, FolderOpen, RefreshCw, Sparkles, Languages, Square, Pause, ArrowLeft, Settings2, Database, FlaskConical, Monitor, Sun, Moon, Keyboard) if gpui-component lacks them.
- Split `ui.rs` (1845 lines) into `ui.rs` (Shell, Page, AppModel, helpers) + `pages/settings.rs`, `pages/dictation.rs`, `pages/about.rs`, `pages/database_error.rs` with no visual change beyond tokens, so later jobs own separate files. Update design.md module rows (`check-design-doc.py` needs them).
Acceptance: all 14 goldens regenerated and visibly using the new scale (buttons no longer stretched, headings per 1.3); AA tests green; color gate script still proves a changed color fails; no page logic changed.

### Job 2: Sidebar shell and meeting list in the sidebar (sequential, after 1)
Owns: new `src/sidebar.rs`, `src/meetings.rs`, `Shell::render` / `Page` enum in `ui.rs`, `src/args.rs` only if `--page` names change, goldens (all, since the sidebar is in every screen), design.md rows `sidebar`, `Page`, `MeetingsView`, `meetings_page`.
Work: Tauri sidebar 2.1 (mark + wordmark, "Ny optagelse" incl. recording state, search field wired to core search if available or disabled with tooltip, virtual meeting list with Tauri rows and hover actions, footer rows Diktering / Importér transskript / Importér lyd / Indstillinger + ⓘ, collapse to 64 px with tooltips). Remove the Møder page; default content = Home (placeholder from Job 3 until it lands: the idle hero). Måling leaves the nav (reachable via `--page måling` until Job 6 links it). Rename/delete dialogs per Tauri.
Acceptance: sidebar pixel-close to `moedeliste-lys.png` / `tom-tilstand-lys.png` at 1280x800 in both modes; empty and loading list states; 1,000-meeting virtualization test still green; keyboard focus order: record, search, list, footer; collapsed state screenshot added as a new fixture (`sidebar-collapsed-light/dark`).

After Job 2 merges, Jobs 3-8 can run **in parallel** (each touches only its own files and goldens):

### Job 3: Home and recording screen
Owns: new `src/pages/home.rs`, `src/recording.rs` (pill, status bar, transcript lines, overlays), `interrupted_card` (moved from `ui.rs` into `home.rs`), fixtures/goldens `home-*`, `recording-*` (add fixtures: idle, recording, paused, device switched, saving, start error), design.md rows.
Work: 2.2 visuals; wire to core recording API where present, otherwise disabled buttons with Tauri's explanatory text; recovery dialog per Tauri.
Acceptance: matches `tom-tilstand-*`, `optagelse-i-gang-*`, `optagelse-pause-*`, `optagelse-enhed-skiftet-*`; pill shadow-md; level bars respect reduced motion.

### Job 4: Meeting document
Owns: new `src/pages/meeting.rs`, `src/proposals.rs` (mount only), `meeting_sharing` (moved), fixtures/goldens `meeting-*` (with summary, without summary, transcript open, summary error, proposals pending), design.md "Meeting document" concept row changes from "Tauri only" to the new component.
Work: 2.4; summary as read-only rendered markdown first (BlockNote editing is a separate functional issue, state it in the PR); toolbars as button groups that collapse labels at narrow widths; delivery approval block.
Acceptance: matches `moedeside-*`, `moedeside-transskript-*`, `moedeside-uden-opsummering-*`; proposals band visible in fixture; no direct writes outside `ProposalStore`.

### Job 5: Settings frame + Generelt tab
Owns: `src/pages/settings.rs` (frame, header, tab bar, Generelt), `mcp_settings`, `language_group` (moved to Transskription tab container but code stays here), fixtures/goldens `settings-*` (one per tab: `settings-general-*`, `settings-transcription-*`, ...), design.md rows.
Work: header with "Tilbage", underline tabs with sliding indicator, section style; Generelt: Udseende segmented control (adds a real override, persisted through core settings; today GPUI only reads), notifications switch, data placement, delivery block, MCP block restyled, "Afslut" last. Other tabs render their sections with what GPUI has (languages, voiceprint) and a muted "Kommer i en senere version" line for missing controls.
Acceptance: matches `indstillinger-lys/moerk.png`; tab keyboard navigation (arrow keys); `--page indstillinger` opens Generelt.

### Job 6: Settings tabs Optagelser, Transskription (incl. Måling entry), Opsummering, Beta
Depends on Job 5 merged (shares the frame). Owns: `src/pages/settings_tabs/{recording,transcription,summary,beta}.rs`, `src/measurement.rs` (restyle + sub-page header), goldens `settings-recording-*`, `settings-transcription-*`, `settings-summary-*`, `settings-beta-*`, `measurement-*`.
Acceptance: section order and texts match the Tauri files in 2.5; Måling reached from Transskription and returns with "Tilbage"; controls without core support are visible but disabled with an explanation.

### Job 7: Diktering page + Diktering settings tab
Owns: `src/pages/dictation.rs`, `src/probe.rs`, `src/pages/settings_tabs/dictation.rs`, goldens `dictation-*`, `dictation-empty-*`, `settings-dictation-*`.
Can start in parallel with Job 5 for the page part; the settings tab part waits for Job 5's frame.
Acceptance: matches `diktering-lys/moerk.png`, `diktering-tom-*`, `indstillinger-diktering-*`; "Indstillinger" button deep-links to the Diktering tab; probe field still usable for manual Windows tests 2 and 3.

### Job 8: About dialog and update dialog
Owns: `src/pages/about.rs` -> `src/about_dialog.rs`, `src/about.rs`, update UI parts of `src/update.rs`, goldens `about-*` (now the dialog over the home screen), new `update-available-*`.
Acceptance: Tauri dialog layout first, GPUI diagnostics/legal sections collapsed below; ⓘ in the sidebar and `--page om` both open it; copying diagnostics still allowlisted.

### Job 9: Onboarding (parallel with 3-8)
Owns: new `src/onboarding.rs`, `Shell` hook to show it instead of sidebar+page (one small change in `ui.rs` `Shell::render`; coordinate by landing after Job 2), goldens `onboarding-welcome-*`, `onboarding-model-*`.
Acceptance: matches `onboarding-lys/moerk.png`, `onboarding-model-*`; model download progress through core; can continue in background.

### Job 10: States sweep (last)
Owns: `src/pages/database_error.rs`, `loading`, `empty_state`, notification styling, goldens `database-error-*`, `loading-*`, design.md Experiences table.
Work: 2.9; then a final side-by-side review of every golden against `docs/screenshots` and a list of remaining deliberate deviations in design.md.
Acceptance: every row of 2.9 handled; design.md "Reference screenshots" section points to the new goldens; no open P1 from a fresh-context review.

Order: 1 -> 2 -> {3, 4, 5, 7(page), 8, 9} in parallel -> {6, 7(tab)} after 5 -> 10. Total: 10 jobs.

Risks to state in each PR:
- gpui-component may not offer letter-spacing, sliding tab indicators or BlockNote-like editing; document any deviation in design.md instead of faking it.
- Several Tauri screens depend on functionality not yet ported to GPUI (recording, summaries, dictation settings, model downloads). These jobs build the look; missing behaviour must be shown as disabled with an honest explanation and linked to its functional issue, never silently faked.
- Goldens are taken at 2x and 1100x760; Tauri references are 1280x800 at 1x. Compare proportions, not pixels.
