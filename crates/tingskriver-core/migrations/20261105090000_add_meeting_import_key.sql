-- Import key for meetings created from an existing transcript (.vtt/.srt).
-- Hash of the parsed content, so importing the same file twice finds the
-- existing meeting instead of creating a duplicate. NULL for recordings and
-- audio imports.
ALTER TABLE meetings ADD COLUMN import_key TEXT;
CREATE UNIQUE INDEX IF NOT EXISTS idx_meetings_import_key
    ON meetings(import_key) WHERE import_key IS NOT NULL;
