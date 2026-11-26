//! Фоновая служба RigControl: подсветка клавиатуры и СЖО, экран помпы и вентиляторы — пока окно закрыто.
//! Без окна: только значок в трее («Открыть RigControl», «Выход»), поэтому памяти почти не занимает.
//! Окно при открытии ставит службу на паузу и само ведёт устройства, при закрытии — отпускает
//! (см. [`rigcontrol_core::service_link`]). Автозапуск с Windows запускает именно службу.
//!
//! `rigcontrol-service.exe --quit` — попросить запущенную службу выйти (нужно install.bat).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rigcontrol_core::host::Devices;
use rigcontrol_core::memory;
use rigcontrol_core::service_link::{self, Request};
use rigcontrol_core::single_instance;

fn main() {
    if std::env::args().any(|a| a == "--quit") {
        service_link::quit();
        return;
    }
    // Порт связи заодно замок: вторая копия службы просто выходит.
    let Some(listener) = service_link::bind() else { return };
    let (tx, rx) = mpsc::channel::<Request>();
    service_link::serve(listener, tx.clone());

    let tip = Arc::new(Mutex::new("RigControl".to_string()));
    let tip2 = tip.clone();
    let ui_thread = tray::thread_id();
    let control = std::thread::Builder::new()
        .name("devices".into())
        .spawn(move || control(rx, tip2, ui_thread))
        .ok();
    tray::run(tx, tip);
    if let Some(c) = control {
        let _ = c.join();
    }
}

