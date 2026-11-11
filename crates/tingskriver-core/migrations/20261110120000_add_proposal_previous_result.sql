-- Preserve the complete summary result for lossless undo.
ALTER TABLE meeting_proposals ADD COLUMN previous_result TEXT;
