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

#[test]
fn repeated_single_frame_scans_use_separate_children_and_repeat_holder_handshake() {
    let root = std::env::temp_dir().join(format!("beblog-d5-repeat-e2e-{}", uuid::Uuid::new_v4()));
    let archive_root = root.join("archive");
    let archive = ArchiveManager::new(archive_root.clone());

    for (ordinal, frame) in [1_u8, 6_u8].into_iter().enumerate() {
        let work = root.join(format!("work-{ordinal}"));
        let export_root = root.join(format!("export-{ordinal}"));
        fs::create_dir_all(&work).unwrap();
        fs::create_dir_all(&export_root).unwrap();
        let prefix = work.join("capture");
        let frame_text = frame.to_string();
        let mut child = Command::new(env!("CARGO_BIN_EXE_beblog-digitizer"))
            .args([
                "--beblog-guided-simulator",
                "--prefix",
                prefix.to_str().unwrap(),
                "--frame",
                &frame_text,
                "--dpi",
                "600",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("each single scan starts a fresh harmless simulator child");

        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        assert_eq!(line.trim_end(), "BEBLOG/1\tOPENED");
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert_eq!(child.try_wait().unwrap(), None, "no automatic continuation");
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(b"CONTINUE\n")
            .unwrap();
        child.stdin.as_mut().unwrap().flush().unwrap();

        let mut events = vec![line.trim_end().to_owned()];
        line.clear();
        while stdout.read_line(&mut line).unwrap() != 0 {
            events.push(line.trim_end().to_owned());
            line.clear();
        }
        let mut stderr = String::new();
        child
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut stderr)
            .unwrap();
        assert!(
            child.wait().unwrap().success(),
            "simulator failed: {stderr}"
        );
        assert!(events
            .iter()
            .any(|event| event == "BEBLOG/1\tHOLDER_DETECTED"));
        assert!(events.iter().any(|event| event == "BEBLOG/1\tEOF\t1438200"));
        assert_eq!(
            events.last().map(String::as_str),
            Some("BEBLOG/1\tCOMPLETE")
        );

        let paths = OfflineArtifactPaths::from_fixed_output_directory(&work);
        let imported = import_offline_artifacts(&archive, frame, &paths).unwrap();
        assert_eq!(imported.manifest.frame, frame);
        let exported = export_validated_artifacts(frame, &paths, &export_root).unwrap();
        assert!(exported.starts_with(fs::canonicalize(&export_root).unwrap()));
        for suffix in ["raw", "tiff", "params.txt", "client.log"] {
            assert!(exported.join(format!("capture.{suffix}")).is_file());
        }
    }

    let captures = fs::read_dir(&archive_root)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name() != ".staging" && entry.path().is_dir())
        .count();
    assert_eq!(captures, 2, "each process must archive a separate capture");
    fs::remove_dir_all(root).unwrap();
}
