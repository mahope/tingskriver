-- Stable integer keys survive VACUUM, unlike implicit source-table rowids.
CREATE TABLE search_documents (
    id INTEGER PRIMARY KEY,
    body TEXT,
    source TEXT NOT NULL,
    source_id TEXT NOT NULL,
    meeting_id TEXT NOT NULL,
    time_seconds REAL,
    UNIQUE(source, source_id)
);

-- One shared corpus makes bm25 scores comparable across sources.
CREATE VIRTUAL TABLE search_fts USING fts5(
    body, source UNINDEXED, source_id UNINDEXED,
    meeting_id UNINDEXED, time_seconds UNINDEXED,
    content = 'search_documents', content_rowid = 'id',
    tokenize = 'unicode61 remove_diacritics 0'
);

CREATE TRIGGER search_documents_insert AFTER INSERT ON search_documents BEGIN
    INSERT INTO search_fts(rowid, body, source, source_id, meeting_id, time_seconds)
    VALUES (new.id, new.body, new.source, new.source_id, new.meeting_id, new.time_seconds);
END;

CREATE TRIGGER search_documents_delete AFTER DELETE ON search_documents BEGIN
    INSERT INTO search_fts(search_fts, rowid, body, source, source_id, meeting_id, time_seconds)
    VALUES ('delete', old.id, old.body, old.source, old.source_id, old.meeting_id, old.time_seconds);
END;

-- This projection is also the single source of truth for manual rebuilds.
CREATE VIEW search_source_documents AS
SELECT title AS body, 'title' AS source, id AS source_id, id AS meeting_id, NULL AS time_seconds FROM meetings
UNION ALL
SELECT COALESCE(NULLIF(notes_markdown, ''), CASE WHEN json_valid(notes_json) THEN CASE WHEN json_type(notes_json, '$.markdown') = 'text' THEN json_extract(notes_json, '$.markdown') ELSE (SELECT group_concat(atom, ' ') FROM json_tree(notes_json) WHERE type = 'text') END ELSE notes_json END) AS body, 'note' AS source, meeting_id AS source_id, meeting_id AS meeting_id, NULL AS time_seconds FROM meeting_notes
UNION ALL
SELECT CASE WHEN json_valid(result) THEN CASE WHEN json_type(result, '$.markdown') = 'text' THEN json_extract(result, '$.markdown') ELSE (SELECT group_concat(atom, ' ') FROM json_tree(result) WHERE type = 'text') END ELSE result END AS body, 'summary' AS source, meeting_id AS source_id, meeting_id AS meeting_id, NULL AS time_seconds FROM summary_processes
UNION ALL
SELECT transcript AS body, 'transcript' AS source, id AS source_id, meeting_id AS meeting_id, audio_start_time AS time_seconds FROM transcripts;

CREATE TRIGGER search_meetings_insert AFTER INSERT ON meetings BEGIN
    INSERT INTO search_documents(body, source, source_id, meeting_id, time_seconds)
    SELECT title AS body, 'title' AS source, id AS source_id, id AS meeting_id, NULL AS time_seconds FROM meetings WHERE rowid = new.rowid;
END;

CREATE TRIGGER search_meetings_update AFTER UPDATE ON meetings BEGIN
    DELETE FROM search_documents WHERE source = 'title' AND source_id = old.id;
    INSERT INTO search_documents(body, source, source_id, meeting_id, time_seconds)
    SELECT title AS body, 'title' AS source, id AS source_id, id AS meeting_id, NULL AS time_seconds FROM meetings WHERE rowid = new.rowid;
END;

CREATE TRIGGER search_meetings_delete AFTER DELETE ON meetings BEGIN
    DELETE FROM search_documents WHERE source = 'title' AND source_id = old.id;
END;

CREATE TRIGGER search_meeting_notes_insert AFTER INSERT ON meeting_notes BEGIN
    INSERT INTO search_documents(body, source, source_id, meeting_id, time_seconds)
    SELECT COALESCE(NULLIF(notes_markdown, ''), CASE WHEN json_valid(notes_json) THEN CASE WHEN json_type(notes_json, '$.markdown') = 'text' THEN json_extract(notes_json, '$.markdown') ELSE (SELECT group_concat(atom, ' ') FROM json_tree(notes_json) WHERE type = 'text') END ELSE notes_json END) AS body, 'note' AS source, meeting_id AS source_id, meeting_id AS meeting_id, NULL AS time_seconds FROM meeting_notes WHERE rowid = new.rowid;
