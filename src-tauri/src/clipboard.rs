use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::{AppError, CoreError};

#[tauri::command]
pub(super) async fn clipboard_read_text(app: AppHandle) -> Result<String, AppError> {
    on_clipboard_thread(app, |app| {
        app.clipboard()
            .read_text()
            .map_err(|error| CoreError::message(error.to_string()))
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
pub(super) async fn clipboard_write_text(app: AppHandle, text: String) -> Result<(), AppError> {
    on_clipboard_thread(app, move |app| {
        app.clipboard()
            .write_text(text)
            .map_err(|error| CoreError::message(error.to_string()))
    })
    .await
    .map_err(Into::into)
}

async fn on_clipboard_thread<T: Send + 'static>(
    app: AppHandle,
    operation: impl FnOnce(&AppHandle) -> Result<T, CoreError> + Send + 'static,
) -> Result<T, CoreError> {
    #[cfg(target_os = "macos")]
    {
        // WebKit also accesses the general NSPasteboard on the main thread.
        // The plugin mutex only guards its own calls, so a Tokio-thread write
        // can race WebKit and crash AppKit's pasteboard ownership handling.
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let clipboard_app = app.clone();
        app.run_on_main_thread(move || {
            let _ = sender.send(operation(&clipboard_app));
        })
        .map_err(|error| CoreError::message(error.to_string()))?;
        receiver
            .await
            .map_err(|_| CoreError::message("Clipboard operation did not complete."))?
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Linux clipboard reads must stay off the UI thread to avoid deadlocks.
        operation(&app)
    }
}
