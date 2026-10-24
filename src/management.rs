//! KotorinToys管理契約2と、画面・検査の寿命に依存しないURL受付窓口。
use crate::url_input::ValidatedUrl;
use crate::win32::{wide, window_state};
use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, Weak, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::UpdateWindow;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub const TOOL_ID: &str = "http-selector";
pub const BROKER_CLASS: &str = "HttpSelector.UrlBroker";
pub const WM_MANAGEMENT_SHOW: u32 = WM_APP + 40;
pub const WM_MANAGEMENT_STOP: u32 = WM_APP + 41;
pub const WM_MANAGEMENT_STATUS: u32 = WM_APP + 42;
const WM_MANAGEMENT_EXIT: u32 = WM_APP + 43;
pub const MAX_REQUEST_BYTES: usize = 8192;
const MAX_QUEUED_URLS: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Event {
    Url(ValidatedUrl),
    Show,
    Stop,
}

struct QueuedEvent {
    event: Event,
    deadline: Option<Instant>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UiResult {
    Completed,
    Accepted,
    Refused,
}

struct Pending {
    command: String,
    deadline: Instant,
    sender: mpsc::SyncSender<UiResult>,
}

struct Shared {
    ready: AtomicBool,
    shutdown: AtomicBool,
    stopping: AtomicBool,
    busy: AtomicUsize,
    active: AtomicUsize,
    broker: AtomicUsize,
    processing_url: AtomicBool,
    forwarded: AtomicBool,
    local_show: AtomicBool,
    next_token: AtomicUsize,
    pending: Mutex<HashMap<usize, Pending>>,
    queue: Mutex<VecDeque<QueuedEvent>>,
    available: Condvar,
}

impl Shared {
    fn new() -> Self {
        Self {
            ready: AtomicBool::new(false),
            shutdown: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
            busy: AtomicUsize::new(0),
            active: AtomicUsize::new(0),
            broker: AtomicUsize::new(0),
            processing_url: AtomicBool::new(false),
            forwarded: AtomicBool::new(false),
            local_show: AtomicBool::new(false),
            next_token: AtomicUsize::new(1),
            pending: Mutex::new(HashMap::new()),
            queue: Mutex::new(VecDeque::new()),
            available: Condvar::new(),
        }
    }

    fn enqueue(&self, event: Event, deadline: Option<Instant>) -> bool {
        let mut queue = self.queue.lock().unwrap();
        // 過負荷では送信側へ拒否を返す。受理したURLは上書きしない。
        if matches!(event, Event::Url(_))
            && queue
                .iter()
                .filter(|item| matches!(item.event, Event::Url(_)))
                .count()
                >= MAX_QUEUED_URLS
        {
            return false;
        }
        queue.push_back(QueuedEvent { event, deadline });
        self.available.notify_one();
        true
    }

    fn pop(queue: &mut VecDeque<QueuedEvent>) -> Option<Event> {
        while let Some(item) = queue.pop_front() {
            if item
                .deadline
                .is_none_or(|deadline| Instant::now() < deadline)
            {
                return Some(item.event);
            }
        }
        None
    }

    fn complete(&self, token: usize, result: UiResult) {
        if let Some(pending) = self.pending.lock().unwrap().remove(&token) {
            if Instant::now() < pending.deadline {
                let _ = pending.sender.send(result);
            }
        }
    }

    fn request_ui(&self, command: &str, deadline: Instant) -> Option<UiResult> {
        let token = self.next_token.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = mpsc::sync_channel(1);
        self.pending.lock().unwrap().insert(
            token,
            Pending {
                command: command.to_owned(),
                deadline,
                sender,
            },
        );
        let active = self.active.load(Ordering::Acquire) as HWND;
        if command == "show" && active.is_null() {
            self.enqueue(Event::Show, Some(deadline));
        } else {
            let target = if active.is_null() {
                self.broker.load(Ordering::Acquire) as HWND
            } else {
                active
            };
            let message = match command {
                "show" => WM_MANAGEMENT_SHOW,
                "stop" => WM_MANAGEMENT_STOP,
                _ => WM_MANAGEMENT_STATUS,
            };
            if unsafe { PostMessageW(target, message, token, 0) } == 0 {
                self.pending.lock().unwrap().remove(&token);
                return None;
            }
        }
        let result = receiver
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .ok();
        self.pending.lock().unwrap().remove(&token);
        result
    }

