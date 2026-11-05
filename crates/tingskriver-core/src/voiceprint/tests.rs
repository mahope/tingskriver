//! Lifecycle tests with a fake secret store and a controllable clock.

use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, TimeZone, Utc};
use sqlx::SqlitePool;

use super::embedder::StubEmbedder;
use super::service::*;
use super::store::{self, MemoryBackend, SecretBackend};
use super::*;

struct TestClock(Mutex<DateTime<Utc>>);

impl TestClock {
    fn advance_days(&self, days: i64) {
        *self.0.lock().unwrap() += Duration::days(days);
    }
}

impl Clock for TestClock {
    fn now(&self) -> DateTime<Utc> {
        *self.0.lock().unwrap()
    }
}

struct Fixture {
    service: VoiceprintService,
    backend: Arc<MemoryBackend>,
    clock: Arc<TestClock>,
    pool: SqlitePool,
}

async fn fixture() -> Fixture {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    let backend = Arc::new(MemoryBackend::default());
    let clock = Arc::new(TestClock(Mutex::new(
        Utc.with_ymd_and_hms(2026, 11, 5, 12, 0, 0).unwrap(),
    )));
    let service = VoiceprintService::new(pool.clone(), backend.clone(), clock.clone());
    Fixture {
        service,
        backend,
        clock,
        pool,
    }
}

fn voice(values: &[f32]) -> Embedding {
    Embedding::new(values.to_vec()).unwrap()
}

fn consent(name: &str) -> ConsentInput {
    ConsentInput {
        person_name: name.to_string(),
        person_informed_and_agreed: true,
        note: None,
    }
}

async fn enabled() -> Fixture {
    let f = fixture().await;
    f.service.set_enabled(true).await.unwrap();
    f
}

#[tokio::test]
async fn off_by_default_and_nothing_works_while_off() {
    let f = fixture().await;
    let settings = f.service.settings().await.unwrap();
    assert!(!settings.enabled);
    assert_eq!(settings.retention_days, 90);
    assert_eq!(Settings::default(), settings);
    assert_eq!(
        f.service.grant_consent(consent("Anna Test")).await,
        Err(VoiceprintError::Disabled)
    );
    assert_eq!(
        f.service.suggest(&[]).await.unwrap_err(),
        VoiceprintError::Disabled
    );
    assert!(f.backend.targets().is_empty());
}

#[tokio::test]
async fn enrolling_without_active_consent_stores_nothing() {
    let f = enabled().await;
    let print = voice(&[1.0, 0.0, 0.0]);
    // Unknown person.
    assert_eq!(
        f.service.enroll("findes-ikke", &print).await,
        Err(VoiceprintError::NoConsent)
    );
    // Unconfirmed consent is refused.
    let mut input = consent("Anna Test");
    input.person_informed_and_agreed = false;
    assert_eq!(
        f.service.grant_consent(input).await,
        Err(VoiceprintError::NotConfirmed)
    );
    assert_eq!(
        f.service.grant_consent(consent("  ")).await,
        Err(VoiceprintError::NameRequired)
    );
    // Withdrawn consent is refused.
    let id = f.service.grant_consent(consent("Anna Test")).await.unwrap();
    f.service.withdraw_consent(&id).await.unwrap();
    assert_eq!(
        f.service.enroll(&id, &print).await,
        Err(VoiceprintError::NoConsent)
    );
    assert!(f.backend.targets().is_empty());
    // With active consent it is stored, in the secret store only.
    let id = f.service.grant_consent(consent("Bo Test")).await.unwrap();
    f.service.enroll(&id, &print).await.unwrap();
    assert_eq!(f.backend.targets().len(), 1);
}

#[tokio::test]
async fn consent_record_has_the_required_fields() {
    let f = enabled().await;
    let mut input = consent(" Anna Test ");
    input.note = Some(" sagde ja på mødet ".to_string());
    let id = f.service.grant_consent(input).await.unwrap();
    let records = f.service.consents().await.unwrap();
    assert_eq!(records.len(), 1);
    let r = &records[0];
    assert_eq!(r.id, id);
    assert_eq!(r.person_name, "Anna Test");
    assert_eq!(r.method, METHOD_CONFIRMED_IN_APP);
    assert_eq!(r.note.as_deref(), Some("sagde ja på mødet"));
    assert_eq!(r.text_version, texts::CONSENT_TEXT_VERSION);
    assert_eq!(r.consented_at, f.clock.now());
    assert!(r.active && r.withdrawn_at.is_none());
}

