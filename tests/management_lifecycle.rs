//! 製品debug EXEを実際に別プロセスで起動する。DB・IPCを一意に分離する。
#![cfg(debug_assertions)]
use http_selector::management::current_user_sid;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;
use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW, WM_KEYDOWN};

struct RunningApp {
    child: Child,
    _data: tempfile::TempDir,
    pipe: String,
    window_class: String,
}
impl RunningApp {
    fn start() -> Self {
        let data = tempfile::tempdir().unwrap();
        let namespace = format!(
            "{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let hash = format!(
            "{:x}",
            Sha256::digest(format!("HttpSelector.Test.{namespace}").as_bytes())
        );
        let child = Command::new(env!("CARGO_BIN_EXE_HttpSelector"))
            .args([
                "--startup",
                "--test-isolated",
                "--launch-origin",
                "via_kotorintoys",
            ])
            .env("HTTPSELECTOR_TEST_DATA_DIR", data.path())
            .env("HTTPSELECTOR_TEST_NO_SCAN", "1")
            .env("HTTPSELECTOR_TEST_NAMESPACE", namespace)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let app = Self {
            child,
            _data: data,
            pipe: format!(
                r"\\.\pipe\KotorinToys.{}.http-selector.{hash}",
                current_user_sid().unwrap()
            ),
            window_class: format!("HttpSelector.MainWindow.{hash}"),
        };
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            if let Some(reply) = app.request(json!({"command":"status","requestId":"ready"}))
                && reply["ready"] == true
            {
                break;
            }
            assert!(Instant::now() < deadline, "隔離EXEの起動が時間切れ");
            std::thread::sleep(Duration::from_millis(25));
        }
        app
    }
    fn request(&self, value: Value) -> Option<Value> {
        let mut pipe = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.pipe)
            .ok()?;
        writeln!(pipe, "{value}").ok()?;
        pipe.flush().ok()?;
        let mut line = String::new();
        BufReader::new(pipe).read_line(&mut line).ok()?;
        serde_json::from_str(&line).ok()
    }
    fn call(&self, command: &str, pid: u32) -> Value {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(reply) =
                self.request(json!({"command":command,"expectedPid":pid,"requestId":command}))
            {
                return reply;
            }
            assert!(Instant::now() < deadline, "管理要求が時間切れ: {command}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    fn window(&self) -> windows_sys::Win32::Foundation::HWND {
        let class: Vec<_> = self.window_class.encode_utf16().chain(Some(0)).collect();
        unsafe { FindWindowW(class.as_ptr(), std::ptr::null()) }
    }
    fn stop(&mut self) {
        assert_eq!(self.call("stop", self.child.id())["result"], "accepted");
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success());
                return;
            }
            assert!(Instant::now() < deadline, "受理後に正常終了しない");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
impl Drop for RunningApp {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[test]
fn startup_status_pid_rejection_and_normal_stop_use_the_real_executable() {
    let mut app = RunningApp::start();
    assert!(app.window().is_null(), "startupでは画面を出さない");
    let reply = app.call("status", app.child.id());
    assert_eq!(reply["launchOrigin"], "via_kotorintoys");
    assert_eq!(reply["protocolVersion"], 2);
    assert_eq!(reply["version"], http_selector::app_title::VERSION);
    assert_eq!(app.call("stop", app.child.id() + 1)["result"], "refused");
    assert!(app.child.try_wait().unwrap().is_none());
    app.stop();
}

#[test]
#[ignore = "対話デスクトップに試験用の選択画面を表示するためCLIオプションで実行"]
fn interactive_desktop_show_escape_and_stop_use_the_real_executable() {
    let mut app = RunningApp::start();
    assert_eq!(app.call("show", app.child.id())["result"], "completed");
    let window = app.window();
    assert!(!window.is_null());
    unsafe {
        assert_ne!(PostMessageW(window, WM_KEYDOWN, VK_ESCAPE as usize, 0), 0);
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    while !app.window().is_null() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(app.call("show", app.child.id())["result"], "completed");
    app.stop();
}
