//! Dictation timings in `dictation.sqlite` (table `dictation_timings`).
//!
//! Only numbers and the engine name are stored, never text or the target
//! program, so the table can be kept when history is turned off and read by
//! `scripts/maal-ydeevne.ps1`.

use serde::Serialize;
use sqlx::{Row, SqlitePool};

use super::logic::timing::{self, ProviderStats, Sample, Stat, Steps};

/// How many timing rows are kept.
pub const TIMINGS_LIMIT: i64 = 2000;

/// Emitted when the median of a provider goes above the target.
pub const LATENCY_WARNING_EVENT: &str = "dictation-latency-warning";

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS dictation_timings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at INTEGER NOT NULL,
    provider TEXT NOT NULL,
    fallback INTEGER NOT NULL DEFAULT 0,
    inserted INTEGER NOT NULL DEFAULT 1,
    start_ms INTEGER,
    audio_ms INTEGER NOT NULL,
    capture_ms INTEGER NOT NULL,
    transcribe_ms INTEGER NOT NULL,
    polish_ms INTEGER,
    insert_ms INTEGER NOT NULL,
    total_ms INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_dictation_timings_created ON dictation_timings(created_at);
"#;

/// One dictation to record.
#[derive(Clone, Debug)]
pub struct NewTiming {
    pub created_at: i64,
    /// "syv" or "local" (`Engine::as_str`).
    pub provider: &'static str,
    /// The primary engine failed or was too slow.
    pub fallback: bool,
    /// The text reached the program. Failed insertions are kept but not
    /// counted in the statistics.
    pub inserted: bool,
    pub steps: Steps,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatMs {
    pub median_ms: u64,
    pub p95_ms: u64,
}

impl From<Stat> for StatMs {
    fn from(s: Stat) -> Self {
        StatMs {
            median_ms: s.median_ms,
            p95_ms: s.p95_ms,
        }
    }
}

/// Statistics for one provider, for the debug page.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderLatency {
    pub provider: String,
    pub count: usize,
    pub total: StatMs,
    pub start: Option<StatMs>,
    pub capture: StatMs,
    pub transcribe: StatMs,
    pub polish: Option<StatMs>,
    pub insert: StatMs,
    pub over_target: bool,
}