    fn receive_url(&self, url: ValidatedUrl) -> bool {
        if self.stopping.load(Ordering::Acquire) {
            return false;
        }
        let active = self.active.load(Ordering::Acquire) as HWND;
        let queue_empty = self.queue.lock().unwrap().is_empty();
        if !active.is_null()
            && self.busy.load(Ordering::Acquire) == 0
            && !self.processing_url.load(Ordering::Acquire)
            && queue_empty
            && !self.forwarded.swap(true, Ordering::AcqRel)
            && crate::single_instance::send_url(active, &url, 500)
                .is_some_and(|accepted| accepted != 0)
        {
            return true;
        }
        self.enqueue(Event::Url(url), None)
    }

    /// 設定編集中・確認中の停止要求は拒否する。
    fn stop_result(&self) -> UiResult {
        if self.busy.load(Ordering::Acquire) > 0 {
            UiResult::Refused
        } else {
            UiResult::Accepted
        }
    }

    fn commit_stop(&self) {
        self.stopping.store(true, Ordering::Release);
        self.queue.lock().unwrap().push_front(QueuedEvent {
            event: Event::Stop,
            deadline: None,
        });
        self.available.notify_one();
        let active = self.active.load(Ordering::Acquire) as HWND;
        if !active.is_null() {
            unsafe { PostMessageW(active, WM_MANAGEMENT_EXIT, 0, 0) };
        }
    }
}

static CURRENT: OnceLock<Mutex<Weak<Shared>>> = OnceLock::new();

fn current() -> Option<Arc<Shared>> {
    CURRENT.get()?.lock().ok()?.upgrade()
}

pub struct Management {
    shared: Arc<Shared>,
    broker_thread: Option<JoinHandle<()>>,
    pipe_thread: Option<JoinHandle<()>>,
}

/// primaryの単一起動ロック取得後に呼ぶ。窓口を安全に作れなければ起動を失敗させる。
pub fn start(launch_origin: &str) -> anyhow::Result<Management> {
    let shared = Arc::new(Shared::new());
    // MSIXのPackage Familyごとに従来のデータ・IPC分離を維持する。
    // 非MSIX版だけがKotorinToysの標準tool-idを使用する。
    let pipe_id = crate::app_identity::AppIdentity::current()?.window_class(TOOL_ID);
    let origin = normalize_origin(launch_origin).to_owned();
    let (sender, receiver) = mpsc::sync_channel(1);
    let broker_shared = shared.clone();
    let broker_thread = std::thread::Builder::new()
        .name("http-selector-url-broker".into())
        .spawn(move || broker_loop(broker_shared, sender))?;
    let mut management = Management {
        shared: shared.clone(),
        broker_thread: Some(broker_thread),
        pipe_thread: None,
    };
    receiver
        .recv_timeout(Duration::from_secs(3))
        .context("URL受付窓口の起動が時間切れです")??;
    *CURRENT
        .get_or_init(|| Mutex::new(Weak::new()))
        .lock()
        .unwrap() = Arc::downgrade(&shared);
    let (sender, receiver) = mpsc::sync_channel(1);
    management.pipe_thread = Some(
        std::thread::Builder::new()
            .name("http-selector-management".into())
            .spawn(move || {
                let pipe = match ManagementPipe::create(&pipe_id) {
                    Ok(pipe) => pipe,
                    Err(error) => {
                        let _ = sender.send(Err(error));
                        return;
                    }
                };
                let _ = sender.send(Ok(()));
                while !shared.shutdown.load(Ordering::Acquire) {
                    if pipe.accept(Duration::from_millis(100)).is_err() {
                        continue;
                    }
                    let deadline = Instant::now() + Duration::from_secs(2);
                    let accepted =
                        handle_request(&pipe, &shared, &origin, deadline).unwrap_or(false);
                    pipe.wait_for_client_close(deadline);
                    pipe.disconnect();
                    if accepted {
                        shared.commit_stop();
                    }
                }
            })?,
    );
    receiver
        .recv_timeout(Duration::from_secs(3))
        .context("管理窓口の起動が時間切れです")??;
    Ok(management)
}

impl Management {
    pub fn set_ready(&self, ready: bool) {
        self.shared.ready.store(ready, Ordering::Release);
    }

