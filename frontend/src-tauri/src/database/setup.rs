use std::sync::RwLock;

use log::{error, info};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tingskriver_core::database::init::DbInitError;

use super::manager::DatabaseManager;
use crate::state::AppState;

/// Event with the new [`DatabaseStatus`] whenever it changes.
pub const DATABASE_STATUS_EVENT: &str = "database-status";

/// Where the database is at startup, for the loading and error pages.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum DatabaseStatus {
    /// Opening in the background; the window shows a loading state.
    Loading,
    /// No database yet: onboarding creates it.
    FirstLaunch,
    Ready,
    /// The database did not open. The window shows the error page.
    Failed {
        error: DbInitError,
        summary: String,
        advice: String,
    },
    /// Something other than opening the database failed (no app data folder).
    Error {
        message: String,
    },
}

/// The current [`DatabaseStatus`], managed before the window opens so the
/// frontend can always ask for it.
#[derive(Default)]
pub struct DatabaseStatusState(RwLock<Option<DatabaseStatus>>);

impl DatabaseStatusState {
    pub fn get(&self) -> DatabaseStatus {
        self.0
            .read()
            .ok()
            .and_then(|status| status.clone())
            .unwrap_or(DatabaseStatus::Loading)
    }

    fn set(&self, status: DatabaseStatus) {
        if let Ok(mut current) = self.0.write() {
            *current = Some(status);
        }
    }
}

fn publish(app: &AppHandle, status: DatabaseStatus) {
    if let Some(state) = app.try_state::<DatabaseStatusState>() {
        state.set(status.clone());
    }
    if let Err(e) = app.emit(DATABASE_STATUS_EVENT, status) {
        error!("Failed to emit {}: {}", DATABASE_STATUS_EVENT, e);
    }
}

/// Open the database in the background after the window is shown. Never
/// panics: a database that does not open becomes [`DatabaseStatus::Failed`]
/// and the frontend shows the error page instead of the app exiting.
pub fn start_database_initialization(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let status = match initialize_database_on_startup(&app).await {
            Ok(status) => status,
            Err(message) => {
                error!("Database initialization failed: {}", message);
                DatabaseStatus::Error { message }
            }
        };
        tingskriver_core::logging::startup_phase("DB ready");
        publish(&app, status);
    });
}

/// Initialize database on app startup
/// Handles first launch detection and conditional initialization
pub async fn initialize_database_on_startup(app: &AppHandle) -> Result<DatabaseStatus, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Could not determine the app data folder: {}", e))?;

    // Check if this is the first launch (no database exists yet)
    let is_first_launch = DatabaseManager::is_first_launch(&app_data_dir)
        .await
        .map_err(|e| format!("Failed to check first launch status: {}", e))?;

    if is_first_launch {
        info!("First launch detected - will notify window when ready");

        // Delay event emission to ensure window is ready and React listeners are registered
        let app_handle = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            match app_handle.emit("first-launch-detected", ()) {
                Ok(()) => info!("Emitted first-launch-detected after delay"),
                Err(e) => error!("Failed to emit first-launch-detected: {}", e),
            }
        });
        return Ok(DatabaseStatus::FirstLaunch);
    }

    match DatabaseManager::open(&app_data_dir).await {
        Ok(db_manager) => {
            app.manage(AppState { db_manager });
            info!("Database initialized successfully");
            Ok(DatabaseStatus::Ready)
        }
        Err(error) => Ok(DatabaseStatus::Failed {
            summary: error.summary_da().to_string(),
            advice: error.advice_da().to_string(),
            error,
        }),
    }
}

/// The database status for the frontend's loading and error pages.
#[tauri::command]
pub fn get_database_status(state: tauri::State<'_, DatabaseStatusState>) -> DatabaseStatus {
    state.get()
}

/// Open the folder with the copy of the database that failed to open.
#[tauri::command]
pub fn open_database_backup_folder(
    state: tauri::State<'_, DatabaseStatusState>,
) -> Result<(), String> {
    let folder = match state.get() {
        DatabaseStatus::Failed { error, .. } => error
            .backup_path()
            .map(|p| p.to_path_buf())
            .or_else(|| error.database_path().parent().map(|p| p.to_path_buf())),
        _ => None,
    }
    .ok_or("Der er ingen kopi af databasen")?;
    crate::utils::open_folder(&folder)
}