END;

CREATE TRIGGER search_meeting_notes_update AFTER UPDATE ON meeting_notes BEGIN
    DELETE FROM search_documents WHERE source = 'note' AND source_id = old.meeting_id;
    INSERT INTO search_documents(body, source, source_id, meeting_id, time_seconds)
    SELECT COALESCE(NULLIF(notes_markdown, ''), CASE WHEN json_valid(notes_json) THEN CASE WHEN json_type(notes_json, '$.markdown') = 'text' THEN json_extract(notes_json, '$.markdown') ELSE (SELECT group_concat(atom, ' ') FROM json_tree(notes_json) WHERE type = 'text') END ELSE notes_json END) AS body, 'note' AS source, meeting_id AS source_id, meeting_id AS meeting_id, NULL AS time_seconds FROM meeting_notes WHERE rowid = new.rowid;
END;

CREATE TRIGGER search_meeting_notes_delete AFTER DELETE ON meeting_notes BEGIN
    DELETE FROM search_documents WHERE source = 'note' AND source_id = old.meeting_id;
END;

CREATE TRIGGER search_summary_processes_insert AFTER INSERT ON summary_processes BEGIN
    INSERT INTO search_documents(body, source, source_id, meeting_id, time_seconds)
    SELECT CASE WHEN json_valid(result) THEN CASE WHEN json_type(result, '$.markdown') = 'text' THEN json_extract(result, '$.markdown') ELSE (SELECT group_concat(atom, ' ') FROM json_tree(result) WHERE type = 'text') END ELSE result END AS body, 'summary' AS source, meeting_id AS source_id, meeting_id AS meeting_id, NULL AS time_seconds FROM summary_processes WHERE rowid = new.rowid;
END;

CREATE TRIGGER search_summary_processes_update AFTER UPDATE ON summary_processes BEGIN
    DELETE FROM search_documents WHERE source = 'summary' AND source_id = old.meeting_id;
    INSERT INTO search_documents(body, source, source_id, meeting_id, time_seconds)
    SELECT CASE WHEN json_valid(result) THEN CASE WHEN json_type(result, '$.markdown') = 'text' THEN json_extract(result, '$.markdown') ELSE (SELECT group_concat(atom, ' ') FROM json_tree(result) WHERE type = 'text') END ELSE result END AS body, 'summary' AS source, meeting_id AS source_id, meeting_id AS meeting_id, NULL AS time_seconds FROM summary_processes WHERE rowid = new.rowid;
END;

CREATE TRIGGER search_summary_processes_delete AFTER DELETE ON summary_processes BEGIN
    DELETE FROM search_documents WHERE source = 'summary' AND source_id = old.meeting_id;
END;

CREATE TRIGGER search_transcripts_insert AFTER INSERT ON transcripts BEGIN
    INSERT INTO search_documents(body, source, source_id, meeting_id, time_seconds)
    SELECT transcript AS body, 'transcript' AS source, id AS source_id, meeting_id AS meeting_id, audio_start_time AS time_seconds FROM transcripts WHERE rowid = new.rowid;
END;

CREATE TRIGGER search_transcripts_update AFTER UPDATE ON transcripts BEGIN
    DELETE FROM search_documents WHERE source = 'transcript' AND source_id = old.id;
    INSERT INTO search_documents(body, source, source_id, meeting_id, time_seconds)
    SELECT transcript AS body, 'transcript' AS source, id AS source_id, meeting_id AS meeting_id, audio_start_time AS time_seconds FROM transcripts WHERE rowid = new.rowid;
END;

CREATE TRIGGER search_transcripts_delete AFTER DELETE ON transcripts BEGIN
    DELETE FROM search_documents WHERE source = 'transcript' AND source_id = old.id;
END;

INSERT INTO search_documents(body, source, source_id, meeting_id, time_seconds)
SELECT body, source, source_id, meeting_id, time_seconds FROM search_source_documents;
