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
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
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
        let rows = sqlx::query("SELECT id, word, aliases FROM dictation_dictionary ORDER BY word COLLATE NOCASE")
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| DictionaryWord {
                id: r.get("id"),
                word: r.get("word"),
                aliases: r.get("aliases"),
            })
            .collect())
    }

    pub async fn upsert_word(&self, id: Option<i64>, word: &str, aliases: &str, now: i64) -> Result<i64, String> {
        let word = word.trim();
        if word.is_empty() {
            return Err("Ordet må ikke være tomt".to_string());
        }
        let aliases = normalize_aliases(aliases);
        match id {
            Some(id) => {
                sqlx::query("UPDATE dictation_dictionary SET word = ?1, aliases = ?2 WHERE id = ?3")
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

    pub async fn delete_word(&self, id: i64) -> Result<(), String> {
        sqlx::query("DELETE FROM dictation_dictionary WHERE id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    // -- snippets ----------------------------------------------------------

    pub async fn snippets(&self) -> Result<Vec<SnippetRow>, String> {
        let rows = sqlx::query("SELECT id, phrase, text FROM dictation_snippets ORDER BY phrase COLLATE NOCASE")
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

    pub async fn upsert_snippet(&self, id: Option<i64>, phrase: &str, text: &str, now: i64) -> Result<i64, String> {
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
                let result = sqlx::query("INSERT INTO dictation_snippets (phrase, text, created_at) VALUES (?1, ?2, ?3)")
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
            });
            for alias in word.aliases.split(',').map(str::trim).filter(|a| !a.is_empty()) {
                replacements.push(Replacement {
                    from: alias.to_string(),
                    to: word.word.clone(),
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
        Ok(Rules { replacements, snippets })
    }

    // -- history -----------------------------------------------------------

    /// Add an entry and trim to [`HISTORY_LIMIT`]. Returns audio files of
    /// trimmed entries, which the caller deletes.
    pub async fn add_history(&self, entry: NewHistoryEntry) -> Result<Vec<String>, String> {
        sqlx::query(
            "INSERT INTO dictation_history
             (created_at, text, app, style, engine, fallback_reason, latency_ms, audio_ms, insert_method, audio_path)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
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
        .execute(&self.pool)
        .await
        .map_err(db_err)?;

        let cutoff = "SELECT id FROM dictation_history ORDER BY created_at DESC, id DESC LIMIT -1 OFFSET ?1";
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
        sqlx::query(&format!("DELETE FROM dictation_history WHERE id IN ({cutoff})"))
            .bind(HISTORY_LIMIT)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(paths)
    }

    pub async fn history(&self, limit: i64) -> Result<Vec<HistoryEntry>, String> {
        let rows = sqlx::query(
            "SELECT id, created_at, text, app, style, engine, fallback_reason, latency_ms, audio_ms,
                    insert_method, audio_path IS NOT NULL AS has_audio
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
        let path: Option<String> = sqlx::query("SELECT audio_path FROM dictation_history WHERE id = ?1")
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
        let paths = sqlx::query("SELECT audio_path FROM dictation_history WHERE audio_path IS NOT NULL")
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

fn normalize_aliases(aliases: &str) -> String {
    aliases
        .split(',')
        .map(str::trim)
        .filter(|a| !a.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
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
            fallback_reason: None,
            latency_ms: 900,
            audio_ms: 4000,
            insert_method: Some("paste".into()),
            audio_path: audio.map(str::to_string),
        }
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
        let id = s.upsert_word(None, " Mahoje ", "maho je,  mahoye ,", 1).await.unwrap();
        s.upsert_snippet(None, "mit cvr", "39413892", 1).await.unwrap();
        assert!(s.upsert_word(None, "  ", "", 1).await.is_err());
        assert!(s.upsert_snippet(None, "x", " ", 1).await.is_err());

        let words = s.dictionary().await.unwrap();
        assert_eq!(words.len(), 1);
        assert_eq!(words[0].word, "Mahoje");
        assert_eq!(words[0].aliases, "maho je, mahoye");

        let rules = s.rules().await.unwrap();
        assert_eq!(rules.replacements.len(), 3);
        assert_eq!(rules.snippets.len(), 1);

        s.upsert_word(Some(id), "Syv.ai", "", 2).await.unwrap();
        assert_eq!(s.dictionary().await.unwrap()[0].word, "Syv.ai");
        s.delete_word(id).await.unwrap();
        assert!(s.dictionary().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn history_is_trimmed_to_limit() {
        let s = store().await;
        for i in 0..HISTORY_LIMIT {
            let audio = if i < 3 { Some(format!("/a/{i}.wav")) } else { None };
            assert!(s.add_history(entry(i, audio.as_deref())).await.unwrap().is_empty());
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
        s.add_history(entry(now - AUDIO_RETENTION_MS - 1, Some("/a/old.wav"))).await.unwrap();
        s.add_history(entry(now - 1000, Some("/a/new.wav"))).await.unwrap();
        assert_eq!(s.expire_audio(now).await.unwrap(), vec!["/a/old.wav".to_string()]);
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
        assert_eq!(s.delete_history(id).await.unwrap(), Some("/a/1.wav".to_string()));
        assert_eq!(s.history(10).await.unwrap().len(), 1);
        assert!(s.clear_history().await.unwrap().is_empty());
        assert!(s.history(10).await.unwrap().is_empty());
    }
}
