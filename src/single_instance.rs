//! Только одна копия окна RigControl. Замок — локальный порт 127.0.0.1:PORT:
//! первая копия его занимает и слушает, вторая не может занять, стучится в первую
//! («покажи окно») и сразу выходит. Порт освобождается сам, даже если программа упала.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

const PORT: u16 = 47821;
const HELLO: &str = "RIGCONTROL-SHOW";
const REPLY: &str = "RIGCONTROL-OK";

pub enum Instance {
    /// Мы первые: держим замок и слушаем просьбы показать окно.
    First(TcpListener),
    /// Уже запущена другая копия RigControl — ей отправлена просьба показать окно.
    AlreadyRunning,
}

pub fn acquire() -> Instance {
    match TcpListener::bind(("127.0.0.1", PORT)) {
        Ok(l) => Instance::First(l),
        Err(_) => {
            if ping_existing() {
                Instance::AlreadyRunning
            } else {
                // Порт занят чем-то чужим — не мешаем запуску.
                match TcpListener::bind(("127.0.0.1", 0)) {
                    Ok(l) => Instance::First(l),
                    Err(_) => Instance::AlreadyRunning,
                }
            }
        }
    }
}

/// Стучимся в уже запущенную копию. true — это действительно RigControl.
fn ping_existing() -> bool {
    let Ok(mut s) = TcpStream::connect_timeout(&([127, 0, 0, 1], PORT).into(), Duration::from_millis(500)) else {
        return false;
    };
    let _ = s.set_read_timeout(Some(Duration::from_millis(1000)));
    if writeln!(s, "{HELLO}").is_err() {
        return false;
    }
    let mut line = String::new();
    BufReader::new(s).read_line(&mut line).is_ok() && line.trim() == REPLY
}

/// Фоновый поток: на каждую просьбу второй копии вызывает on_show.
pub fn listen(listener: TcpListener, on_show: impl Fn() + Send + 'static) {
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut s) = conn else { continue };
            let _ = s.set_read_timeout(Some(Duration::from_millis(1000)));
            let mut line = String::new();
            if BufReader::new(&s).read_line(&mut line).is_ok() && line.trim() == HELLO {
                let _ = writeln!(s, "{REPLY}");
                on_show();
            }
        }
    });
}
