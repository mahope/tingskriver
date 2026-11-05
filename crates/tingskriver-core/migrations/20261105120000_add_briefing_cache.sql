-- Cache for meeting preparation (key points per minutes hash, joint text).
CREATE TABLE IF NOT EXISTS briefing_cache (
    cache_key TEXT PRIMARY KEY,
    payload TEXT NOT NULL,
    created_at TEXT NOT NULL
);
