use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tingskriver_core::{
    delivery::{Delivery, JobStatus, Policy, Target},
    providers::secrets::CredentialManager,
};
use tokio::sync::OnceCell;

static SERVICE: OnceCell<Delivery> = OnceCell::const_new();

async fn service(app: &AppHandle) -> Result<&'static Delivery, String> {
    SERVICE
        .get_or_try_init(|| async {
            let dir = app
                .path()
                .app_data_dir()
                .map_err(|_| "Datamappen kunne ikke findes".to_string())?;
            Delivery::open(&dir, Arc::new(CredentialManager))
                .await
                .map_err(|e| e.to_string())
        })
        .await
}

pub async fn worker(app: AppHandle) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(15));
    loop {
        interval.tick().await;
        match service(&app).await {
            Ok(service) => {
                if service.run_due().await.is_err() {
                    log::warn!("Afleveringskøen kunne ikke behandles");
                }
            }
            Err(_) => log::warn!("Afleveringskøen kunne ikke åbnes"),
        }
    }
}

#[derive(serde::Serialize)]
pub struct Status {
    policy: Policy,
    targets: Vec<Target>,
    jobs: Vec<JobStatus>,
}

#[tauri::command]
pub async fn delivery_status(app: AppHandle) -> Result<Status, String> {
    let s = service(&app).await?;
    Ok(Status {
        policy: s.policy().await.map_err(|e| e.to_string())?,
        targets: s.targets().await.map_err(|e| e.to_string())?,
        jobs: s.jobs().await.map_err(|e| e.to_string())?,
    })
}

#[tauri::command]
pub async fn delivery_save_target(
    app: AppHandle,
    target: Target,
    webhook_url: Option<String>,
) -> Result<(), String> {
    service(&app)
        .await?
        .save_target(target, webhook_url)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delivery_delete_target(app: AppHandle, id: String) -> Result<(), String> {
    service(&app)
        .await?
        .delete_target(&id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delivery_set_policy(app: AppHandle, policy: Policy) -> Result<(), String> {
    service(&app)
        .await?
        .set_policy(policy)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delivery_test(app: AppHandle, id: String) -> Result<String, String> {
    service(&app)
        .await?
        .test_target(&id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delivery_retry(app: AppHandle, id: String) -> Result<(), String> {
    service(&app)
        .await?
        .retry(&id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delivery_approve(
    app: AppHandle,
    target_id: String,
    meeting_id: String,
    summary: String,
    private: bool,
) -> Result<String, String> {
    let s = service(&app).await?;
    let host = crate::core_bridge::host(&app);
    let database = host.database().ok_or("Mødedatabasen er ikke åben")?;
    let bundle =
        tingskriver_core::delivery::meeting::snapshot(database.pool(), &meeting_id, summary)
            .await
            .map_err(|e| e.to_string())?;
    s.enqueue(&target_id, bundle, true, private)
        .await
        .map_err(|e| e.to_string())
}
