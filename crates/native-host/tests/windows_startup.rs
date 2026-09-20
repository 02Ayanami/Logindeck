#![cfg(target_os = "windows")]

use std::{
    io::{Read, Write},
    process::{Command, Stdio},
};

use autologin_native_host::EXTENSION_ID;

#[test]
fn pinned_edge_origin_receives_disconnected_status_without_desktop_lock() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_autologin-native-host"))
        .arg(format!("chrome-extension://{EXTENSION_ID}/"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start native host");

    let request = br#"{"version":1,"requestId":"startup","body":{"type":"status.get"}}"#;
    let mut stdin = child.stdin.take().expect("child stdin");
    stdin
        .write_all(&(request.len() as u32).to_le_bytes())
        .expect("write frame length");
    stdin.write_all(request).expect("write frame body");
    drop(stdin);

    let mut stdout = child.stdout.take().expect("child stdout");
    let mut length = [0_u8; 4];
    stdout.read_exact(&mut length).expect("read frame length");
    let mut body = vec![0_u8; u32::from_le_bytes(length) as usize];
    stdout.read_exact(&mut body).expect("read frame body");

    let response: serde_json::Value = serde_json::from_slice(&body).expect("status response");
    assert_eq!(response["type"], "status");
    assert_eq!(response["connected"], false);
    assert_eq!(response["enabled"], false);

    let status = child.wait().expect("wait for native host");
    assert!(status.success());
}