    /// 選択中のURLを後着URLで置換しない。URLなし画面だけ既存UIの受付経路を使う。
    pub fn set_processing_url(&self, processing: bool) {
        self.shared
            .processing_url
            .store(processing, Ordering::Release);
    }

    pub fn show_requested(&self) -> bool {
        self.shared.local_show.swap(false, Ordering::AcqRel)
            || self
                .shared
                .pending
                .lock()
                .unwrap()
                .values()
                .any(|pending| pending.command == "show" && Instant::now() < pending.deadline)
    }

    pub fn try_event(&self) -> Option<Event> {
        Shared::pop(&mut self.shared.queue.lock().unwrap())
    }

    pub fn wait_event(&self) -> Event {
        let mut queue = self.shared.queue.lock().unwrap();
        loop {
            if let Some(event) = Shared::pop(&mut queue) {
                return event;
            }
            queue = self.shared.available.wait(queue).unwrap();
        }
    }
}

impl Drop for Management {
    fn drop(&mut self) {
        self.shared.shutdown.store(true, Ordering::Release);
        let broker = self.shared.broker.load(Ordering::Acquire) as HWND;
        if !broker.is_null() {
            unsafe { PostMessageW(broker, WM_CLOSE, 0, 0) };
        }
        if let Some(thread) = self.pipe_thread.take() {
            let _ = thread.join();
        }
        if let Some(thread) = self.broker_thread.take() {
            let _ = thread.join();
        }
    }
}

/// ShowWindow/UpdateWindowの後にUIスレッドで登録し、表示完了を管理要求へ返す。
pub fn set_active_window(window: HWND) {
    if let Some(shared) = current() {
        shared.active.store(window as usize, Ordering::Release);
        shared.forwarded.store(false, Ordering::Release);
        let tokens: Vec<_> = shared
            .pending
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(token, pending)| (pending.command == "show").then_some(*token))
            .collect();
        for token in tokens {
            shared.complete(token, UiResult::Completed);
        }
    }
}

pub fn clear_active_window(window: HWND) {
    if let Some(shared) = current() {
        let _ =
            shared
                .active
                .compare_exchange(window as usize, 0, Ordering::AcqRel, Ordering::Acquire);
    }
}

pub struct BusyGuard(Option<Arc<Shared>>);

