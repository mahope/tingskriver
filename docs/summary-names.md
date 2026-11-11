# Preferred names and generated meeting titles

Dictionary terms and attendee names are included in the English report prompt,
the translation prompt (including Danish), and the report-based title prompt.
Only attendee display names are included, never email addresses.

Attendees come from a local calendar cache and must belong to a timed event
that overlaps the recording. The recording interval uses `metadata.json`'s
recording start (`created_at`) and `duration_seconds`, not the database row's
creation time. Imports and recordings without reliable metadata contribute no
calendar names. No application caller currently refreshes `calendar/events.json`,
so calendar names are only available when that cache has been populated.

A successful empty title response, including `<EMPTY>`, keeps the existing
meeting title. Only a failed title call may use the report's heading as a
fallback. Placeholder headings such as `<Add Title here>` and `Meeting Summary`
are rejected. User-chosen meeting titles are preserved by the repository update.
