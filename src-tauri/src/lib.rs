mod adapter;
mod archive;
mod core;
mod log_parser;
mod model;
mod process;
mod state_machine;

use core::{AppCore, JobSummary};
use model::{AppSnapshot, StartScanRequest};
use tauri::{AppHandle, Manager, State};

#[tauri::command]
fn get_snapshot(core: State<'_, AppCore>) -> AppSnapshot {
    core.snapshot()
}

#[tauri::command]
fn list_jobs(core: State<'_, AppCore>) -> Vec<JobSummary> {
    core.jobs()
}

#[tauri::command]
fn start_scan(
    app: AppHandle,
    core: State<'_, AppCore>,
    request: StartScanRequest,
) -> Result<String, String> {
    core.start(app, request)
}

#[tauri::command]
fn cancel_scan(app: AppHandle, core: State<'_, AppCore>) -> Result<(), String> {
    core.cancel(&app)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let archive_root = app.path().app_local_data_dir()?.join("scan-archive");
            let core = AppCore::new(archive_root);
            core.recover_archive().map_err(std::io::Error::other)?;
            app.manage(core);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            list_jobs,
            start_scan,
            cancel_scan
        ])
        .run(tauri::generate_context!())
        .expect("error while running BeBlog Digitizer");
}