/// Поток устройств: запускает их, отдаёт окну на время паузы, останавливает при выходе.
fn control(rx: Receiver<Request>, tip: Arc<Mutex<String>>, ui_thread: u32) {
    // Окно уже открыто (служба стартовала позже него — например, автозапуск с задержкой 10 с, а окно
    // открыли сразу после входа): устройства ведёт оно, и паузу оно уже не попросит. Не запускаем свои
    // потоки рядом с его потоками, а ждём, пока окно закроется.
    let mut devices = (!single_instance::window_running()).then(Devices::start);
    let mut paused = 0usize;
    let mut trim_at = Instant::now() + Duration::from_secs(20);
    loop {
        match rx.recv_timeout(Duration::from_secs(2)) {
            Ok(Request::Pause(ack)) => {
                paused += 1;
                if let Some(d) = devices.take() {
                    d.stop();
                }
                let _ = ack.send(());
            }
            Ok(Request::Resume) => {
                paused = paused.saturating_sub(1);
                if paused == 0 && devices.is_none() && !single_instance::window_running() {
                    devices = Some(Devices::start());
                    trim_at = Instant::now() + Duration::from_secs(15);
                }
            }
            Ok(Request::Quit(ack)) => {
                if let Some(d) = devices.take() {
                    d.stop();
                }
                if let Some(a) = ack {
                    let _ = a.send(());
                }
                tray::quit(ui_thread);
                return;
            }
            Err(RecvTimeoutError::Timeout) => {
                // Паузы нет, но устройства не наши — их вело окно, открытое до запуска службы. Закрылось — берём.
                if paused == 0 && devices.is_none() && !single_instance::window_running() {
                    devices = Some(Devices::start());
                    trim_at = Instant::now() + Duration::from_secs(15);
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                if let Some(d) = devices.take() {
                    d.stop();
                }
                tray::quit(ui_thread);
                return;
            }
        }
        let text = match &devices {
            None => "RigControl · открыто окно".to_string(),
            Some(d) => match d.battery() {
                Some(p) => format!("RigControl · клавиатура {p} %"),
                None => "RigControl".to_string(),
            },
        };
        *tip.lock().unwrap_or_else(|e| e.into_inner()) = text;
        // Отдавать системе неиспользуемые страницы: служба работает сутками.
        if Instant::now() >= trim_at {
            memory::trim();
            trim_at = Instant::now() + Duration::from_secs(120);
        }
    }
}

/// Открыть окно (оно поставит службу на паузу; если уже открыто — просто покажется).
fn open_gui() {
    if let Some(exe) = service_link::sibling(service_link::GUI_EXE) {
        let _ = std::process::Command::new(exe).spawn();
    }
}

fn logo_rgba(size: u32) -> Option<Vec<u8>> {
    let img = image::load_from_memory_with_format(include_bytes!("../../assets/logo.png"), image::ImageFormat::Png).ok()?;
    Some(image::imageops::resize(&img.to_rgba8(), size, size, image::imageops::FilterType::Triangle).into_raw())
}

#[cfg(windows)]
mod tray {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// MSG из WinUser.h (48 байт на x64).
    #[repr(C)]
    #[allow(dead_code)]
    struct Msg {
        hwnd: isize,
        message: u32,
        wparam: usize,
        lparam: isize,
        time: u32,
        pt: [i32; 2],
        private: u32,
    }

    const WM_TIMER: u32 = 0x0113;
    /// Своё сообщение потоку значка: «выйти из цикла».
    const WM_APP_QUIT: u32 = 0x8000 + 1;

    #[link(name = "user32")]
    extern "system" {
        fn GetMessageW(msg: *mut Msg, hwnd: isize, min: u32, max: u32) -> i32;
        fn TranslateMessage(msg: *const Msg) -> i32;
        fn DispatchMessageW(msg: *const Msg) -> isize;
        fn PostThreadMessageW(thread: u32, msg: u32, wparam: usize, lparam: isize) -> i32;
        fn SetTimer(hwnd: isize, id: usize, ms: u32, func: Option<unsafe extern "system" fn(isize, u32, usize, u32)>) -> usize;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentThreadId() -> u32;
    }

    pub fn thread_id() -> u32 {
        unsafe { GetCurrentThreadId() }
    }

    /// Пора выходить. Сообщение потоку может потеряться: пока открыто меню значка, сообщения потока
    /// (без окна) забирает и выбрасывает модальный цикл меню. Поэтому выход ещё и помечается флагом,
    /// а цикл проверяет его на каждом сообщении — в том числе по таймеру раз в 2 с.
    static QUIT: AtomicBool = AtomicBool::new(false);

    pub fn quit(thread: u32) {
        QUIT.store(true, Ordering::SeqCst);
        unsafe {
            PostThreadMessageW(thread, WM_APP_QUIT, 0, 0);
        }
    }

    fn make(tx: Sender<Request>) -> Option<tray_icon::TrayIcon> {
        use tray_icon::menu::{Menu, MenuEvent, MenuItem};
        use tray_icon::{Icon, TrayIconBuilder, TrayIconEvent};
        let menu = Menu::new();
        let open = MenuItem::new("Открыть RigControl", true, None);
        let quit = MenuItem::new("Выход", true, None);
        menu.append(&open).ok()?;
        menu.append(&quit).ok()?;
        let (open_id, quit_id) = (open.id().clone(), quit.id().clone());
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            if e.id == quit_id {
                let _ = tx.send(Request::Quit(None));
            } else if e.id == open_id {
                open_gui();
            }
        }));
        TrayIconEvent::set_event_handler(Some(|e: TrayIconEvent| {
            if matches!(e, TrayIconEvent::DoubleClick { .. }) {
                open_gui();
            }
        }));
        let icon = Icon::from_rgba(logo_rgba(32)?, 32, 32).ok()?;
        TrayIconBuilder::new().with_menu(Box::new(menu)).with_tooltip("RigControl").with_icon(icon).build().ok()
    }

    /// Значок в трее и цикл сообщений Windows (без него значок не отвечает). Возвращается при выходе.
    pub fn run(tx: Sender<Request>, tip: Arc<Mutex<String>>) {
        let icon = make(tx);
        unsafe {
            SetTimer(0, 0, 2000, None);
        }
        let mut shown = String::new();
        let mut msg: Msg = unsafe { std::mem::zeroed() };
        loop {
            if unsafe { GetMessageW(&mut msg, 0, 0, 0) } <= 0 {
                break;
            }
            if QUIT.load(Ordering::SeqCst) || (msg.hwnd == 0 && msg.message == WM_APP_QUIT) {
                break;
            }
            if msg.hwnd == 0 && msg.message == WM_TIMER {
                let text = tip.lock().unwrap_or_else(|e| e.into_inner()).clone();
                if text != shown {
                    if let Some(i) = &icon {
                        let _ = i.set_tooltip(Some(text.clone()));
                    }
                    shown = text;
                }
                continue;
            }
            unsafe {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        drop(icon);
    }
}

#[cfg(not(windows))]
mod tray {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    static QUIT: AtomicBool = AtomicBool::new(false);

    pub fn thread_id() -> u32 {
        0
    }

    pub fn quit(_thread: u32) {
        QUIT.store(true, Ordering::Relaxed);
    }

    /// Без трея: просто ждём выхода.
    pub fn run(_tx: Sender<Request>, _tip: Arc<Mutex<String>>) {
        let _ = (open_gui as fn(), logo_rgba as fn(u32) -> Option<Vec<u8>>);
        while !QUIT.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(200));
        }
    }
}
