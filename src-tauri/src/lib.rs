mod adapter;
pub mod archive;
mod core;
mod guided_protocol;
#[cfg(test)]
mod hardware_test_prep;
mod log_parser;
mod model;
pub mod offline_import;
mod process;
mod sidecar;
mod state_machine;

use core::{AppCore, JobSummary};
use model::{AppSnapshot, StartBatchRequest, StartScanRequest};
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

#[tauri::command]
fn start_batch(
    app: AppHandle,
    core: State<'_, AppCore>,
    request: StartBatchRequest,
) -> Result<String, String> {
    core.start_batch(&app, request)
}

#[tauri::command]
fn confirm_batch_holder(app: AppHandle, core: State<'_, AppCore>) -> Result<(), String> {
    core.confirm_batch_holder(app)
}

#[tauri::command]
fn cancel_batch(app: AppHandle, core: State<'_, AppCore>) -> Result<(), String> {
    core.cancel_batch(&app)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let resource_dir = app.path().resource_dir()?;
            sidecar::SidecarResolver::resolve(
                &resource_dir,
                sidecar::SidecarResource::Arm64Test,
                sidecar::trusted_arm64_test_sidecar(),
            )
            .map_err(std::io::Error::other)?;
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
            cancel_scan,
            start_batch,
            confirm_batch_holder,
            cancel_batch
        ])
        .run(tauri::generate_context!())
        .expect("error while running BeBlog Digitizer");
}
