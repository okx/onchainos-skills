//! Offline CLI boundary tests. All signals target children owned by this test.
mod common;
use common::{assert_error_contains, assert_ok_and_extract_data, fresh_home, onchainos, scrubbed};
use serde_json::json;
use std::{fs, path::Path, process::Output};

fn session(home: &Path, id: &str) -> std::path::PathBuf {
    let dir = home.join("watch").join(id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("config.json"),
        json!({"channels":["price"],"created_at":0}).to_string(),
    )
    .unwrap();
    fs::write(dir.join("status"), "stopped|0").unwrap();
    dir
}

fn stop(home: &Path, id: Option<&str>, flush: bool) -> Output {
    let mut cmd = onchainos();
    scrubbed(&mut cmd, home).args(["ws", "stop"]);
    if let Some(id) = id {
        cmd.args(["--id", id]);
    }
    if flush {
        cmd.arg("--flush");
    }
    cmd.output().unwrap()
}

#[test]
fn invalid_ids_and_missing_sessions_preserve_all_targets() {
    let (_guard, home) = fresh_home("ws_stop_invalid");
    let dir = session(&home, "ws_abc123");
    let outside = home.join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("sentinel"), b"keep byte-for-byte").unwrap();
    for id in [
        "",
        ".",
        "..",
        "../outside",
        "ws_abc123/../outside",
        "ws_abc123/",
        "ws_abc123\\x",
        "C:\\outside",
        "C:outside",
        "\\\\server\\share",
        "ws_abc123.",
        "ws_abc123 ",
        "ws_ABC123",
        "WS_abc123",
        "ws_abc12g",
        "ws_abc12",
        "ws_abc1234",
        "ws_abc123\n",
        "ws_ébc123",
        "watch_123456",
        outside.to_str().unwrap(),
    ] {
        for flush in [false, true] {
            let output = stop(&home, Some(id), flush);
            assert_error_contains(&output, &["invalid"]);
            assert!(dir.join("config.json").exists());
            assert_eq!(
                fs::read(outside.join("sentinel")).unwrap(),
                b"keep byte-for-byte"
            );
        }
    }
    assert_error_contains(&stop(&home, Some("ws_ffffff"), false), &["not found"]);
    assert!(dir.exists());
}

#[test]
fn valid_sessions_flush_and_cleanup_without_affecting_others() {
    let (_guard, home) = fresh_home("ws_stop_valid");
    let other = session(&home, "ws_def456");
    for id in ["ws_abc123", "ws_123abc"] {
        let dir = session(&home, id);
        fs::write(
            dir.join("events.price.0.jsonl"),
            "{\"price\":1}\n{\"price\":2}\n",
        )
        .unwrap();
        let data = assert_ok_and_extract_data(&stop(&home, Some(id), true));
        assert_eq!(data["id"], id);
        assert_eq!(data["status"], "stopped");
        assert_eq!(data["flushed_count"], 2);
        assert_eq!(data["flushed_events"], json!([{"price":1},{"price":2}]));
        assert!(!dir.exists());
        assert!(other.exists());
    }
    let data = assert_ok_and_extract_data(&stop(&home, Some("ws_def456"), false));
    assert_eq!(data["flushed_count"], 0);
    assert!(!other.exists());
    assert_eq!(fs::read_dir(home.join("watch")).unwrap().count(), 0);
    assert_eq!(
        assert_ok_and_extract_data(&stop(&home, None, false))["stopped"],
        json!([])
    );
}