#[tokio::test]
async fn withdrawing_consent_deletes_the_voiceprints_at_once_and_logs_it() {
    let f = enabled().await;
    let anna = f.service.grant_consent(consent("Anna Test")).await.unwrap();
    let bo = f.service.grant_consent(consent("Bo Test")).await.unwrap();
    f.service.enroll(&anna, &voice(&[1.0, 0.0])).await.unwrap();
    f.service.enroll(&anna, &voice(&[0.9, 0.1])).await.unwrap();
    f.service.enroll(&bo, &voice(&[0.0, 1.0])).await.unwrap();
    f.clock.advance_days(1);
    f.service.withdraw_consent(&anna).await.unwrap();
    // Only Bo's voiceprint is left.
    assert_eq!(f.backend.targets().len(), 1);
    let records = f.service.consents().await.unwrap();
    let anna_record = records.iter().find(|r| r.id == anna).unwrap();
    assert!(!anna_record.active);
    assert_eq!(anna_record.exemplars, 0);
    assert_eq!(anna_record.withdrawn_at, Some(f.clock.now()));
    let events: Vec<(String,)> = sqlx::query_as(
        "SELECT event FROM voiceprint_consent_log WHERE consent_id = $1 ORDER BY id",
    )
    .bind(&anna)
    .fetch_all(&f.pool)
    .await
    .unwrap();
    assert_eq!(
        events,
        vec![("granted".to_string(),), ("withdrawn".to_string(),)]
    );
    // Withdrawing twice does not add a second record.
    f.service.withdraw_consent(&anna).await.unwrap();
    let (n,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM voiceprint_consent_log WHERE consent_id = $1")
            .bind(&anna)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(n, 2);
}

#[tokio::test]
async fn failed_delete_is_reported_and_can_be_retried() {
    let f = enabled().await;
    let anna = f.service.grant_consent(consent("Anna Test")).await.unwrap();
    f.service.enroll(&anna, &voice(&[1.0, 0.0])).await.unwrap();
    f.backend.set_fail_deletes(true);
    assert_eq!(
        f.service.withdraw_consent(&anna).await,
        Err(VoiceprintError::Storage)
    );
    // Consent is already withdrawn, so nothing new can be stored, and the
    // index row is kept so the deletion is not forgotten.
    assert_eq!(
        f.service.enroll(&anna, &voice(&[1.0, 1.0])).await,
        Err(VoiceprintError::NoConsent)
    );
    assert_eq!(f.backend.targets().len(), 1);
    f.backend.set_fail_deletes(false);
    f.service.withdraw_consent(&anna).await.unwrap();
    assert!(f.backend.targets().is_empty());
}

#[tokio::test]
async fn forget_person_and_forget_all_remove_everything_including_leftovers() {
    let f = enabled().await;
    let anna = f.service.grant_consent(consent("Anna Test")).await.unwrap();
    let bo = f.service.grant_consent(consent("Bo Test")).await.unwrap();
    f.service.enroll(&anna, &voice(&[1.0, 0.0])).await.unwrap();
    f.service.enroll(&bo, &voice(&[0.0, 1.0])).await.unwrap();
    assert_eq!(f.service.forget_person(&anna).await.unwrap(), 1);
    assert_eq!(f.backend.targets().len(), 1);
    // The consent stays; forgetting is not withdrawing.
    assert!(
        f.service
            .consents()
            .await
            .unwrap()
            .iter()
            .find(|r| r.id == anna)
            .unwrap()
            .active
    );
    // A leftover the index does not know, and an unrelated credential.
    f.backend
        .plant(&store::target_for("faldet-ud-af-indekset"), "x");
    f.backend.plant("Tingskriver/syv.ai", "ikke-mit");
    f.service.forget_all().await.unwrap();
    assert_eq!(f.backend.targets(), vec!["Tingskriver/syv.ai".to_string()]);
    assert!(f
        .service
        .consents()
        .await
        .unwrap()
        .iter()
        .all(|r| r.exemplars == 0));
}

#[tokio::test]
async fn keeps_at_most_three_voiceprints_per_person() {
    let f = enabled().await;
    let anna = f.service.grant_consent(consent("Anna Test")).await.unwrap();
    for i in 1..=5 {
        f.clock.advance_days(1);
        f.service
            .enroll(&anna, &voice(&[i as f32, 1.0]))
            .await
            .unwrap();
    }
    assert_eq!(f.backend.targets().len(), MAX_EXEMPLARS_PER_PERSON);
}

#[tokio::test]
async fn automatic_deletion_follows_the_setting_with_a_controllable_clock() {
    for (days, kept_after, gone_after) in [(30u32, 29i64, 31i64), (90, 89, 91), (180, 179, 181)] {
        let f = enabled().await;
        f.service.set_retention_days(days).await.unwrap();
        let anna = f.service.grant_consent(consent("Anna Test")).await.unwrap();
        f.service.enroll(&anna, &voice(&[1.0, 0.0])).await.unwrap();
        f.clock.advance_days(kept_after);
        assert_eq!(
            f.service.purge_expired().await.unwrap(),
            0,
            "kept at {kept_after} days"
        );
        assert_eq!(f.backend.targets().len(), 1);
        f.clock.advance_days(gone_after - kept_after);
        assert_eq!(
            f.service.purge_expired().await.unwrap(),
            1,
            "gone at {gone_after} days"
        );
        assert!(f.backend.targets().is_empty());
    }
}

#[tokio::test]
async fn default_retention_is_90_days_and_never_is_not_offered() {
    let f = enabled().await;
    assert_eq!(f.service.settings().await.unwrap().retention_days, 90);
    for bad in [0, 1, 45, 365, u32::MAX] {
        assert_eq!(
            f.service.set_retention_days(bad).await,
            Err(VoiceprintError::InvalidRetention)
        );
    }
    assert_eq!(RETENTION_CHOICES, [30, 90, 180]);
}

#[tokio::test]
async fn a_confirmed_match_postpones_the_deletion() {
    let f = enabled().await;
    let anna = f.service.grant_consent(consent("Anna Test")).await.unwrap();
    f.service
        .enroll(&anna, &voice(&[1.0, 0.0, 0.0]))
        .await
        .unwrap();
    f.clock.advance_days(80);
    let speakers = vec![("Taler 1".to_string(), voice(&[0.98, 0.05, 0.0]))];
    let suggestions = f.service.suggest(&speakers).await.unwrap();
    assert_eq!(suggestions.len(), 1);
    // A suggestion alone does not count; only the user's confirmation does.
    f.clock.advance_days(15);
    assert_eq!(f.service.purge_expired().await.unwrap(), 1);

    let f = enabled().await;
    let anna = f.service.grant_consent(consent("Anna Test")).await.unwrap();
    f.service
        .enroll(&anna, &voice(&[1.0, 0.0, 0.0]))
        .await
        .unwrap();
    f.clock.advance_days(80);
    let suggestions = f.service.suggest(&speakers).await.unwrap();
    let name = f
        .service
        .confirm_suggestion(&suggestions[0].suggestion)
        .await
        .unwrap();
    assert_eq!(name, "Anna Test");
    f.clock.advance_days(15);
    assert_eq!(f.service.purge_expired().await.unwrap(), 0);
    f.clock.advance_days(80);
    assert_eq!(f.service.purge_expired().await.unwrap(), 1);
}

#[tokio::test]
async fn purge_runs_at_most_daily_and_also_while_off() {
    let f = enabled().await;
    let anna = f.service.grant_consent(consent("Anna Test")).await.unwrap();
    f.service.enroll(&anna, &voice(&[1.0, 0.0])).await.unwrap();
    assert_eq!(
        f.service.purge_if_due().await.unwrap(),
        Some(0),
        "first run at startup"
    );
    assert_eq!(
        f.service.purge_if_due().await.unwrap(),
        None,
        "not due again"
    );
    f.service.set_enabled(false).await.unwrap();
    f.clock.advance_days(91);
    assert_eq!(
        f.service.purge_if_due().await.unwrap(),
        Some(1),
        "expired data goes even while off"
    );
}

#[tokio::test]
async fn suggestions_only_use_active_consent_and_name_nobody_by_themselves() {
    let f = enabled().await;
    let anna = f.service.grant_consent(consent("Anna Test")).await.unwrap();
    f.service
        .enroll(&anna, &voice(&[1.0, 0.0, 0.0]))
        .await
        .unwrap();
    let speakers = vec![("Taler 1".to_string(), voice(&[1.0, 0.02, 0.0]))];
    let got = f.service.suggest(&speakers).await.unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].person_name, "Anna Test");
    f.service.withdraw_consent(&anna).await.unwrap();
    assert!(f.service.suggest(&speakers).await.unwrap().is_empty());
    // Confirming a stale suggestion of a withdrawn person is refused.
    assert_eq!(
        f.service.confirm_suggestion(&got[0].suggestion).await,
        Err(VoiceprintError::NoConsent)
    );
}

