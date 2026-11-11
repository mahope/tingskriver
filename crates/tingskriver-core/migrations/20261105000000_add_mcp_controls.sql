-- Keep the meeting selection separate from the master switch. Existing installs
-- also require an explicit opt-in after updating.
ALTER TABLE mcp_settings ADD COLUMN enabled INTEGER NOT NULL DEFAULT 0 CHECK(enabled IN (0,1));
ALTER TABLE mcp_settings ADD COLUMN notes INTEGER NOT NULL DEFAULT 0 CHECK(notes IN (0,1));
ALTER TABLE mcp_settings ADD COLUMN summary INTEGER NOT NULL DEFAULT 0 CHECK(summary IN (0,1));
ALTER TABLE mcp_settings ADD COLUMN transcript INTEGER NOT NULL DEFAULT 0 CHECK(transcript IN (0,1));
ALTER TABLE mcp_settings ADD COLUMN participants INTEGER NOT NULL DEFAULT 0 CHECK(participants IN (0,1));