#[test]
fn list_and_poll_preserve_session_state_and_flush_only_unread_events() {
    let (_guard, home) = fresh_home("ws_stop_poll");
    let dir = session(&home, "ws_abc123");
    fs::write(dir.join("events.price.0.jsonl"), "{\"n\":1}\n{\"n\":2}\n").unwrap();
    let output = scrubbed(&mut onchainos(), &home)
        .args(["ws", "list"])
        .output()
        .unwrap();
    let data = assert_ok_and_extract_data(&output);
    assert_eq!(data[0]["id"], "ws_abc123");
    assert_eq!(data[0]["channels"], json!(["price"]));
    assert_eq!(data[0]["status"], "stopped");
    let output = scrubbed(&mut onchainos(), &home)
        .args([
            "ws",
            "poll",
            "--id",
            "ws_abc123",
            "--channel",
            "price",
            "--limit",
            "1",
        ])
        .output()
        .unwrap();
    let data = assert_ok_and_extract_data(&output);
    assert_eq!(data["new_count"], 1);
    assert_eq!(data["events"], json!([{"n":1}]));
    assert_eq!(fs::read_to_string(dir.join("cursor.price")).unwrap(), "0|8");
    let data = assert_ok_and_extract_data(&stop(&home, Some("ws_abc123"), true));
    assert_eq!(data["flushed_events"], json!([{"n":2}]));
    assert_eq!(data["flushed_count"], 1);
    assert!(!dir.exists());
}

#[cfg(unix)]
#[test]
fn unsafe_entries_fail_while_batch_continues_with_safe_sessions() {
    use std::os::unix::fs::symlink;
    let (_guard, home) = fresh_home("ws_stop_batch");
    let safe = session(&home, "ws_aabbcc");
    let bad = session(&home, "watch_bad");
    fs::write(bad.join("config.json"), "{}").unwrap();
    let arbitrary = home.join("watch/ws_ccdd00");
    fs::create_dir_all(&arbitrary).unwrap();
    fs::write(arbitrary.join("sentinel"), "keep").unwrap();
    let outside = home.join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("sentinel"), "keep").unwrap();
    symlink(&outside, home.join("watch/ws_abcdef")).unwrap();
    // macOS filesystems reject these non-UTF-8 names during fixture creation.
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::ffi::OsStrExt;
        fs::create_dir_all(
            home.join("watch")
                .join(std::ffi::OsStr::from_bytes(b"unrelated_\xff")),
        )
        .unwrap();
        fs::create_dir_all(
            home.join("watch")
                .join(std::ffi::OsStr::from_bytes(b"ws_\xff")),
        )
        .unwrap();
    }
    let output = stop(&home, None, true);
    assert_error_contains(
        &output,
        &["ws_aabbcc", "watch_bad", "ws_ccdd00", "ws_abcdef"],
    );
    assert!(!safe.exists());
    assert!(bad.exists());
    assert!(arbitrary.exists());
    assert_eq!(
        fs::read_to_string(outside.join("sentinel")).unwrap(),
        "keep"
    );
    assert!(fs::symlink_metadata(home.join("watch/ws_abcdef"))
        .unwrap()
        .file_type()
        .is_symlink());
}

#[cfg(unix)]
#[test]
fn valid_stop_terminates_only_its_owned_test_process() {
    let (_guard, home) = fresh_home("ws_stop_process");
    let dir = session(&home, "ws_112233");
    let mut child = std::process::Command::new("sleep")
        .arg("60")
        .spawn()
        .unwrap();
    fs::write(dir.join("pid"), child.id().to_string()).unwrap();
    let output = stop(&home, Some("ws_112233"), false);
    let status = child.try_wait().unwrap();
    if status.is_none() {
        let _ = child.kill();
    }
    let _ = child.wait();
    assert_ok_and_extract_data(&output);
    assert!(status.is_some(), "stop must terminate the owned process");
    assert!(!dir.exists());
}

