//! Связь окна с фоновой службой `rigcontrol-service.exe`.
//!
//! Служба ведёт подсветку, экран помпы и вентиляторы, пока окно закрыто, — без окна, с одним значком в трее,
//! поэтому почти не занимает памяти. Окно при открытии ставит службу на паузу (она отдаёт устройства и
//! закрывает своего помощника датчиков), а при закрытии отпускает её — служба продолжает с тех же настроек.
//! Пауза держится, пока открыто соединение: окно упало — соединение закрылось — служба продолжила сама.
//!
//! Обмен — строки по 127.0.0.1:PORT (этот же порт — замок «одна служба»):
//! `RIGCONTROL-SERVICE PING` → `OK`; `… PAUSE` → `PAUSED` (и держать соединение); `… QUIT` → `OK`.
//! `BUSY` вместо `PAUSED`/`OK` — служба не успела остановить устройства за отведённое время:
//! считать, что она их отдала, нельзя (иначе ими управляли бы два процесса сразу).

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

pub const PORT: u16 = 47822;
const MAGIC: &str = "RIGCONTROL-SERVICE";

pub const SERVICE_EXE: &str = if cfg!(windows) { "rigcontrol-service.exe" } else { "rigcontrol-service" };
pub const GUI_EXE: &str = if cfg!(windows) { "rigcontrol-gui.exe" } else { "rigcontrol-gui" };

/// Что попросили у службы.
pub enum Request {
    /// Отдать устройства (открылось окно); ответить в канал, когда отдала.
    Pause(Sender<()>),
    /// Окно закрылось — продолжить.
    Resume,
    /// Выйти; ответить, когда всё остановлено.
    Quit(Option<Sender<()>>),
}

/// Программа рядом с этой: служба или окно.
pub fn sibling(name: &str) -> Option<PathBuf> {
    let p = std::env::current_exe().ok()?.with_file_name(name);
    p.exists().then_some(p)
}

fn connect() -> Option<TcpStream> {
    let s = TcpStream::connect_timeout(&([127, 0, 0, 1], PORT).into(), Duration::from_millis(400)).ok()?;
    let _ = s.set_nodelay(true);
    Some(s)
}

/// Отправить команду и дождаться ответа. Возвращает соединение (для паузы) и ответ.
fn ask(cmd: &str, timeout: Duration) -> Option<(TcpStream, String)> {
    let mut s = connect()?;
    s.set_read_timeout(Some(timeout)).ok()?;
    writeln!(s, "{MAGIC} {cmd}").ok()?;
    let mut line = String::new();
    BufReader::new(s.try_clone().ok()?).read_line(&mut line).ok()?;
    Some((s, line.trim().to_string()))
}

/// Служба запущена.
pub fn running() -> bool {
    matches!(ask("PING", Duration::from_secs(1)), Some((_, r)) if r == "OK")
}

/// Попросить службу выйти (ответ — когда она всё остановила). false — службы нет.
pub fn quit() -> bool {
    matches!(ask("QUIT", Duration::from_secs(12)), Some((_, r)) if r == "OK")
}

/// Служба на паузе, пока жив этот объект (точнее, его соединение).
pub struct Pause {
    stream: TcpStream,
}

impl Pause {
    /// Окно закрывается без работы в фоне: служба выходит, а не продолжает.
    pub fn quit(mut self) {
        let _ = writeln!(self.stream, "{MAGIC} QUIT");
        let _ = self.stream.flush();
        let _ = self.stream.set_read_timeout(Some(Duration::from_secs(12)));
        let mut line = String::new();
        let _ = BufReader::new(&self.stream).read_line(&mut line);
    }
}

/// Поставить запущенную службу на паузу: ждём, пока она отдаст устройства (до 15 с).
/// Ok(None) — службы нет; Err — служба есть, но устройства не отдала (запускать свои потоки нельзя).
pub fn pause() -> Result<Option<Pause>, String> {
    let Some((s, r)) = ask("PAUSE", Duration::from_secs(15)) else { return Ok(None) };
    match r.as_str() {
        "PAUSED" => {
            let _ = s.set_read_timeout(None);
            Ok(Some(Pause { stream: s }))
        }
        "" => Err("служба не ответила за 15 с".into()),
        "BUSY" => Err("служба не успела остановить устройства".into()),
        other => Err(format!("неожиданный ответ «{other}»")),
    }
}

/// Запустить службу отдельным процессом (она переживёт закрытие окна).
pub fn spawn() -> Result<(), String> {
    let exe = sibling(SERVICE_EXE).ok_or_else(|| format!("нет {SERVICE_EXE} рядом с программой"))?;
    let mut cmd = std::process::Command::new(&exe);
    cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0000_0008 | 0x0000_0200); // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP
    }
    cmd.spawn().map(|_| ()).map_err(|e| format!("{}: {e}", exe.display()))
}

/// Занять порт службы. None — служба уже запущена.
pub fn bind() -> Option<TcpListener> {
    TcpListener::bind(("127.0.0.1", PORT)).ok()
}

