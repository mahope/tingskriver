CREATE TABLE meeting_note_documents (
    meeting_id TEXT PRIMARY KEY NOT NULL REFERENCES meetings(id) ON DELETE CASCADE
);
CREATE TABLE meeting_note_blocks (
    meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK(position >= 0),
    block_type TEXT NOT NULL CHECK(block_type IN ('paragraph','heading','bullet','numbered','checkbox')),
    level INTEGER CHECK(level BETWEEN 1 AND 3),
    checked INTEGER CHECK(checked IN (0,1)),
    text_markdown TEXT NOT NULL,
    created_offset_ms INTEGER CHECK(created_offset_ms >= 0),
    PRIMARY KEY(meeting_id, position)
);