#[cfg(unix)]
#[test]
fn linked_session_and_files_cannot_signal_a_test_child_or_modify_external_files() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let (_guard, home) = fresh_home("ws_stop_links");
    let mut child = std::process::Command::new("sleep")
        .arg("60")
        .spawn()
        .unwrap();
    struct ChildGuard<'a>(&'a mut std::process::Child);
    impl Drop for ChildGuard<'_> {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let child = ChildGuard(&mut child);
    let outside = session(&home, "ws_a1b2c3");
    fs::rename(&outside, home.join("outside")).unwrap();
    let outside = home.join("outside");
    fs::write(outside.join("pid"), child.0.id().to_string()).unwrap();
    fs::write(outside.join("daemon.log"), "private sentinel").unwrap();
    fs::set_permissions(
        outside.join("daemon.log"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    let root = home.join("watch");
    symlink(&outside, root.join("ws_abcdef")).unwrap();
    for flush in [false, true] {
        assert_error_contains(&stop(&home, Some("ws_abcdef"), flush), &["unsafe"]);
        assert!(child.0.try_wait().unwrap().is_none());
        assert_eq!(
            fs::read_to_string(outside.join("daemon.log")).unwrap(),
            "private sentinel"
        );
        assert_eq!(common::file_mode(&outside.join("daemon.log")), 0o644);
    }
    for name in [
        "config.json",
        "pid",
        "status",
        "cursor.price",
        "events.price.0.jsonl",
        "events.price.5.jsonl",
        ".status.tmp",
        ".cursor.price.tmp",
        "CONFIG.JSON",
        "PID",
        "STATUS",
        ".STATUS.TMP",
        "CURSOR.PRICE",
        "EVENTS.PRICE.0.JSONL",
        ".CURSOR.PRICE.TMP",
    ] {
        let dir = session(&home, "ws_445566");
        let path = dir.join(name);
        if path.exists() {
            fs::remove_file(&path).unwrap();
        }
        symlink(
            outside.join(if name == "pid" { "pid" } else { "daemon.log" }),
            &path,
        )
        .unwrap();
        assert_error_contains(&stop(&home, Some("ws_445566"), true), &["unsafe"]);
        assert!(child.0.try_wait().unwrap().is_none());
        assert!(dir.exists());
        fs::remove_dir_all(dir).unwrap();
    }
    let dir = session(&home, "ws_778899");
    symlink(&outside, dir.join("nested")).unwrap();
    assert_ok_and_extract_data(&stop(&home, Some("ws_778899"), false));
    assert_eq!(
        fs::read_to_string(outside.join("daemon.log")).unwrap(),
        "private sentinel"
    );
    assert!(child.0.try_wait().unwrap().is_none());
}

#[cfg(unix)]
#[test]
fn watch_root_link_is_rejected_and_configured_home_link_is_supported() {
    use std::os::unix::fs::symlink;
    let (_guard, home) = fresh_home("ws_stop_root");
    let dir = session(&home, "ws_abc123");
    fs::rename(home.join("watch"), home.join("outside")).unwrap();
    symlink(home.join("outside"), home.join("watch")).unwrap();
    assert_error_contains(&stop(&home, Some("ws_abc123"), false), &["unsafe"]);
    assert!(home.join("outside/ws_abc123").exists());
    fs::remove_file(home.join("watch")).unwrap();
    fs::rename(home.join("outside"), home.join("watch")).unwrap();
    let alias = home.join("home-alias");
    symlink(&home, &alias).unwrap();
    assert_ok_and_extract_data(&stop(&alias, Some("ws_abc123"), false));
    assert!(!dir.exists());
}

#[test]
fn regular_file_and_unmarked_directory_targets_are_preserved() {
    let (_guard, home) = fresh_home("ws_stop_file");
    fs::create_dir_all(home.join("watch")).unwrap();
    let file = home.join("watch/ws_aabbcc");
    fs::write(&file, b"keep").unwrap();
    let unmarked = home.join("watch/ws_ccdd00");
    fs::create_dir(&unmarked).unwrap();
    fs::write(unmarked.join("sentinel"), b"keep").unwrap();
    for flush in [false, true] {
        assert_error_contains(&stop(&home, Some("ws_aabbcc"), flush), &["unsafe"]);
        assert_error_contains(&stop(&home, Some("ws_ccdd00"), flush), &["config"]);
        assert_eq!(fs::read(&file).unwrap(), b"keep");
        assert_eq!(fs::read(unmarked.join("sentinel")).unwrap(), b"keep");
    }
}

#[cfg(unix)]
#[test]
fn invalid_ids_never_signal_or_modify_existing_sessions() {
    let (_guard, home) = fresh_home("ws_stop_invalid_process");
    let mut child = std::process::Command::new("sleep")
        .arg("60")
        .spawn()
        .unwrap();
    let invalid = [
        "watch_123456",
        "ws_ABC123",
        "ws_abc12g",
        "ws_abc12",
        "ws_abc1234",
    ];
    let mut results = Vec::new();
    for id in invalid {
        let dir = session(&home, id);
        fs::write(dir.join("pid"), child.id().to_string()).unwrap();
        for flush in [false, true] {
            results.push((
                stop(&home, Some(id), flush),
                child.try_wait().unwrap().is_none(),
            ));
        }
        assert_eq!(fs::read_to_string(dir.join("status")).unwrap(), "stopped|0");
    }
    let _ = child.kill();
    let _ = child.wait();
    for (output, alive) in results {
        assert_error_contains(&output, &["invalid"]);
        assert!(alive);
    }
}

#[cfg(windows)]
#[test]
fn windows_reparse_targets_are_rejected_and_child_links_preserve_sentinels() {
    use std::os::windows::fs::symlink_dir;
    let (_guard, home) = fresh_home("ws_stop_reparse");
    let dir = session(&home, "ws_aabbcc");
    let outside = home.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("sentinel"), b"keep").unwrap();
    // The supported Windows test host needs Developer Mode or symlink privilege.
    symlink_dir(&outside, home.join("watch/ws_abcdef")).unwrap();
    for flush in [false, true] {
        assert_error_contains(&stop(&home, Some("ws_abcdef"), flush), &["unsafe"]);
    }
    for name in [
        "CONFIG.JSON",
        "PID",
        "STATUS",
        ".STATUS.TMP",
        "CURSOR.PRICE",
        ".CURSOR.PRICE.TMP",
        "EVENTS.PRICE.0.JSONL",
    ] {
        let linked = session(&home, "ws_445566");
        let path = linked.join(name);
        if path.exists() {
            fs::remove_file(&path).unwrap();
        }
        std::os::windows::fs::symlink_file(outside.join("sentinel"), &path).unwrap();
        assert_error_contains(&stop(&home, Some("ws_445566"), true), &["unsafe"]);
        assert!(linked.exists());
        fs::remove_dir_all(linked).unwrap();
    }
    symlink_dir(&outside, dir.join("child")).unwrap();
    assert_ok_and_extract_data(&stop(&home, Some("ws_aabbcc"), false));
    assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"keep");
    // Junctions also carry FILE_ATTRIBUTE_REPARSE_POINT.
    let junction = home.join("watch/ws_123abc");
    let status = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&junction)
        .arg(&outside)
        .status()
        .unwrap();
    assert!(status.success());
    assert_error_contains(&stop(&home, Some("ws_123abc"), false), &["unsafe"]);
    assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"keep");
}

#[test]
fn stale_pid_session_still_cleans_up() {
    let (_guard, home) = fresh_home("ws_stop_stale_pid");
    let dir = session(&home, "ws_abcdef");
    #[cfg(unix)]
    let mut child = std::process::Command::new("sh")
        .args(["-c", "exit 0"])
        .spawn()
        .unwrap();
    #[cfg(windows)]
    let mut child = std::process::Command::new("cmd")
        .args(["/C", "exit 0"])
        .spawn()
        .unwrap();
    let pid = child.id();
    child.wait().unwrap();
    fs::write(dir.join("pid"), pid.to_string()).unwrap();
    assert_ok_and_extract_data(&stop(&home, Some("ws_abcdef"), false));
    assert!(!dir.exists());
}