#[tokio::test]
async fn embedder_trait_works_with_the_marked_stub() {
    let f = enabled().await;
    let embedder: &dyn SpeakerEmbedder = &StubEmbedder;
    let audio: Vec<f32> = (0..1600).map(|i| ((i * 7) % 13) as f32 / 13.0).collect();
    let print = embedder.embed(&audio, 16_000).unwrap();
    let anna = f.service.grant_consent(consent("Anna Test")).await.unwrap();
    f.service.enroll(&anna, &print).await.unwrap();
    let got = f
        .service
        .suggest(&[("Taler 1".to_string(), print)])
        .await
        .unwrap();
    assert_eq!(got.len(), 1);
    assert!(embedder.embed(&[], 16_000).is_err());
}

#[tokio::test]
async fn the_database_never_holds_biometric_data() {
    let f = enabled().await;
    let anna = f.service.grant_consent(consent("Anna Test")).await.unwrap();
    let print = voice(&[0.123, 0.456, 0.789, 0.321]);
    f.service.enroll(&anna, &print).await.unwrap();
    let encoded = store::encode(&print);
    // No table column is a blob or is named like a voiceprint.
    let tables: Vec<(String,)> = sqlx::query_as(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name LIKE 'voiceprint_%'",
    )
    .fetch_all(&f.pool)
    .await
    .unwrap();
    assert_eq!(tables.len(), 4);
    for (table,) in tables {
        let columns: Vec<(String, String)> = sqlx::query_as(&format!(
            "SELECT name, type FROM pragma_table_info('{table}')"
        ))
        .fetch_all(&f.pool)
        .await
        .unwrap();
        for (name, kind) in columns {
            assert!(
                !kind.eq_ignore_ascii_case("blob"),
                "{table}.{name} is a blob"
            );
            for bad in ["embedding", "vector", "print", "audio", "sample"] {
                assert!(!name.contains(bad), "{table}.{name} looks biometric");
            }
        }
        // And no value in any row contains the stored voiceprint.
        let dump = dump_rows(&f.pool, &table).await;
        // The dump must really read rows, or the check proves nothing.
        if table != "voiceprint_settings" {
            assert!(!dump.is_empty(), "{table} dump is empty");
        }
        for row in dump {
            assert!(!row.contains(&encoded));
        }
    }
    // Positive control: the same dump does find a voiceprint if one is put in.
    sqlx::query("UPDATE voiceprint_consents SET note = $1")
        .bind(&encoded)
        .execute(&f.pool)
        .await
        .unwrap();
    let rows = dump_rows(&f.pool, "voiceprint_consents").await;
    assert!(rows.iter().any(|r| r.contains(&encoded)));
}

/// Every row of `table` as one text line (all columns cast to text).
async fn dump_rows(pool: &SqlitePool, table: &str) -> Vec<String> {
    let columns: Vec<(String,)> =
        sqlx::query_as(&format!("SELECT name FROM pragma_table_info('{table}')"))
            .fetch_all(pool)
            .await
            .unwrap();
    let expr = columns
        .iter()
        .map(|(c,)| format!("COALESCE(CAST({c} AS TEXT), '')"))
        .collect::<Vec<_>>()
        .join(" || '|' || ");
    let rows: Vec<(String,)> = sqlx::query_as(&format!("SELECT {expr} FROM {table}"))
        .fetch_all(pool)
        .await
        .unwrap();
    rows.into_iter().map(|(r,)| r).collect()
}
