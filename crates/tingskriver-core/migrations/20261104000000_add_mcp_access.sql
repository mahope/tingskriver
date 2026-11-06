ALTER TABLE meetings ADD COLUMN ai_shared INTEGER NOT NULL DEFAULT 0 CHECK(ai_shared IN (0,1));
CREATE TABLE mcp_settings (
    id INTEGER PRIMARY KEY CHECK(id = 1),
    access TEXT NOT NULL CHECK(access IN ('all','marked','metadata','off'))
);
INSERT INTO mcp_settings VALUES (1, 'marked');
