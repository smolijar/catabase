use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_updater::{Update, UpdaterExt};
use tokio::sync::{Mutex, RwLock, RwLockReadGuard};

use super::{AppError, AppState, CoreError};

const UPDATE_ENDPOINT: &str =
    "https://github.com/smolijar/catabase/releases/latest/download/latest.json";
const PROGRESS_EVENT: &str = "catabase:update-progress";

#[derive(Default)]
pub(super) struct UpdateState {
    pending: Mutex<Option<Update>>,
    workspace: RwLock<()>,
    installing: AtomicBool,
}

impl UpdateState {
    pub(super) fn workspace_operation(&self) -> Result<RwLockReadGuard<'_, ()>, CoreError> {
        self.workspace.try_read().map_err(|_| {
            CoreError::message("An application update is in progress. Wait for it to finish.")
        })
    }

    pub(super) fn is_installing(&self) -> bool {
        self.installing.load(Ordering::Acquire)
    }
}

struct Installing<'a>(&'a AtomicBool);

impl<'a> Installing<'a> {
    fn new(flag: &'a AtomicBool) -> Self {
        flag.store(true, Ordering::Release);
        Self(flag)
    }
}

impl Drop for Installing<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct UpdaterStatus {
    current_version: String,
    enabled: bool,
    unavailable_reason: Option<&'static str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct UpdateInfo {
    version: String,
    notes: Option<String>,
    published_at: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct UpdateCheck {
    current_version: String,
    update: Option<UpdateInfo>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateProgress {
    phase: &'static str,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
}

fn availability_reason(macos: bool, development: bool, configured: bool) -> Option<&'static str> {
    if !macos {
        Some("In-app updates are currently available for macOS releases only.")
    } else if development {
        Some("Development builds do not install updates. Use a packaged release.")
    } else if !configured {
        Some("This build has no update-signing key. Install a release with signed updates enabled.")
    } else {
        None
    }
}

fn configured(app: &AppHandle) -> bool {
    app.config()
        .plugins
        .0
        .get("updater")
        .and_then(|config| config.get("pubkey"))
        .and_then(serde_json::Value::as_str)
        .is_some_and(|key| !key.trim().is_empty())
}

pub(super) fn setup(app: &AppHandle) -> tauri::Result<()> {
    // No dummy public key, dynamic key download, or direct frontend updater
    // permissions. Only the guarded commands below can install an update.
    if configured(app) {
        app.plugin(tauri_plugin_updater::Builder::new().build())?;
    }
    Ok(())
}

#[tauri::command]
pub(super) fn updater_status(app: AppHandle) -> UpdaterStatus {
    let unavailable_reason = availability_reason(
        cfg!(target_os = "macos"),
        cfg!(debug_assertions),
        configured(&app),
    );
    UpdaterStatus {
        current_version: app.package_info().version.to_string(),
        enabled: unavailable_reason.is_none(),
        unavailable_reason,
    }
}

fn require_updater(app: &AppHandle) -> Result<(), CoreError> {
    if let Some(reason) = updater_status(app.clone()).unavailable_reason {
        return Err(CoreError::message(reason));
    }
    Ok(())
}

fn update_error(error: tauri_plugin_updater::Error) -> AppError {
    let message = match error {
        tauri_plugin_updater::Error::ReleaseNotFound => {
            "No signed update feed is available. Check Releases or try again later.".to_string()
        }
        other => format!("Application update failed: {other}"),
    };
    CoreError::message(message).into()
}

#[tauri::command]
pub(super) async fn check_for_update(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<UpdateCheck, AppError> {
    require_updater(&app)?;
    let mut pending = state.updates.pending.try_lock().map_err(|_| {
        CoreError::message("An update check or installation is already in progress.")
    })?;
    let update = app
        .updater_builder()
        // Keep the endpoint in native code; IPC cannot supply a URL or key.
        .endpoints(vec![
            UPDATE_ENDPOINT.parse().expect("static HTTPS update URL"),
        ])
        .map_err(update_error)?
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(update_error)?
        .check()
        .await
        .map_err(update_error)?;
    let info = update.as_ref().map(|update| UpdateInfo {
        version: update.version.clone(),
        notes: update.body.clone(),
        published_at: update
            .raw_json
            .get("pub_date")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
    });
    *pending = update;
    Ok(UpdateCheck {
        current_version: app.package_info().version.to_string(),
        update: info,
    })
}

#[tauri::command]
pub(super) async fn open_releases() -> Result<(), AppError> {
    const RELEASES: &str = "https://github.com/smolijar/catabase/releases";
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = tokio::process::Command::new("open");
        command.arg(RELEASES);
        command
    };
    #[cfg(target_os = "linux")]
    let mut command = {
        let mut command = tokio::process::Command::new("xdg-open");
        command.arg(RELEASES);
        command
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = tokio::process::Command::new("rundll32.exe");
        command.arg("url.dll,FileProtocolHandler").arg(RELEASES);
        command
    };
    let status = tokio::time::timeout(Duration::from_secs(10), command.status())
        .await
        .map_err(|_| CoreError::message("Opening Releases timed out."))??;
    if !status.success() {
        return Err(CoreError::message("Could not open Releases in your browser.").into());
    }
    Ok(())
}

#[tauri::command]
pub(super) async fn install_update(
    app: AppHandle,
    state: State<'_, AppState>,
    version: String,
) -> Result<(), AppError> {
    require_updater(&app)?;
    let pending = state.updates.pending.try_lock().map_err(|_| {
        CoreError::message("An update check or installation is already in progress.")
    })?;
    let mut update = pending
        .as_ref()
        .filter(|update| update.version == version)
        .cloned()
        .ok_or_else(|| CoreError::message("Check for this update again before installing it."))?;

    // Never wait for a query or transaction to become idle and then restart
    // unexpectedly. Fail immediately; the user can retry when work is finished.
    // Holding the exclusive gate also rejects work arriving after this check.
    let _workspace = state.updates.workspace.try_write().map_err(|_| {
        CoreError::message("Finish the running operation before installing an update.")
    })?;
    let _installing = Installing::new(&state.updates.installing);
    let transactions = state.tx.try_lock().map_err(|_| {
        CoreError::message("Transaction status is busy. Try the update again once it is idle.")
    })?;
    if transactions.is_some() {
        return Err(
            CoreError::message("Commit or roll back the transaction before updating.").into(),
        );
    }
    let operations = state.cancels.try_lock().map_err(|_| {
        CoreError::message("An operation is starting. Try the update again once it is idle.")
    })?;
    if operations.values().any(|operation| operation.started) {
        return Err(CoreError::message("Finish the running query before updating.").into());
    }
    drop(operations);
    drop(transactions);

    // The frontend has already frozen editing and awaited its draft/archiving
    // queue. It must not call this command before that persistence handshake.
    update.timeout = Some(Duration::from_secs(300));
    let mut downloaded = 0_u64;
    let mut last_progress = Instant::now();
    let _ = app.emit(
        PROGRESS_EVENT,
        UpdateProgress {
            phase: "downloading",
            downloaded_bytes: 0,
            total_bytes: None,
        },
    );
    let bytes = update
        .download(
            |chunk, total| {
                downloaded += chunk as u64;
                if last_progress.elapsed() >= Duration::from_millis(100)
                    || total == Some(downloaded)
                {
                    let _ = app.emit(
                        PROGRESS_EVENT,
                        UpdateProgress {
                            phase: "downloading",
                            downloaded_bytes: downloaded,
                            total_bytes: total,
                        },
                    );
                    last_progress = Instant::now();
                }
            },
            || {},
        )
        .await
        .map_err(update_error)?;
    // download() has verified the updater signature against the compiled-in
    // public key before any app files are replaced.
    let _ = app.emit(
        PROGRESS_EVENT,
        UpdateProgress {
            phase: "installing",
            downloaded_bytes: downloaded,
            total_bytes: Some(downloaded),
        },
    );
    tauri::async_runtime::spawn_blocking(move || update.install(bytes))
        .await
        .map_err(|_| CoreError::message("The update installation task could not finish."))?
        .map_err(update_error)?;
    let _ = app.emit(
        PROGRESS_EVENT,
        UpdateProgress {
            phase: "restarting",
            downloaded_bytes: downloaded,
            total_bytes: Some(downloaded),
        },
    );
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_configured_packaged_macos_builds_can_update() {
        assert!(availability_reason(true, false, true).is_none());
        assert!(
            availability_reason(false, false, true)
                .unwrap()
                .contains("macOS")
        );
        assert!(
            availability_reason(true, true, true)
                .unwrap()
                .contains("Development")
        );
        assert!(
            availability_reason(true, false, false)
                .unwrap()
                .contains("key")
        );
    }

    #[test]
    fn installation_refuses_inflight_work_and_blocks_late_operations() {
        let updates = UpdateState::default();
        let operation = updates.workspace_operation().unwrap();
        assert!(updates.workspace.try_write().is_err());
        drop(operation);
        let install = updates.workspace.try_write().unwrap();
        assert!(updates.workspace_operation().is_err());
        drop(install);
        assert!(updates.workspace_operation().is_ok());
    }

    #[test]
    fn failure_releases_the_native_exit_guard() {
        let updates = UpdateState::default();
        let installing = Installing::new(&updates.installing);
        assert!(updates.is_installing());
        drop(installing);
        assert!(!updates.is_installing());
    }
}
