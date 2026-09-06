use std::{fs, process::Command, time::SystemTime};

#[test]
fn invalid_local_assets_fail_before_connection_startup() {
    let unique = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "cinnabar-startup-order-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir(&directory).unwrap();
    let assets = directory.join("invalid.mcbea");
    fs::write(&assets, b"invalid world carrier").unwrap();
    let socket_dir = directory.join("no-bridge");

    for direct in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_bedrock-client"));
        command
            .arg("--assets")
            .arg(&assets)
            .arg("--socket-dir")
            .arg(&socket_dir);
        if direct {
            command.args(["--address", "127.0.0.1:1"]);
        }
        let output = command.output().unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success());
        assert!(stderr.contains("load startup block assets"), "{stderr}");
        assert!(!stderr.contains("spawn Go core"), "{stderr}");
        assert!(!stderr.contains("wait for Go core"), "{stderr}");
    }
    fs::remove_dir_all(directory).unwrap();
}
