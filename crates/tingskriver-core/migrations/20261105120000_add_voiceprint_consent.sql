-- Voiceprints across meetings (issue #198). Biometric data (GDPR art. 9).
--
-- These tables hold NO biometric data: no embedding, no vector, no audio.
-- The embeddings themselves live only in Windows Credential Manager (see
-- src/voiceprint/store.rs). Only consent records and plain metadata are here.

-- One row with the user's choices. No row means the defaults: off, 90 days.
CREATE TABLE IF NOT EXISTS voiceprint_settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    enabled INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
    retention_days INTEGER NOT NULL DEFAULT 90 CHECK (retention_days IN (30, 90, 180)),
    last_purge_at TEXT
);

-- Consent per person. Withdrawn consents stay as a record (without biometrics).
CREATE TABLE IF NOT EXISTS voiceprint_consents (
    id TEXT PRIMARY KEY,
    person_name TEXT NOT NULL,
    consented_at TEXT NOT NULL,
    method TEXT NOT NULL,
    note TEXT,
    text_version INTEGER NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('active', 'withdrawn')),
    withdrawn_at TEXT
);

-- Append-only record of grants and withdrawals.
CREATE TABLE IF NOT EXISTS voiceprint_consent_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    consent_id TEXT NOT NULL,
    event TEXT NOT NULL CHECK (event IN ('granted', 'withdrawn')),
    at TEXT NOT NULL
);

-- Index of the exemplars in Credential Manager: ids and dates only.
CREATE TABLE IF NOT EXISTS voiceprint_exemplars (
    id TEXT PRIMARY KEY,
    consent_id TEXT NOT NULL REFERENCES voiceprint_consents(id),
    created_at TEXT NOT NULL,
    last_matched_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_voiceprint_exemplars_consent ON voiceprint_exemplars(consent_id);
