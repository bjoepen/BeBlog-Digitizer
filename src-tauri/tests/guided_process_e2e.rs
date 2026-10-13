use beblog_digitizer_lib::{
    archive::{ArchiveManager, CaptureKind},
    offline_import::{export_validated_artifacts, import_offline_artifacts, OfflineArtifactPaths},
};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    process::{Command, Stdio},
};

#[test]
fn child_simulator_handshake_events_validate_archive_and_export_all_artifacts() {
    let root = std::env::temp_dir().join(format!("beblog-d4-e2e-{}", uuid::Uuid::new_v4()));
    let work = root.join("work");
    let archive_root = root.join("archive");
    let export_root = root.join("external-output");
    fs::create_dir_all(&work).unwrap();
    fs::create_dir_all(&export_root).unwrap();
    let prefix = work.join("capture");
    let mut child = Command::new(env!("CARGO_BIN_EXE_beblog-digitizer"))
        .args([
            "--beblog-guided-simulator",
            "--prefix",
            prefix.to_str().unwrap(),
            "--frame",
            "1",
            "--dpi",
            "600",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the explicitly built simulator should start");

    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut events = Vec::new();
    let mut line = String::new();
    stdout.read_line(&mut line).unwrap();
    assert_eq!(line.trim_end(), "BEBLOG/1\tOPENED");
    events.push(line.trim_end().to_string());
    std::thread::sleep(std::time::Duration::from_millis(30));
    assert_eq!(
        child.try_wait().unwrap(),
        None,
        "child must remain blocked at OPENED until the explicit user continuation"
    );
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"CONTINUE\n")
        .unwrap();
    child.stdin.as_mut().unwrap().flush().unwrap();
    loop {
        line.clear();
        if stdout.read_line(&mut line).unwrap() == 0 {
            break;
        }
        events.push(line.trim_end().to_string());
    }
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    let status = child.wait().unwrap();
    assert!(status.success(), "simulator failed: {stderr}");
    assert!(events
        .iter()
        .any(|event| event == "BEBLOG/1\tHOLDER_DETECTED"));
    assert!(events.iter().any(|event| event == "BEBLOG/1\tEOF\t1438200"));
    assert_eq!(
        events.last().map(String::as_str),
        Some("BEBLOG/1\tCOMPLETE")
    );

    let paths = OfflineArtifactPaths::from_fixed_output_directory(&work);
    let archive = ArchiveManager::new(archive_root);
    let imported = import_offline_artifacts(&archive, 1, &paths).unwrap();
    assert_eq!(imported.manifest.capture_kind, CaptureKind::OfflineImport);
    let exported = export_validated_artifacts(1, &paths, &export_root).unwrap();
    for name in [
        "capture.raw",
        "capture.tiff",
        "capture.params.txt",
        "capture.client.log",
    ] {
        assert_eq!(
            fs::read(exported.join(name)).unwrap(),
            fs::read(work.join(name)).unwrap()
        );
    }
    assert!(exported.starts_with(fs::canonicalize(&export_root).unwrap()));
    fs::remove_dir_all(root).unwrap();
}