/// 設定編集中・確認ダイアログ・外部検査の間は正常停止を拒否する。
pub fn busy_guard() -> BusyGuard {
    let shared = current();
    if let Some(shared) = &shared {
        shared.busy.fetch_add(1, Ordering::AcqRel);
    }
    BusyGuard(shared)
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        if let Some(shared) = &self.0 {
            shared.busy.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

/// 管理要求だけを扱い、通常の画面メッセージは呼出元の処理へ戻す。
///
/// # Safety
/// windowの所有UIスレッドから、そのwindowのWndProc内で呼ぶこと。
pub unsafe fn handle_window_message(window: HWND, message: u32, token: usize) -> Option<isize> {
    if !matches!(
        message,
        WM_MANAGEMENT_SHOW | WM_MANAGEMENT_STOP | WM_MANAGEMENT_STATUS | WM_MANAGEMENT_EXIT
    ) {
        return None;
    }
    let Some(shared) = current() else {
        return Some(0);
    };
    // URLなしの二重起動は管理パイプ外の、期限付き応答を持たない表示要求。
    if message == WM_MANAGEMENT_SHOW && token == 0 {
        bring_to_front(window);
        return Some(0);
    }
    if message == WM_MANAGEMENT_EXIT {
        if shared.stopping.load(Ordering::Acquire) && shared.busy.load(Ordering::Acquire) == 0 {
            unsafe { DestroyWindow(window) };
        }
        return Some(0);
    }
    let valid = shared
        .pending
        .lock()
        .unwrap()
        .get(&token)
        .is_some_and(|pending| Instant::now() < pending.deadline);
    if !valid {
        return Some(0);
    }
    let result = match message {
        WM_MANAGEMENT_SHOW => {
            bring_to_front(window);
            UiResult::Completed
        }
        WM_MANAGEMENT_STOP => shared.stop_result(),
        _ => UiResult::Completed,
    };
    shared.complete(token, result);
    Some(0)
}

fn bring_to_front(window: HWND) {
    unsafe {
        ShowWindow(window, SW_RESTORE);
        UpdateWindow(window);
        SetForegroundWindow(window);
    }
}

pub fn normalize_origin(origin: &str) -> &str {
    match origin {
        "via_kotorintoys" | "direct_task" | "host_manual" => origin,
        _ => "standalone",
    }
}

pub fn launch_origin(arguments: &[String]) -> &str {
    arguments
        .windows(2)
        .find(|pair| pair[0] == "--launch-origin")
        .map_or("standalone", |pair| normalize_origin(&pair[1]))
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Request {
    command: String,
    #[serde(default)]
    expected_pid: Option<u32>,
    #[serde(default)]
    request_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Reply<'a> {
    protocol_version: u32,
    request_id: &'a str,
    pid: u32,
    ready: bool,
    version: &'static str,
    launch_origin: &'a str,
    capabilities: [&'static str; 3],
    result: &'static str,
    detail: &'static str,
}

fn parse_request(line: &str) -> Result<Request, serde_json::Error> {
    if line == "status" {
        Ok(Request {
            command: "status".into(),
            ..Default::default()
        })
    } else {
        serde_json::from_str(line)
    }
}

fn validate(request: &Request, pid: u32, busy: bool) -> (&'static str, &'static str) {
    if request.expected_pid.is_some_and(|expected| expected != pid) {
        return ("refused", "対象PIDが変わりました。");
    }
    if request.request_id.len() > 128 {
        return ("refused", "要求IDが長すぎます。");
    }
    match request.command.as_str() {
        "show" | "stop" if request.expected_pid.is_none() => {
            ("refused", "操作には対象PIDが必要です。")
        }
        "stop" if busy => (
            "refused",
            "設定編集中またはURLの確認中です。操作を完了してから停止してください。",
        ),
        "stop" => ("accepted", ""),
        "status" | "show" => ("completed", ""),
        _ => ("unsupported", "未対応の操作です。"),
    }
}

fn handle_request(
    pipe: &ManagementPipe,
    shared: &Shared,
    origin: &str,
    deadline: Instant,
) -> anyhow::Result<bool> {
    ensure!(pipe.same_session()?, "別セッションの管理要求です");
    let request = parse_request(&pipe.read_line(deadline)?)?;
    let (mut result, mut detail) = validate(
        &request,
        std::process::id(),
        shared.busy.load(Ordering::Acquire) > 0,
    );
    if matches!(result, "completed" | "accepted") {
        match shared.request_ui(&request.command, deadline) {
            Some(UiResult::Completed) => {}
            Some(UiResult::Accepted) => {}
            Some(UiResult::Refused) => {
                result = "refused";
                detail = "設定編集中またはURLの確認中です。操作を完了してから停止してください。";
            }
            None => {
                result = "failed";
                detail = "UIの応答を期限内に確認できませんでした。";
            }
        }
    }
    let reply = Reply {
        protocol_version: 2,
        request_id: &request.request_id,
        pid: std::process::id(),
        ready: shared.ready.load(Ordering::Acquire) && result != "failed",
        version: crate::app_title::VERSION,
        launch_origin: origin,
        capabilities: ["status", "show", "stop"],
        result,
        detail,
    };
    pipe.write_line(&serde_json::to_string(&reply)?, deadline)?;
    Ok(result == "accepted")
}

fn broker_loop(shared: Arc<Shared>, sender: mpsc::SyncSender<anyhow::Result<()>>) {
    let result = create_broker(&shared);
    let window = match result {
        Ok(window) => window,
        Err(error) => {
            let _ = sender.send(Err(error));
            return;
        }
    };
    shared.broker.store(window as usize, Ordering::Release);
    let _ = sender.send(Ok(()));
    if shared.shutdown.load(Ordering::Acquire) {
        unsafe { DestroyWindow(window) };
        shared.broker.store(0, Ordering::Release);
        return;
    }
    let mut message: MSG = unsafe { std::mem::zeroed() };
    while unsafe { GetMessageW(&mut message, null_mut(), 0, 0) } > 0 {
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    shared.broker.store(0, Ordering::Release);
}

fn create_broker(shared: &Arc<Shared>) -> anyhow::Result<HWND> {
    let class = wide(&crate::app_identity::AppIdentity::current()?.window_class(BROKER_CLASS));
    let instance = unsafe { GetModuleHandleW(null()) };
    let mut registration: WNDCLASSW = unsafe { std::mem::zeroed() };
    registration.lpfnWndProc = Some(broker_proc);
    registration.hInstance = instance;
    registration.lpszClassName = class.as_ptr();
    if unsafe { RegisterClassW(&registration) } == 0 {
        ensure!(
            unsafe { GetLastError() } == 1410,
            "URL受付クラスを登録できません"
        );
    }
    // broker_loopがメッセージループ全体を所有するため、WndProcは借用だけを保持する。
    let pointer = Arc::as_ptr(shared);
    let window = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            wide("HttpSelector URL broker").as_ptr(),
            0,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            instance,
            pointer.cast(),
        )
    };
    if window.is_null() {
        anyhow::bail!("URL受付窓口を作成できません");
    }
    Ok(window)
}

unsafe extern "system" fn broker_proc(
    window: HWND,
    message: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    let pointer = unsafe { window_state::<Shared>(window, message, lparam) }.cast_const();
    if !pointer.is_null() {
        let shared = unsafe { &*pointer };
        if matches!(message, WM_MANAGEMENT_STOP | WM_MANAGEMENT_STATUS) {
            let result = if message == WM_MANAGEMENT_STOP {
                shared.stop_result()
            } else {
                UiResult::Completed
            };
            shared.complete(wparam, result);
            return 0;
        }
        match message {
            WM_COPYDATA => {
                let Some(url) = (unsafe { crate::single_instance::received_url(lparam) }) else {
                    return 0;
                };
                return isize::from(shared.receive_url(url));
            }
            WM_MANAGEMENT_SHOW => {
                // URLなしの二重起動は通常の画面表示要求。応答不要のローカル要求。
                let active = shared.active.load(Ordering::Acquire) as HWND;
                if active.is_null() {
                    shared.local_show.store(true, Ordering::Release);
                    shared.enqueue(Event::Show, None);
                } else {
                    unsafe { PostMessageW(active, WM_MANAGEMENT_SHOW, 0, 0) };
                }
                return 0;
            }
            WM_CLOSE => {
                unsafe { DestroyWindow(window) };
                return 0;
            }
            WM_DESTROY => {
                unsafe { PostQuitMessage(0) };
                return 0;
            }
            WM_NCDESTROY => unsafe {
                SetWindowLongPtrW(window, GWLP_USERDATA, 0);
            },
            _ => {}
        }
    }
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

mod pipe;
use pipe::ManagementPipe;
pub use pipe::current_user_sid;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};

    #[test]
    fn management_requests_require_pid_and_do_not_override_busy_ui() {
        assert_eq!(parse_request("status").unwrap().command, "status");
        assert!(parse_request("{").is_err());
        let mut request =
            parse_request(r#"{"command":"stop","expectedPid":42,"requestId":"test"}"#).unwrap();
        assert_eq!(validate(&request, 41, false).0, "refused");
        assert_eq!(validate(&request, 42, true).0, "refused");
        assert_eq!(validate(&request, 42, false).0, "accepted");
        request.expected_pid = None;
        assert_eq!(validate(&request, 42, false).0, "refused");
        request.command = "shortcuts".into();
        assert_eq!(validate(&request, 42, false).0, "unsupported");
        request.request_id = "x".repeat(129);
        assert_eq!(validate(&request, 42, false).0, "refused");
        assert_eq!(
            launch_origin(&["--launch-origin".into(), "direct_task".into()]),
            "direct_task"
        );
        assert_eq!(
            launch_origin(&["--launch-origin".into(), "invalid".into()]),
            "standalone"
        );
    }

    #[test]
    fn url_queue_preserves_fifo_and_discards_expired_show() {
        let shared = Shared::new();
        let first = ValidatedUrl::parse("https://example.com/one")
            .unwrap()
            .with_forced_confirmation(true);
        let second = ValidatedUrl::parse("https://example.com/two").unwrap();
        shared.enqueue(Event::Show, Some(Instant::now() - Duration::from_millis(1)));
        assert!(shared.receive_url(first.clone()));
        assert!(shared.receive_url(second.clone()));
        let mut queue = shared.queue.lock().unwrap();
        assert_eq!(Shared::pop(&mut queue), Some(Event::Url(first)));
        assert_eq!(Shared::pop(&mut queue), Some(Event::Url(second)));
        assert!(Shared::pop(&mut queue).is_none());
    }

    #[test]
    fn owned_pipe_is_exclusive_and_idle_accept_has_a_deadline() {
        let id = format!("http-selector-idle-test-{}", std::process::id());
        let pipe = ManagementPipe::create(&id).unwrap();
        assert!(ManagementPipe::create(&id).is_err());
        assert!(current_user_sid().unwrap().starts_with("S-1-"));
        let started = Instant::now();
        assert!(pipe.accept(Duration::from_millis(40)).is_err());
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn pipe_accepts_split_utf8_and_reconnects() {
        let id = format!("http-selector-lines-test-{}", std::process::id());
        let (ready, wait) = mpsc::sync_channel(1);
        let server_id = id.clone();
        let server = std::thread::spawn(move || {
            let pipe = ManagementPipe::create(&server_id).unwrap();
            ready.send(()).unwrap();
            for _ in 0..2 {
                pipe.accept(Duration::from_secs(3)).unwrap();
                assert!(pipe.same_session().unwrap());
                let deadline = Instant::now() + Duration::from_secs(2);
                assert_eq!(pipe.read_line(deadline).unwrap(), "状態");
                pipe.write_line("ok", deadline).unwrap();
                pipe.wait_for_client_close(deadline);
                pipe.disconnect();
            }
        });
        wait.recv_timeout(Duration::from_secs(3)).unwrap();
        let name = format!(r"\\.\pipe\KotorinToys.{}.{id}", current_user_sid().unwrap());
        for _ in 0..2 {
            std::thread::sleep(Duration::from_millis(30));
            let mut client = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&name)
                .unwrap();
            let input = "状態\r\n".as_bytes();
            client.write_all(&input[..1]).unwrap();
            client.write_all(&input[1..]).unwrap();
            let mut response = String::new();
            BufReader::new(&client).read_line(&mut response).unwrap();
            assert_eq!(response, "ok\n");
        }
        server.join().unwrap();
    }

    #[test]
    fn connected_client_cannot_hold_a_read_indefinitely() {
        let id = format!("http-selector-read-timeout-test-{}", std::process::id());
        let (ready, wait) = mpsc::sync_channel(1);
        let server_id = id.clone();
        let server = std::thread::spawn(move || {
            let pipe = ManagementPipe::create(&server_id).unwrap();
            ready.send(()).unwrap();
            pipe.accept(Duration::from_secs(3)).unwrap();
            let started = Instant::now();
            assert!(pipe.read_line(started + Duration::from_millis(40)).is_err());
            assert!(started.elapsed() < Duration::from_secs(2));
        });
        wait.recv_timeout(Duration::from_secs(3)).unwrap();
        let name = format!(r"\\.\pipe\KotorinToys.{}.{id}", current_user_sid().unwrap());
        let _client = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(name)
            .unwrap();
        server.join().unwrap();
    }

    #[test]
    fn url_queue_limit_refuses_without_overwriting_accepted_urls() {
        let shared = Shared::new();
        for index in 0..MAX_QUEUED_URLS {
            let url = ValidatedUrl::parse(format!("https://example.com/{index}")).unwrap();
            assert!(shared.receive_url(url));
        }
        assert!(!shared.receive_url(ValidatedUrl::parse("https://example.com/rejected").unwrap()));
        let mut queue = shared.queue.lock().unwrap();
        assert_eq!(queue.len(), MAX_QUEUED_URLS);
        for index in 0..MAX_QUEUED_URLS {
            let url = ValidatedUrl::parse(format!("https://example.com/{index}")).unwrap();
            assert_eq!(Shared::pop(&mut queue), Some(Event::Url(url)));
        }
    }
}
