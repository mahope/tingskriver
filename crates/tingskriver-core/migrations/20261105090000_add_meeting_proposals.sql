-- AI proposals for notes and summaries (#205). A proposal never changes the
-- meeting; only an explicit apply by the user does. base_content is the text
-- the proposal was made against, so a changed base is detected exactly.
CREATE TABLE meeting_proposals (
    id TEXT PRIMARY KEY NOT NULL,
    meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
    field TEXT NOT NULL CHECK(field IN ('notes','summary')),
    base_updated_at TEXT,
    base_content TEXT NOT NULL,
    proposed_content TEXT NOT NULL,
    diff TEXT NOT NULL,
    source_kind TEXT NOT NULL CHECK(source_kind IN ('regenerate','assistant','chat','command_mode','mcp')),
    source_label TEXT NOT NULL DEFAULT '',
    session_id TEXT,
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','applied','rejected','stale','expired','undone')),
    status_reason TEXT,
    applied_content TEXT,
    created_at TEXT NOT NULL,
    resolved_at TEXT
);
CREATE INDEX meeting_proposals_meeting_status ON meeting_proposals(meeting_id, status);
CREATE INDEX meeting_proposals_session ON meeting_proposals(session_id) WHERE session_id IS NOT NULL;
