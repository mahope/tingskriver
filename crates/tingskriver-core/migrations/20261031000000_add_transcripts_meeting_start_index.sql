-- Transcripts are always read per meeting in recording order (page view,
-- count, copy, summary) and deleted per meeting by ON DELETE CASCADE.
-- Without this index each of those scans the whole table.
CREATE INDEX IF NOT EXISTS idx_transcripts_meeting_start
    ON transcripts(meeting_id, audio_start_time);
