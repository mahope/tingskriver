-- Cache for meeting preparation (key points per minutes hash, joint text).
CREATE TABLE IF NOT EXISTS briefing_cache (
    cache_key TEXT NOT NULL,
    meeting_id TEXT NOT NULL REFERENCES meetings(id),
    payload TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (cache_key, meeting_id)
);
CREATE INDEX briefing_cache_meeting ON briefing_cache(meeting_id);

-- A joint overview must disappear when any of its source meetings is deleted.
-- A trigger also covers connections where foreign keys are disabled.
CREATE TRIGGER briefing_cache_delete_meeting BEFORE DELETE ON meetings
BEGIN
    DELETE FROM briefing_cache WHERE cache_key IN (
        SELECT cache_key FROM briefing_cache WHERE meeting_id = OLD.id
    );
END;