/// Сторона службы: принимать соединения и передавать просьбы в канал.
pub fn serve(listener: TcpListener, tx: Sender<Request>) {
    let _ = std::thread::Builder::new().name("service-link".into()).spawn(move || {
        for conn in listener.incoming() {
            let Ok(s) = conn else { continue };
            let tx = tx.clone();
            let _ = std::thread::Builder::new().name("service-conn".into()).spawn(move || handle(s, tx));
        }
    });
}

fn command(line: &str) -> Option<&str> {
    line.trim().strip_prefix(MAGIC).map(str::trim)
}

fn handle(s: TcpStream, tx: Sender<Request>) {
    handle_with(s, tx, Duration::from_secs(14), Duration::from_secs(10));
}

/// pause_wait / quit_wait — сколько ждать, пока поток устройств всё остановит.
fn handle_with(mut s: TcpStream, tx: Sender<Request>, pause_wait: Duration, quit_wait: Duration) {
    let Ok(clone) = s.try_clone() else { return };
    let mut lines = BufReader::new(clone);
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    let mut line = String::new();
    if lines.read_line(&mut line).is_err() {
        return;
    }
    let quit = |s: &mut TcpStream| {
        let (q, done) = mpsc::channel();
        let _ = tx.send(Request::Quit(Some(q)));
        // «OK» — только когда устройства действительно остановлены.
        let reply = if done.recv_timeout(quit_wait).is_ok() { "OK" } else { "BUSY" };
        let _ = writeln!(s, "{reply}");
    };
    let cmd = command(&line).map(str::to_string);
    match cmd.as_deref() {
        Some("PING") => {
            let _ = writeln!(s, "OK");
        }
        Some("QUIT") => quit(&mut s),
        Some("PAUSE") => {
            let (ack, done) = mpsc::channel();
            if tx.send(Request::Pause(ack)).is_err() {
                return;
            }
            // Не успела остановить устройства — так и говорим и паузу снимаем: окно, получив «PAUSED»,
            // запустило бы свои потоки рядом с ещё работающими потоками службы.
            if done.recv_timeout(pause_wait).is_err() {
                let _ = tx.send(Request::Resume);
                let _ = writeln!(s, "BUSY");
                return;
            }
            if writeln!(s, "PAUSED").is_err() {
                let _ = tx.send(Request::Resume);
                return;
            }
            // Пауза — пока окно держит соединение.
            let _ = s.set_read_timeout(None);
            loop {
                line.clear();
                match lines.read_line(&mut line) {
                    Ok(0) | Err(_) => {
                        let _ = tx.send(Request::Resume);
                        return;
                    }
                    Ok(_) if command(&line) == Some("QUIT") => {
                        quit(&mut s);
                        return;
                    }
                    Ok(_) => {}
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::Receiver;

    /// Сервер на свободном порту: одно соединение, поток устройств изображает замыкание `devices`.
    fn serve_once(pause_wait: Duration, devices: impl FnOnce(Receiver<Request>) + Send + 'static) -> TcpStream {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || devices(rx));
        std::thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            handle_with(s, tx, pause_wait, pause_wait);
        });
        let s = TcpStream::connect(addr).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        s
    }

    fn reply(s: &TcpStream) -> String {
        let mut line = String::new();
        BufReader::new(s).read_line(&mut line).unwrap();
        line.trim().to_string()
    }

    #[test]
    fn pause_is_confirmed_only_after_devices_stop() {
        let (resumed_tx, resumed) = mpsc::channel();
        let mut s = serve_once(Duration::from_secs(5), move |rx| {
            for r in rx {
                match r {
                    Request::Pause(ack) => {
                        let _ = ack.send(());
                    }
                    Request::Resume => {
                        let _ = resumed_tx.send(());
                    }
                    Request::Quit(_) => {}
                }
            }
        });
        writeln!(s, "{MAGIC} PAUSE").unwrap();
        assert_eq!(reply(&s), "PAUSED");
        // Окно закрылось (соединение оборвано) — служба продолжает.
        drop(s);
        resumed.recv_timeout(Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn pause_that_did_not_finish_is_busy_and_resumes() {
        let (seen_tx, seen) = mpsc::channel();
        // Поток устройств «завис на остановке»: запрос получил, но не подтверждает.
        let mut s = serve_once(Duration::from_millis(200), move |rx| {
            let mut held = Vec::new();
            for r in rx {
                let _ = seen_tx.send(match &r {
                    Request::Pause(_) => "pause",
                    Request::Resume => "resume",
                    Request::Quit(_) => "quit",
                });
                held.push(r);
            }
        });
        writeln!(s, "{MAGIC} PAUSE").unwrap();
        assert_eq!(reply(&s), "BUSY");
        assert_eq!(seen.recv_timeout(Duration::from_secs(5)).unwrap(), "pause");
        assert_eq!(seen.recv_timeout(Duration::from_secs(5)).unwrap(), "resume");
    }

    #[test]
    fn commands() {
        assert_eq!(command("RIGCONTROL-SERVICE PAUSE\n"), Some("PAUSE"));
        assert_eq!(command("RIGCONTROL-SERVICE  QUIT"), Some("QUIT"));
        assert_eq!(command("GET / HTTP/1.1"), None);
    }
}
