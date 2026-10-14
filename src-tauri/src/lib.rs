mod adapter;
pub mod archive;
pub mod batch_plan;
mod client_simulator;
mod core;
mod guided_process;
mod guided_protocol;
#[cfg(test)]
mod hardware_test_prep;
mod log_parser;
mod model;
pub mod offline_import;
mod process;
mod scanner_discovery;
mod scanner_lease;
mod sidecar;
mod state_machine;

use core::{AppCore, JobSummary};
use model::{AppSnapshot, StartBatchRequest, StartScanRequest};
use tauri::{AppHandle, Manager, State};

pub fn run_guided_simulator(args: &[std::ffi::OsString]) -> Result<(), String> {
    client_simulator::run_from_args(args)
}

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

#[tauri::command]
fn start_guided_scan(
    app: AppHandle,
    core: State<'_, AppCore>,
    request: guided_process::GuidedScanRequest,
) -> Result<String, String> {
    core.start_guided_scan(app, request)
}

#[tauri::command]
fn start_guided_simulation(
    app: AppHandle,
    core: State<'_, AppCore>,
    frame: u8,
    dpi: u16,
    output_directory: String,
) -> Result<String, String> {
    core.start_guided_simulation(app, frame, dpi, output_directory.into())
}

#[tauri::command]
fn confirm_guided_holder(core: State<'_, AppCore>) -> Result<(), String> {
    core.confirm_guided_holder()
}

#[tauri::command]
fn cancel_guided_scan(core: State<'_, AppCore>) -> Result<(), String> {
    core.cancel_guided_scan()
}

#[tauri::command]
fn detect_minolta_scanners(
    core: State<'_, AppCore>,
) -> Result<scanner_discovery::DiscoveryResult, String> {
    core.discover_minolta_scanners()
}

#[tauri::command]
fn guided_scan_cooldown_remaining(core: State<'_, AppCore>) -> u64 {
    core.guided_cooldown_remaining_seconds()
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
            let core = AppCore::new_with_resource(archive_root, resource_dir);
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
            cancel_batch,
            start_guided_scan,
            start_guided_simulation,
            confirm_guided_holder,
            cancel_guided_scan,
            detect_minolta_scanners,
            guided_scan_cooldown_remaining
        ])
        .run(tauri::generate_context!())
        .expect("error while running BeBlog Digitizer");
}
