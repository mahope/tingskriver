//! Dictation data in its own SQLite file (`dictation.sqlite` in app data):
//! settings, dictionary, snippets and history. Kept apart from the meeting
//! database so the two never block each other.

use std::path::Path;

use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};

use super::logic::cleanup::{Replacement, Rules, Snippet};
use super::settings::DictationSettings;

/// How many history entries are kept.
pub const HISTORY_LIMIT: i64 = 500;
/// Recordings are deleted after this long.
pub const AUDIO_RETENTION_MS: i64 = 24 * 60 * 60 * 1000;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS dictation_settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS dictation_dictionary (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    word TEXT NOT NULL,
    aliases TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS dictation_suggestions (
    word_key TEXT PRIMARY KEY,
    word TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'pending' CHECK(state IN ('pending', 'ignored', 'added'))
);
CREATE TRIGGER IF NOT EXISTS trim_dictation_suggestions
AFTER INSERT ON dictation_suggestions
BEGIN
    DELETE FROM dictation_suggestions WHERE rowid IN (
        SELECT rowid FROM dictation_suggestions WHERE state = 'pending'
        ORDER BY rowid DESC LIMIT -1 OFFSET 200
    );
END;
DELETE FROM dictation_suggestions WHERE rowid IN (
    SELECT rowid FROM dictation_suggestions WHERE state = 'pending'
    ORDER BY rowid DESC LIMIT -1 OFFSET 200
);
CREATE TABLE IF NOT EXISTS dictation_snippets (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    phrase TEXT NOT NULL,
    text TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS dictation_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at INTEGER NOT NULL,
    text TEXT NOT NULL,
    app TEXT,
    style TEXT NOT NULL,
    engine TEXT NOT NULL,
    fallback_reason TEXT,
    latency_ms INTEGER NOT NULL,
    audio_ms INTEGER NOT NULL,
    insert_method TEXT,
    audio_path TEXT
);
CREATE INDEX IF NOT EXISTS idx_dictation_history_created ON dictation_history(created_at);
"#;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryWord {
    pub id: i64,
    pub word: String,
    /// Comma separated spellings the speech model tends to produce.
    pub aliases: String,
    /// "Behold engelsk stavning": the word is English and kept exactly as
    /// written, also by polishing.
    #[serde(default)]
    pub english: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnippetRow {
    pub id: i64,
    pub phrase: String,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: i64,
    pub created_at: i64,
    pub text: String,
    pub app: Option<String>,
    pub style: String,
    pub engine: String,
    pub provider: Option<String>,
    pub syv_model: Option<String>,
    pub language: Option<String>,
    pub fallback_reason: Option<String>,
    pub latency_ms: i64,
    pub audio_ms: i64,
    pub insert_method: Option<String>,
    pub has_audio: bool,
}

#[derive(Clone, Debug)]
pub struct NewHistoryEntry {
    pub created_at: i64,
    pub text: String,
    pub app: Option<String>,
    pub style: String,
    pub engine: String,
    pub provider: Option<String>,
    pub syv_model: Option<String>,
    pub language: Option<String>,
    pub fallback_reason: Option<String>,
    pub latency_ms: i64,
    pub audio_ms: i64,
    pub insert_method: Option<String>,
    pub audio_path: Option<String>,
}

#[derive(Clone)]
pub struct Store {
    pool: SqlitePool,
}

fn db_err(e: sqlx::Error) -> String {
    format!("Databasefejl (diktering): {e}")
}

impl Store {
    pub async fn open(path: &Path) -> Result<Store, String> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| e.to_string())?;
        }
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal);
        let pool = SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await
            .map_err(db_err)?;
        Self::with_pool(pool).await
    }

    pub async fn with_pool(pool: SqlitePool) -> Result<Store, String> {
        sqlx::raw_sql(SCHEMA).execute(&pool).await.map_err(db_err)?;
        migrate(&pool).await?;
        Ok(Store { pool })
    }

    /// The pool, for tables kept in their own module (`timings`).
    pub fn pool(&self) -> SqlitePool {
        self.pool.clone()
    }

    // -- settings ----------------------------------------------------------

    pub async fn settings(&self) -> Result<DictationSettings, String> {
        let row = sqlx::query("SELECT value FROM dictation_settings WHERE key = 'settings'")
            .fetch_optional(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(match row {
            Some(row) => {
                let value: String = row.get("value");
                serde_json::from_str(&value).unwrap_or_else(|e| {
                    log::warn!("Dictation settings unreadable, using defaults: {e}");
                    DictationSettings::default()
                })
            }
            None => DictationSettings::default(),
        })
    }

    pub async fn save_settings(&self, settings: &DictationSettings) -> Result<(), String> {
        let value = serde_json::to_string(settings).map_err(|e| e.to_string())?;
        sqlx::query(
            "INSERT INTO dictation_settings (key, value) VALUES ('settings', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(value)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }

    // -- dictionary --------------------------------------------------------

    pub async fn dictionary(&self) -> Result<Vec<DictionaryWord>, String> {
        let rows = sqlx::query("SELECT id, word, aliases, english FROM dictation_dictionary ORDER BY word COLLATE NOCASE")
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| DictionaryWord {
                id: r.get("id"),
                word: r.get("word"),
                aliases: r.get("aliases"),
                english: r.get::<i64, _>("english") != 0,
            })
            .collect())
    }

    pub async fn upsert_word(
        &self,
        id: Option<i64>,
        word: &str,
        aliases: &str,
        now: i64,
    ) -> Result<i64, String> {
        let word = word.trim();
        if word.is_empty() {
            return Err("Ordet må ikke være tomt".to_string());
        }
        let aliases = normalize_aliases(aliases);
        match id {
            Some(id) => {
                sqlx::query(
                    "UPDATE dictation_dictionary SET word = ?1, aliases = ?2 WHERE id = ?3",
                )
                .bind(word)
                .bind(aliases)
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(db_err)?;
                Ok(id)
            }
            None => {
                let result = sqlx::query("INSERT INTO dictation_dictionary (word, aliases, created_at) VALUES (?1, ?2, ?3)")
                    .bind(word)
                    .bind(aliases)
                    .bind(now)
                    .execute(&self.pool)
                    .await
                    .map_err(db_err)?;
                Ok(result.last_insert_rowid())
            }
        }
    }

    /// Mark a dictionary word as English (or not).
    pub async fn set_word_english(&self, id: i64, english: bool) -> Result<(), String> {
        sqlx::query("UPDATE dictation_dictionary SET english = ?1 WHERE id = ?2")
            .bind(i64::from(english))
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    pub async fn delete_word(&self, id: i64) -> Result<(), String> {
        sqlx::query("DELETE FROM dictation_dictionary WHERE id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    /// Store only changed words, never the snapshots or keystrokes.
    pub async fn suggest_corrections(
        &self,
        inserted: &str,
        observed: &str,
        elapsed: std::time::Duration,
        password: bool,
    ) -> Result<(), String> {
        let words = super::logic::suggest::corrected_words(inserted, observed, elapsed, password);
        if words.is_empty() {
            return Ok(());
        }
        let dictionary = self.dictionary().await?;
        for word in words {
            let known = dictionary
                .iter()
                .any(|entry| entry.word.to_lowercase() == word.to_lowercase());
            if !known {
                sqlx::query(
                    "INSERT OR IGNORE INTO dictation_suggestions (word_key, word) VALUES (?1, ?2)",
                )
                .bind(word.to_lowercase())
                .bind(word)
                .execute(&self.pool)
                .await
                .map_err(db_err)?;
            }
        }
        Ok(())
    }

    pub async fn suggestions(&self) -> Result<Vec<String>, String> {
        let rows = sqlx::query(
            "SELECT word FROM dictation_suggestions WHERE state = 'pending' ORDER BY word_key",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows.into_iter().map(|row| row.get("word")).collect())
    }

    /// Forget all pending words and remembered decisions for privacy controls.
    pub async fn clear_suggestions(&self) -> Result<(), String> {
        sqlx::query("DELETE FROM dictation_suggestions")
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    /// The only decisions are explicit addition or permanent ignoring.
    pub async fn resolve_suggestion(&self, word: &str, add: bool, now: i64) -> Result<(), String> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        let row = sqlx::query("UPDATE dictation_suggestions SET state = ?1 WHERE word_key = ?2 AND state = 'pending' RETURNING word")
            .bind(if add { "added" } else { "ignored" }).bind(word.to_lowercase())
            .fetch_optional(&mut *tx).await.map_err(db_err)?;
        if add {
            if let Some(row) = row {
                let word: String = row.get("word");
                let word_key = word.to_lowercase();
                let dictionary = sqlx::query("SELECT word FROM dictation_dictionary")
                    .fetch_all(&mut *tx)
                    .await
                    .map_err(db_err)?;
                let known = dictionary
                    .iter()
                    .any(|entry| entry.get::<String, _>("word").to_lowercase() == word_key);
                if !known {
                    sqlx::query("INSERT INTO dictation_dictionary (word, aliases, created_at) VALUES (?1, '', ?2)")
                        .bind(word).bind(now).execute(&mut *tx).await.map_err(db_err)?;
                }
            }
        }
        tx.commit().await.map_err(db_err)?;
        Ok(())
    }

    // -- snippets ----------------------------------------------------------

    pub async fn snippets(&self) -> Result<Vec<SnippetRow>, String> {
        let rows = sqlx::query(
            "SELECT id, phrase, text FROM dictation_snippets ORDER BY phrase COLLATE NOCASE",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| SnippetRow {
                id: r.get("id"),
                phrase: r.get("phrase"),
                text: r.get("text"),
            })
            .collect())
    }

    pub async fn upsert_snippet(
        &self,
        id: Option<i64>,
        phrase: &str,
        text: &str,
        now: i64,
    ) -> Result<i64, String> {
        let phrase = phrase.trim();
        if phrase.is_empty() || text.trim().is_empty() {
            return Err("Både udløser og tekst skal udfyldes".to_string());
        }
        match id {
            Some(id) => {
                sqlx::query("UPDATE dictation_snippets SET phrase = ?1, text = ?2 WHERE id = ?3")
                    .bind(phrase)
                    .bind(text)
                    .bind(id)
                    .execute(&self.pool)
                    .await
                    .map_err(db_err)?;
                Ok(id)
            }
            None => {
                let result = sqlx::query(
                    "INSERT INTO dictation_snippets (phrase, text, created_at) VALUES (?1, ?2, ?3)",
                )
                .bind(phrase)
                .bind(text)
                .bind(now)
                .execute(&self.pool)
                .await
                .map_err(db_err)?;
                Ok(result.last_insert_rowid())
            }
        }
    }

    pub async fn delete_snippet(&self, id: i64) -> Result<(), String> {
        sqlx::query("DELETE FROM dictation_snippets WHERE id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    /// Dictionary and snippets as cleanup rules.
    pub async fn rules(&self) -> Result<Rules, String> {
        let mut replacements = Vec::new();
        for word in self.dictionary().await? {
            replacements.push(Replacement {
                from: word.word.clone(),
                to: word.word.clone(),
                keep_english: word.english,
            });
            for alias in word
                .aliases
                .split(',')
                .map(str::trim)
                .filter(|a| !a.is_empty())
            {
                replacements.push(Replacement {
                    from: alias.to_string(),
                    to: word.word.clone(),
                    keep_english: word.english,
                });
            }
        }
        let snippets = self
            .snippets()
            .await?
            .into_iter()
            .map(|s| Snippet {
                trigger: s.phrase,
                text: s.text,
            })
            .collect();
        Ok(Rules {
            replacements,
            snippets,
            ..Rules::default()
        })
    }

    // -- history -----------------------------------------------------------

    /// Add an entry and trim to [`HISTORY_LIMIT`]. Returns audio files of
    /// trimmed entries, which the caller deletes.
    pub async fn add_history(&self, entry: NewHistoryEntry) -> Result<Vec<String>, String> {
        sqlx::query(
            "INSERT INTO dictation_history
             (created_at, text, app, style, engine, fallback_reason, latency_ms, audio_ms, insert_method, audio_path, provider, syv_model, language)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        )
        .bind(entry.created_at)
        .bind(entry.text)
        .bind(entry.app)
        .bind(entry.style)
        .bind(entry.engine)
        .bind(entry.fallback_reason)
        .bind(entry.latency_ms)
        .bind(entry.audio_ms)
        .bind(entry.insert_method)
        .bind(entry.audio_path)
        .bind(entry.provider)
        .bind(entry.syv_model)
        .bind(entry.language)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;

        let cutoff =
            "SELECT id FROM dictation_history ORDER BY created_at DESC, id DESC LIMIT -1 OFFSET ?1";
        let paths = sqlx::query(&format!(
            "SELECT audio_path FROM dictation_history WHERE audio_path IS NOT NULL AND id IN ({cutoff})"
        ))
        .bind(HISTORY_LIMIT)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?
        .into_iter()
        .map(|r| r.get::<String, _>("audio_path"))
        .collect();
        sqlx::query(&format!(
            "DELETE FROM dictation_history WHERE id IN ({cutoff})"
        ))
        .bind(HISTORY_LIMIT)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(paths)
    }

    pub async fn history(&self, limit: i64) -> Result<Vec<HistoryEntry>, String> {
        let rows = sqlx::query(
            "SELECT id, created_at, text, app, style, engine, fallback_reason, latency_ms, audio_ms,
                    insert_method, provider, syv_model, language, audio_path IS NOT NULL AS has_audio
             FROM dictation_history ORDER BY created_at DESC, id DESC LIMIT ?1",
        )
        .bind(limit.clamp(1, HISTORY_LIMIT))
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| HistoryEntry {
                id: r.get("id"),
                created_at: r.get("created_at"),
                text: r.get("text"),
                app: r.get("app"),
                style: r.get("style"),
                engine: r.get("engine"),
                provider: r.get("provider"),
                syv_model: r.get("syv_model"),
                language: r.get("language"),
                fallback_reason: r.get("fallback_reason"),
                latency_ms: r.get("latency_ms"),
                audio_ms: r.get("audio_ms"),
                insert_method: r.get("insert_method"),
                has_audio: r.get::<i64, _>("has_audio") != 0,
            })
            .collect())
    }

    /// Delete one entry; returns its audio file if any.
    pub async fn delete_history(&self, id: i64) -> Result<Option<String>, String> {
        let path: Option<String> =
            sqlx::query("SELECT audio_path FROM dictation_history WHERE id = ?1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?
                .and_then(|r| r.get("audio_path"));
        sqlx::query("DELETE FROM dictation_history WHERE id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(path)
    }

    /// Delete all entries; returns their audio files.
    pub async fn clear_history(&self) -> Result<Vec<String>, String> {
        let paths =
            sqlx::query("SELECT audio_path FROM dictation_history WHERE audio_path IS NOT NULL")
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?
                .into_iter()
                .map(|r| r.get::<String, _>("audio_path"))
                .collect();
        sqlx::query("DELETE FROM dictation_history")
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(paths)
    }

    /// Forget recordings older than [`AUDIO_RETENTION_MS`]; returns their files.
    pub async fn expire_audio(&self, now: i64) -> Result<Vec<String>, String> {
        let cutoff = now - AUDIO_RETENTION_MS;
        let paths = sqlx::query("SELECT audio_path FROM dictation_history WHERE audio_path IS NOT NULL AND created_at < ?1")
            .bind(cutoff)
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|r| r.get::<String, _>("audio_path"))
            .collect();
        sqlx::query("UPDATE dictation_history SET audio_path = NULL WHERE audio_path IS NOT NULL AND created_at < ?1")
            .bind(cutoff)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(paths)
    }
}

/// Columns added after the first release.
async fn migrate(pool: &SqlitePool) -> Result<(), String> {
    let history_columns = sqlx::query("SELECT name FROM pragma_table_info('dictation_history')")
        .fetch_all(pool)
        .await
        .map_err(db_err)?;
    for name in ["provider", "syv_model", "language"] {
        if !history_columns
            .iter()
            .any(|r| r.get::<String, _>("name") == name)
        {
            sqlx::query(&format!(
                "ALTER TABLE dictation_history ADD COLUMN {name} TEXT"
            ))
            .execute(pool)
            .await
            .map_err(db_err)?;
        }
    }

    let columns = sqlx::query("SELECT name FROM pragma_table_info('dictation_dictionary')")
        .fetch_all(pool)
        .await
        .map_err(db_err)?;
    let has = |name: &str| columns.iter().any(|r| r.get::<String, _>("name") == name);
    if !has("english") {
        sqlx::query(
            "ALTER TABLE dictation_dictionary ADD COLUMN english INTEGER NOT NULL DEFAULT 0",
        )
        .execute(pool)
        .await
        .map_err(db_err)?;
    }
    Ok(())
}

fn normalize_aliases(aliases: &str) -> String {
    aliases
        .split(',')
        .map(str::trim)
        .filter(|a| !a.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;

    async fn store() -> Store {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        Store::with_pool(pool).await.unwrap()
    }

    fn entry(created_at: i64, audio: Option<&str>) -> NewHistoryEntry {
        NewHistoryEntry {
            created_at,
            text: format!("tekst {created_at}"),
            app: Some("Code.exe".into()),
            style: "raw".into(),
            engine: "syv".into(),
            provider: None,
            syv_model: None,
            language: None,
            fallback_reason: None,
            latency_ms: 900,
            audio_ms: 4000,
            insert_method: Some("paste".into()),
            audio_path: audio.map(str::to_string),
        }
    }

    #[tokio::test]
    async fn inserted_sentences_and_case_changes_are_not_stored() {
        let s = store().await;
        for observed in [
            "Hej kære Søren",
            "Hej Søren. Her er en ny sætning.",
            "HEJ SØREN",
        ] {
            s.suggest_corrections("Hej Søren", observed, std::time::Duration::ZERO, false)
                .await
                .unwrap();
        }
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM dictation_suggestions")
            .fetch_one(&s.pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn pending_suggestions_are_bounded_and_all_states_can_be_cleared() {
        let s = store().await;
        s.suggest_corrections("Soren Ase", "Søren Åse", std::time::Duration::ZERO, false)
            .await
            .unwrap();
        s.resolve_suggestion("søren", false, 1).await.unwrap();
        s.resolve_suggestion("åse", true, 1).await.unwrap();
        for i in 0..201 {
            sqlx::query("INSERT INTO dictation_suggestions (word_key, word) VALUES (?1, ?1)")
                .bind(format!("candidate{i:03}"))
                .execute(&s.pool)
                .await
                .unwrap();
        }
        let pending = s.suggestions().await.unwrap();
        assert_eq!(pending.len(), 200);
        assert!(!pending.contains(&"candidate000".to_string()));
        assert!(pending.contains(&"candidate200".to_string()));
        s.clear_suggestions().await.unwrap();
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM dictation_suggestions")
            .fetch_one(&s.pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
        assert_eq!(s.dictionary().await.unwrap().len(), 1);
        s.suggest_corrections("Soren Ase", "Søren Åse", std::time::Duration::ZERO, false)
            .await
            .unwrap();
        assert_eq!(s.suggestions().await.unwrap(), vec!["Søren"]);
    }

    #[tokio::test]
    async fn opening_existing_database_trims_pending_suggestions() {
        let s = store().await;
        sqlx::query("DROP TRIGGER trim_dictation_suggestions")
            .execute(&s.pool)
            .await
            .unwrap();
        for i in 0..205 {
            sqlx::query("INSERT INTO dictation_suggestions (word_key, word) VALUES (?1, ?1)")
                .bind(format!("candidate{i:03}"))
                .execute(&s.pool)
                .await
                .unwrap();
        }
        let s = Store::with_pool(s.pool.clone()).await.unwrap();
        let pending = s.suggestions().await.unwrap();
        assert_eq!(pending.len(), 200);
        assert_eq!(pending.first().unwrap(), "candidate005");
        assert_eq!(pending.last().unwrap(), "candidate204");
    }

    #[tokio::test]
    async fn suggestions_require_explicit_decisions_and_remember_ignored_words() {
        let s = store().await;
        let elapsed = std::time::Duration::from_secs(30);
        s.suggest_corrections("Soren og Ase", "Søren og Åse", elapsed, false)
            .await
            .unwrap();
        assert_eq!(s.suggestions().await.unwrap(), vec!["Søren", "Åse"]);
        assert!(s.dictionary().await.unwrap().is_empty());
        s.resolve_suggestion("søren", false, 1).await.unwrap();
        s.resolve_suggestion("Åse", true, 1).await.unwrap();
        s.resolve_suggestion("Åse", true, 1).await.unwrap();
        assert_eq!(s.dictionary().await.unwrap().len(), 1);
        let s = Store::with_pool(s.pool.clone()).await.unwrap();
        s.suggest_corrections("soren og ase", "SØREN og Åse", elapsed, false)
            .await
            .unwrap();
        assert!(s.suggestions().await.unwrap().is_empty());
        s.suggest_corrections("Oystein", "Øystein", elapsed, true)
            .await
            .unwrap();
        s.suggest_corrections(
            "Oystein",
            "Øystein",
            elapsed + std::time::Duration::from_millis(1),
            false,
        )
        .await
        .unwrap();
        assert!(s.suggestions().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn accepting_suggestion_rechecks_dictionary_with_unicode_lowercase() {
        let s = store().await;
        s.suggest_corrections("Soren", "Søren", std::time::Duration::ZERO, false)
            .await
            .unwrap();
        assert_eq!(s.suggestions().await.unwrap(), vec!["Søren"]);
        let id = s.upsert_word(None, "SØREN", "soren", 1).await.unwrap();
        s.set_word_english(id, true).await.unwrap();

        s.resolve_suggestion("søren", true, 2).await.unwrap();

        let dictionary = s.dictionary().await.unwrap();
        assert_eq!(dictionary.len(), 1);
        assert_eq!(dictionary[0].id, id);
        assert_eq!(dictionary[0].word, "SØREN");
        assert_eq!(dictionary[0].aliases, "soren");
        assert!(dictionary[0].english);
        assert!(s.suggestions().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn suggestions_deduplicate_and_skip_existing_dictionary_words() {
        let s = store().await;
        s.upsert_word(None, "SØREN", "", 1).await.unwrap();
        s.suggest_corrections(
            "Soren Ase Ase",
            "Søren Åse ÅSE",
            std::time::Duration::ZERO,
            false,
        )
        .await
        .unwrap();
        assert_eq!(s.suggestions().await.unwrap(), vec!["Åse"]);
        s.resolve_suggestion("missing", true, 1).await.unwrap();
        assert_eq!(s.dictionary().await.unwrap().len(), 1);
        s.resolve_suggestion("åse", false, 1).await.unwrap();
        s.resolve_suggestion("Åse", true, 1).await.unwrap();
        assert_eq!(s.dictionary().await.unwrap().len(), 1);
        assert!(s.suggestions().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn history_migration_preserves_legacy_rows_with_unknown_provider() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::raw_sql(SCHEMA).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO dictation_history (created_at, text, style, engine, latency_ms, audio_ms) VALUES (42, 'Original', 'raw', 'local', 7, 8)")
            .execute(&pool).await.unwrap();
        let store = Store::with_pool(pool.clone()).await.unwrap();
        let entry = store.history(1).await.unwrap().remove(0);
        assert_eq!(entry.text, "Original");
        assert_eq!(entry.created_at, 42);
        assert_eq!(entry.engine, "local");
        assert_eq!(entry.latency_ms, 7);
        assert_eq!(entry.audio_ms, 8);
        assert!(entry.provider.is_none());
        assert!(entry.syv_model.is_none());
        assert!(entry.language.is_none());
        Store::with_pool(pool).await.unwrap();
    }

    #[tokio::test]
    async fn settings_default_and_roundtrip() {
        let s = store().await;
        assert_eq!(s.settings().await.unwrap(), DictationSettings::default());
        let mut changed = DictationSettings::default();
        changed.shortcut = "Ctrl+Alt+Space".into();
        changed.history_enabled = false;
        s.save_settings(&changed).await.unwrap();
        assert_eq!(s.settings().await.unwrap(), changed);
    }

    #[tokio::test]
    async fn dictionary_and_snippets_become_rules() {
        let s = store().await;
        let id = s
            .upsert_word(None, " Mahoje ", "maho je,  mahoye ,", 1)
            .await
            .unwrap();
        s.upsert_snippet(None, "mit cvr", "39413892", 1)
            .await
            .unwrap();
        assert!(s.upsert_word(None, "  ", "", 1).await.is_err());
        assert!(s.upsert_snippet(None, "x", " ", 1).await.is_err());

        let words = s.dictionary().await.unwrap();
        assert_eq!(words.len(), 1);
        assert_eq!(words[0].word, "Mahoje");
        assert_eq!(words[0].aliases, "maho je, mahoye");

        let rules = s.rules().await.unwrap();
        assert_eq!(rules.replacements.len(), 3);
        assert_eq!(rules.snippets.len(), 1);

        assert!(!rules.replacements[0].keep_english);
        s.set_word_english(id, true).await.unwrap();
        assert!(s.dictionary().await.unwrap()[0].english);
        assert!(s
            .rules()
            .await
            .unwrap()
            .replacements
            .iter()
            .all(|r| r.keep_english));

        s.upsert_word(Some(id), "Syv.ai", "", 2).await.unwrap();
        assert_eq!(s.dictionary().await.unwrap()[0].word, "Syv.ai");
        s.delete_word(id).await.unwrap();
        assert!(s.dictionary().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn english_column_is_added_to_old_databases() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::raw_sql(
            "CREATE TABLE dictation_dictionary (id INTEGER PRIMARY KEY AUTOINCREMENT, word TEXT NOT NULL,
             aliases TEXT NOT NULL DEFAULT '', created_at INTEGER NOT NULL);
             INSERT INTO dictation_dictionary (word, aliases, created_at) VALUES ('Mahoje', '', 1);",
        )
        .execute(&pool)
        .await
        .unwrap();
        let s = Store::with_pool(pool).await.unwrap();
        let words = s.dictionary().await.unwrap();
        assert_eq!(
            (words[0].word.as_str(), words[0].english),
            ("Mahoje", false)
        );
        // The additive suggestion table also exists in upgraded databases.
        s.suggest_corrections("Soren", "Søren", std::time::Duration::ZERO, false)
            .await
            .unwrap();
        assert_eq!(s.suggestions().await.unwrap(), vec!["Søren"]);
        assert_eq!(s.dictionary().await.unwrap()[0].word, "Mahoje");
        // Opening again does not add the column twice.
        Store::with_pool(s.pool.clone()).await.unwrap();
    }

    #[tokio::test]
    async fn history_is_trimmed_to_limit() {
        let s = store().await;
        for i in 0..HISTORY_LIMIT {
            let audio = if i < 3 {
                Some(format!("/a/{i}.wav"))
            } else {
                None
            };
            assert!(s
                .add_history(entry(i, audio.as_deref()))
                .await
                .unwrap()
                .is_empty());
        }
        let trimmed = s.add_history(entry(HISTORY_LIMIT, None)).await.unwrap();
        assert_eq!(trimmed, vec!["/a/0.wav".to_string()]);
        let history = s.history(1000).await.unwrap();
        assert_eq!(history.len(), HISTORY_LIMIT as usize);
        assert_eq!(history[0].created_at, HISTORY_LIMIT);
        assert_eq!(history.last().unwrap().created_at, 1);
    }

    #[tokio::test]
    async fn audio_expires_after_retention() {
        let s = store().await;
        let now = 10 * AUDIO_RETENTION_MS;
        s.add_history(entry(now - AUDIO_RETENTION_MS - 1, Some("/a/old.wav")))
            .await
            .unwrap();
        s.add_history(entry(now - 1000, Some("/a/new.wav")))
            .await
            .unwrap();
        assert_eq!(
            s.expire_audio(now).await.unwrap(),
            vec!["/a/old.wav".to_string()]
        );
        let history = s.history(10).await.unwrap();
        assert_eq!(history.len(), 2, "text is kept");
        assert!(history[0].has_audio);
        assert!(!history[1].has_audio);
    }

    #[tokio::test]
    async fn delete_and_clear_history() {
        let s = store().await;
        s.add_history(entry(1, Some("/a/1.wav"))).await.unwrap();
        s.add_history(entry(2, None)).await.unwrap();
        let id = s.history(10).await.unwrap()[1].id;
        assert_eq!(
            s.delete_history(id).await.unwrap(),
            Some("/a/1.wav".to_string())
        );
        assert_eq!(s.history(10).await.unwrap().len(), 1);
        assert!(s.clear_history().await.unwrap().is_empty());
        assert!(s.history(10).await.unwrap().is_empty());
    }
}
