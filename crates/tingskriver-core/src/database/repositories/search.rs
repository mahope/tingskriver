//! Local, UI-independent full-text search. See `docs/soegning.md` for semantics.
use chrono::{DateTime, Utc};
use sqlx::{QueryBuilder, Sqlite, SqlitePool};

/// Snippet match delimiters. Render as text and translate these to UI highlighting.
pub const MATCH_START: &str = "\u{1e}";
pub const MATCH_END: &str = "\u{1f}";

#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(rename_all = "lowercase")]
pub enum SearchSource {
    Title,
    Note,
    Summary,
    Transcript,
}

impl SearchSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Note => "note",
            Self::Summary => "summary",
            Self::Transcript => "transcript",
        }
    }
}

/// Meeting creation time in UTC: inclusive lower bound, exclusive upper bound.
/// Results default to 50 and are capped at 500. Zero requests no results.
#[derive(Debug)]
pub struct SearchFilters {
    pub created_from: Option<DateTime<Utc>>,
    pub created_before: Option<DateTime<Utc>>,
    pub source: Option<SearchSource>,
    pub limit: u32,
}

impl Default for SearchFilters {
    fn default() -> Self {
        Self {
            created_from: None,
            created_before: None,
            source: None,
            limit: 50,
        }
    }
}

#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct SearchHit {
    pub source: SearchSource,
    pub source_id: String,
    pub meeting_id: String,
    pub snippet: String,
    /// Recording-relative seconds; absent for non-audio sources and legacy segments.
    pub time_seconds: Option<f64>,
    /// FTS5 bm25: smaller (more negative) scores rank first.
    pub rank: f64,
}

// Quote each whitespace-delimited term, treating FTS operators as literal text.
// Only a trailing star opts a term into prefix matching.
fn prepare_query(query: &str) -> String {
    query
        .split_whitespace()
        .filter_map(|term| {
            let prefix = term.ends_with('*');
            let text = term.trim_end_matches('*');
            if !text.chars().any(char::is_alphanumeric) {
                return None;
            }
            Some(format!(
                "\"{}\"{}",
                text.replace('"', "\"\""),
                if prefix { "*" } else { "" }
            ))
        })
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// Search literal terms (AND), allowing `ord*` prefixes. No raw FTS syntax is
/// executed. Order is bm25, meeting id, source, source id, all ascending.
pub async fn search(
    pool: &SqlitePool,
    query: &str,
    filters: &SearchFilters,
) -> Result<Vec<SearchHit>, sqlx::Error> {
    let query = prepare_query(query);
    if query.is_empty() || filters.limit == 0 {
        return Ok(Vec::new());
    }
    let mut sql = QueryBuilder::<Sqlite>::new(
        "SELECT d.source, d.source_id, d.meeting_id, d.time_seconds,
                bm25(search_fts) AS rank, snippet(search_fts, 0, ",
    );
    sql.push_bind(MATCH_START)
        .push(", ")
        .push_bind(MATCH_END)
        .push(
            ", '…', 32) AS snippet
               FROM search_fts JOIN search_documents d ON d.id = search_fts.rowid",
        );
    if filters.created_from.is_some() || filters.created_before.is_some() {
        sql.push(" JOIN meetings m ON m.id = d.meeting_id");
    }
    sql.push(" WHERE search_fts MATCH ").push_bind(query);
    if let Some(source) = filters.source {
        sql.push(" AND d.source = ").push_bind(source.as_str());
    }
    if let Some(from) = filters.created_from {
        sql.push(" AND julianday(m.created_at) >= julianday(")
            .push_bind(from.to_rfc3339())
            .push(")");
    }
    if let Some(before) = filters.created_before {
        sql.push(" AND julianday(m.created_at) < julianday(")
            .push_bind(before.to_rfc3339())
            .push(")");
    }
    sql.push(" ORDER BY rank, d.meeting_id, d.source, d.source_id LIMIT ")
        .push_bind(filters.limit.min(500));
    sql.build_query_as::<SearchHit>().fetch_all(pool).await
}

/// Atomically replace the derived index from authoritative tables. Readers see
/// either the old or the complete new index; source data is never modified.
pub async fn rebuild(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let mut transaction = pool.begin().await?;
    sqlx::query("DELETE FROM search_documents")
        .execute(&mut *transaction)
        .await?;
    sqlx::query(
        "INSERT INTO search_documents(body, source, source_id, meeting_id, time_seconds)
         SELECT body, source, source_id, meeting_id, time_seconds FROM search_source_documents",
    )
    .execute(&mut *transaction)
    .await?;
    sqlx::query("INSERT INTO search_fts(search_fts) VALUES ('rebuild')")
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await
}