impl From<&ProviderStats> for ProviderLatency {
    fn from(s: &ProviderStats) -> Self {
        ProviderLatency {
            provider: s.provider.clone(),
            count: s.count,
            total: s.total.into(),
            start: s.start.map(Into::into),
            capture: s.capture.into(),
            transcribe: s.transcribe.into(),
            polish: s.polish.map(Into::into),
            insert: s.insert.into(),
            over_target: s.over_target,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatencyReport {
    pub target_ms: u64,
    pub window: usize,
    pub providers: Vec<ProviderLatency>,
}

impl LatencyReport {
    pub fn from_stats(stats: &[ProviderStats]) -> Self {
        LatencyReport {
            target_ms: timing::TARGET_MS,
            window: timing::WINDOW,
            providers: stats.iter().map(Into::into).collect(),
        }
    }
}

/// Payload of [`LATENCY_WARNING_EVENT`].
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatencyWarning {
    pub provider: String,
    pub median_ms: u64,
    pub p95_ms: u64,
    pub count: usize,
    pub target_ms: u64,
}

impl From<&ProviderStats> for LatencyWarning {
    fn from(s: &ProviderStats) -> Self {
        LatencyWarning {
            provider: s.provider.clone(),
            median_ms: s.total.median_ms,
            p95_ms: s.total.p95_ms,
            count: s.count,
            target_ms: timing::TARGET_MS,
        }
    }
}

fn db_err(e: sqlx::Error) -> String {
    format!("Databasefejl (dikteringstider): {e}")
}

fn ms(value: Option<u64>) -> Option<i64> {
    value.map(|v| v.min(i64::MAX as u64) as i64)
}

fn get_ms(row: &sqlx::sqlite::SqliteRow, column: &str) -> u64 {
    row.get::<i64, _>(column).max(0) as u64
}

fn get_opt_ms(row: &sqlx::sqlite::SqliteRow, column: &str) -> Option<u64> {
    row.get::<Option<i64>, _>(column).map(|v| v.max(0) as u64)
}

#[derive(Clone)]
pub struct Timings {
    pool: SqlitePool,
}

impl Timings {
    pub async fn open(pool: SqlitePool) -> Result<Timings, String> {
        sqlx::raw_sql(SCHEMA).execute(&pool).await.map_err(db_err)?;
        Ok(Timings { pool })
    }

    /// Add a row and trim to [`TIMINGS_LIMIT`].
    pub async fn add(&self, t: &NewTiming) -> Result<(), String> {
        sqlx::query(
            "INSERT INTO dictation_timings
             (created_at, provider, fallback, inserted, start_ms, audio_ms, capture_ms, transcribe_ms,
              polish_ms, insert_ms, total_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        )
        .bind(t.created_at)
        .bind(t.provider)
        .bind(t.fallback as i64)
        .bind(t.inserted as i64)
        .bind(ms(t.steps.start_ms))
        .bind(ms(Some(t.steps.audio_ms)))
        .bind(ms(Some(t.steps.capture_ms)))
        .bind(ms(Some(t.steps.transcribe_ms)))
        .bind(ms(t.steps.polish_ms))
        .bind(ms(Some(t.steps.insert_ms)))
        .bind(ms(Some(t.steps.total_ms)))
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        sqlx::query(
            "DELETE FROM dictation_timings WHERE id IN
             (SELECT id FROM dictation_timings ORDER BY created_at DESC, id DESC LIMIT -1 OFFSET ?1)",
        )
        .bind(TIMINGS_LIMIT)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }

    /// The newest `per_provider` inserted dictations of each provider,
    /// newest first.
    pub async fn recent(&self, per_provider: i64) -> Result<Vec<Sample>, String> {
        let rows = sqlx::query(
            "SELECT provider, start_ms, audio_ms, capture_ms, transcribe_ms, polish_ms, insert_ms, total_ms
             FROM (SELECT *, ROW_NUMBER() OVER (PARTITION BY provider ORDER BY created_at DESC, id DESC) AS n
                   FROM dictation_timings WHERE inserted = 1)
             WHERE n <= ?1 ORDER BY created_at DESC, id DESC",
        )
        .bind(per_provider)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows
            .iter()
            .map(|row| Sample {
                provider: row.get("provider"),
                steps: Steps {
                    start_ms: get_opt_ms(row, "start_ms"),
                    audio_ms: get_ms(row, "audio_ms"),
                    capture_ms: get_ms(row, "capture_ms"),
                    transcribe_ms: get_ms(row, "transcribe_ms"),
                    polish_ms: get_opt_ms(row, "polish_ms"),
                    insert_ms: get_ms(row, "insert_ms"),
                    total_ms: get_ms(row, "total_ms"),
                },
            })
            .collect())
    }

    /// Median and p95 per provider over the newest dictations.
    pub async fn stats(&self) -> Result<Vec<ProviderStats>, String> {
        let samples = self.recent(timing::WINDOW as i64).await?;
        Ok(timing::summarize(&samples))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn timings() -> Timings {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        Timings::open(pool).await.unwrap()
    }

    fn timing(created_at: i64, provider: &'static str, total_ms: u64, inserted: bool) -> NewTiming {
        NewTiming {
            created_at,
            provider,
            fallback: false,
            inserted,
            steps: Steps {
                start_ms: Some(35),
                audio_ms: 15_000,
                capture_ms: 12,
                transcribe_ms: total_ms - 150,
                polish_ms: None,
                insert_ms: 138,
                total_ms,
            },
        }
    }

    /// The table must never get a column that could hold dictated text, the
    /// program name or a fallback message.
    #[tokio::test]
    async fn schema_has_no_text_fields() {
        let t = timings().await;
        let columns: Vec<(String, String)> = sqlx::query("PRAGMA table_info(dictation_timings)")
            .fetch_all(&t.pool)
            .await
            .unwrap()
            .iter()
            .map(|row| (row.get::<String, _>("name"), row.get::<String, _>("type")))
            .collect();
        let text_columns: Vec<&str> = columns
            .iter()
            .filter(|(_, ty)| ty != "INTEGER")
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(text_columns, vec!["provider"]);
        let names: Vec<&str> = columns.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "id",
                "created_at",
                "provider",
                "fallback",
                "inserted",
                "start_ms",
                "audio_ms",
                "capture_ms",
                "transcribe_ms",
                "polish_ms",
                "insert_ms",
                "total_ms"
            ]
        );
    }

    #[tokio::test]
    async fn rows_roundtrip_and_failed_inserts_are_not_counted() {
        let t = timings().await;
        for i in 0..5 {
            t.add(&timing(i, "syv", 900 + i as u64, true))
                .await
                .unwrap();
        }
        t.add(&timing(10, "syv", 9000, false)).await.unwrap();
        let recent = t.recent(10).await.unwrap();
        assert_eq!(recent.len(), 5);
        assert_eq!(recent[0].steps.total_ms, 904);
        assert_eq!(recent[0].steps.start_ms, Some(35));
        assert_eq!(recent[0].steps.polish_ms, None);
        let stats = t.stats().await.unwrap();
        assert_eq!(stats[0].total.median_ms, 902);
        assert!(!stats[0].over_target);
    }

    #[tokio::test]
    async fn slow_provider_produces_a_warning_payload() {
        let t = timings().await;
        for i in 0..6 {
            t.add(&timing(i, "local", 1800, true)).await.unwrap();
            t.add(&timing(i, "syv", 700, true)).await.unwrap();
        }
        let stats = t.stats().await.unwrap();
        let mut warned = Default::default();
        let warnings: Vec<LatencyWarning> = timing::new_warnings(&stats, &mut warned)
            .into_iter()
            .map(Into::into)
            .collect();
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].provider, "local");
        assert_eq!(warnings[0].median_ms, 1800);
        let json = serde_json::to_value(&warnings[0]).unwrap();
        assert_eq!(json["targetMs"], 1500);
        let report = serde_json::to_value(LatencyReport::from_stats(&stats)).unwrap();
        assert_eq!(report["providers"][0]["overTarget"], true);
        assert_eq!(report["providers"][1]["transcribe"]["medianMs"], 550);
    }

    #[tokio::test]
    async fn each_provider_gets_a_full_window() {
        let t = timings().await;
        let window = timing::WINDOW as i64;
        for i in 0..window {
            t.add(&timing(i, "local", 1200, true)).await.unwrap();
        }
        for i in 0..(window * 4) {
            t.add(&timing(window + i, "syv", 700, true)).await.unwrap();
        }
        let stats = t.stats().await.unwrap();
        let count = |p: &str| stats.iter().find(|s| s.provider == p).map(|s| s.count);
        assert_eq!(count("local"), Some(timing::WINDOW));
        assert_eq!(count("syv"), Some(timing::WINDOW));
    }

    #[tokio::test]
    async fn table_is_trimmed() {
        let t = timings().await;
        for i in 0..(TIMINGS_LIMIT + 3) {
            t.add(&timing(i, "syv", 800, true)).await.unwrap();
        }
        let count: i64 = sqlx::query("SELECT COUNT(*) AS n FROM dictation_timings")
            .fetch_one(&t.pool)
            .await
            .unwrap()
            .get("n");
        assert_eq!(count, TIMINGS_LIMIT);
    }
}
