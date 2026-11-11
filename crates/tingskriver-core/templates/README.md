# Meeting Summary Templates

This directory contains template definitions for meeting summary generation.

## Available Templates

### 1. `daily_standup.json`
Time-boxed daily updates template designed for engineering/product teams.

**Sections:**
- Date
- Attendees
- Yesterday (completed work)
- Today (planned work)
- Blockers
- Notes

### 2. `standard_meeting.json`
General-purpose meeting notes template focusing on key outcomes and actions.

**Sections:**
- Summary
- Key Decisions
- Action Items
- Discussion Highlights

## Template Structure

Each template JSON file follows this schema:

```json
{
  "name": "Template Name",
  "description": "Brief description of the template's purpose",
  "sections": [
    {
      "title": "Section Title",
      "instruction": "Instructions for the LLM on what to extract/include",
      "format": "paragraph|list|string",
      "item_format": "Optional: Markdown table format for list items"
    }
  ]
}
```

## Custom Templates

Users can add custom templates to the application data directory:

- **macOS**: `~/Library/Application Support/Meetily/templates/`
- **Windows**: `%APPDATA%\Meetily\templates\`
- **Linux**: `~/.config/Meetily/templates/`

Custom templates override built-in templates with the same filename.

## Template Fields

### Root Level
- `name` (required): Display name for the template
- `description` (required): Brief explanation of the template's use case
- `sections` (required): Array of section definitions

### Section Object
- `title` (required): Section heading text
- `instruction` (required): LLM guidance for this section
- `format` (required): One of `"paragraph"`, `"list"`, or `"string"`
- `item_format` (optional): Markdown formatting hint for list items (e.g., table structure)
- `example_item_format` (optional): Alternative formatting hint

## Usage in Code

Templates are loaded using the `templates` module:

```rust
use crate::summary::templates;

// Get a specific template
let template = templates::get_template("daily_standup")?;

// List available templates
let available = templates::list_templates();

// Validate custom template JSON
let custom_json = std::fs::read_to_string("custom.json")?;
let validated = templates::validate_template(&custom_json)?;
```

## Danish templates and calendar suggestions

The embedded Danish templates are `standard` (Standardreferat), `kunde`
(Kundemøde), `one_to_one` (1:1) and `intern_status` (Intern status).
Each contains Danish section instructions, decisions, tasks and open questions.
The existing English template identifiers remain available.

Core callers can use `templates::save_template(id, json).await` to create or
edit a custom template and `templates::export_template(id).await` to export its
complete JSON, including all section instructions and formatting hints. Saving a
built-in identifier creates a user override. Invalid JSON and unsafe identifiers
are rejected before writing; replacement is atomic. Disk work runs off the async
executor. The directory is the same platform data directory used by the existing
loader (`dirs::data_dir()/Meetily/templates`).

`summary::template_rules::select_template(event, calendar_config)` suggests 1:1
when the title contains `1:1`, otherwise Kunde when a non-declined attendee has a
domain outside the user's `own_addresses`, otherwise Standard. Domains compare
case-insensitively. Without configured own domains, external status is unknown
and the suggestion is Standard. Intern status is selected manually. The function
only returns an identifier and cannot modify meeting notes or saved summaries.

These are core APIs. Desktop editor/export controls and applying calendar
suggestions in the desktop meeting flow require separate shell integration.

Calendar selection rules are ordered JSON data. The embedded
`rules/calendar.json` defaults to a title containing `1:1`, then an external
attendee, then `standard`. `template_rules::save_rules(json).await` validates
and atomically saves edits to `templates/.rules/calendar.json`;
`load_rules()` reads them back. A final `default` rule is required. Invalid
saved files are logged and selection falls back to the embedded defaults.
Own addresses may also be bare domains; public mail domains do not imply
that other attendees belong to the same organization.

`TINGSKRIVER_TEMPLATES_DIR` overrides the custom template directory, including
rule storage, for isolated environments. The Tauri commands `save_template`
and `export_template` expose template editing and export to the desktop UI.
Template selection only chooses summary instructions; it does not edit saved
meeting notes. The saved-notes regression test covers this core boundary.
