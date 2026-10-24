#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use http_selector::app_options::parse_arguments;
use http_selector::app_paths::AppPaths;
use http_selector::app_title;
use http_selector::integrity::IntegrityManager;
use http_selector::management::{self, Event};
use http_selector::session::{NativeHost, Session, open_storage};
use http_selector::single_instance::{InstanceStatus, acquire};
use http_selector::ui::show_error;

fn main() {
    if let Err(error) = run() {
        show_error(&app_title::main(), &format!("{error:#}"));
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_SHIFT};
    let force_confirmation = unsafe { GetAsyncKeyState(VK_SHIFT as i32) } < 0;
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    let options = parse_arguments(&arguments)?;
    let startup = options.startup;
    let mut url = options
        .url
        .map(|url| url.with_forced_confirmation(force_confirmation));
    let _instance_guard = match acquire(url.as_ref())? {
        InstanceStatus::Primary(guard) => guard,
        InstanceStatus::Forwarded => return Ok(()),
    };
    let management = management::start(&options.origin)?;

    let paths = AppPaths::for_current_user()?;
    paths.ensure_data_dir()?;
    let integrity = IntegrityManager::new(&paths);
    let host = NativeHost {
        management: &management,
    };
    let Some(storage) = open_storage(&paths, &integrity, &host)? else {
        return Ok(());
    };
    management.set_ready(true);
    let mut display = !startup || url.is_some();
    loop {
        if display {
            Session::new(&paths, &storage, &integrity, &host, url.take())?.run()?;
        }
        // 管理用の常駐待機では次の要求を待ち、通常起動では受付済みの要求だけを処理する。
        match management
            .try_event()
            .or_else(|| startup.then(|| management.wait_event()))
        {
            Some(Event::Url(incoming)) => {
                url = Some(incoming);
                display = true;
            }
            Some(Event::Show) => {
                display = management.show_requested();
            }
            Some(Event::Stop) | None => break,
        }
    }
    Ok(())
}
