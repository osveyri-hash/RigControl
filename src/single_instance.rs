//! Только одна копия окна RigControl. Замок — локальный порт 127.0.0.1:PORT:
//! первая копия его занимает и слушает, вторая не может занять, стучится в первую
//! («покажи окно») и сразу выходит. Порт освобождается сам, даже если программа упала.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

const PORT: u16 = 47821;
const HELLO: &str = "RIGCONTROL-SHOW";
/// «Ты здесь?» — без просьбы показать окно (так фоновая служба узнаёт, что окно открыто).
const PING: &str = "RIGCONTROL-PING";
const REPLY: &str = "RIGCONTROL-OK";

pub enum Instance {
    /// Мы первые: держим замок и слушаем просьбы показать окно.
    First(TcpListener),
    /// Уже запущена другая копия RigControl — ей отправлена просьба показать окно.
    AlreadyRunning,
}

/// Кто держит порт окна.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Probe {
    /// RigControl: ответил как положено.
    Ours,
    /// Соединение принято, но ответа пока нет: окно ещё запускается (порт занят с первой строки main,
    /// а отвечать оно начинает позже — после того как фоновая служба отдаст устройства).
    Starting,
    /// Ответил кто-то другой или соединение сразу закрыли.
    Foreign,
    /// Не соединиться.
    Nobody,
}

pub fn acquire() -> Instance {
    match TcpListener::bind(("127.0.0.1", PORT)) {
        Ok(l) => Instance::First(l),
        Err(_) => match probe(PORT, HELLO, Duration::from_millis(1000)) {
            // Запускающейся копии просьба «покажи окно» не нужна — она и так его покажет.
            // Раньше «не ответила за секунду» считалось чужой программой, и открывалось второе окно.
            Probe::Ours | Probe::Starting => Instance::AlreadyRunning,
            // Порт занят чем-то чужим — не мешаем запуску.
            Probe::Foreign | Probe::Nobody => match TcpListener::bind(("127.0.0.1", 0)) {
                Ok(l) => Instance::First(l),
                Err(_) => Instance::AlreadyRunning,
            },
        },
    }
}

/// Окно RigControl открыто (или прямо сейчас запускается). Окно при этом не показывается.
pub fn window_running() -> bool {
    matches!(probe(PORT, PING, Duration::from_millis(700)), Probe::Ours | Probe::Starting)
}

/// Стучимся в порт окна словом hello и смотрим, кто там.
fn probe(port: u16, hello: &str, wait: Duration) -> Probe {
    let Ok(mut s) = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(500)) else {
        return Probe::Nobody;
    };
    let _ = s.set_read_timeout(Some(wait));
    if writeln!(s, "{hello}").is_err() {
        return Probe::Foreign;
    }
    let mut line = String::new();
    match BufReader::new(s).read_line(&mut line) {
        Ok(n) if n > 0 && line.trim() == REPLY => Probe::Ours,
        Ok(_) => Probe::Foreign,
        Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => Probe::Starting,
        Err(_) => Probe::Foreign,
    }
}

/// Фоновый поток: на каждую просьбу второй копии вызывает on_show.
pub fn listen(listener: TcpListener, on_show: impl Fn() + Send + 'static) {
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut s) = conn else { continue };
            let _ = s.set_read_timeout(Some(Duration::from_millis(1000)));
            let mut line = String::new();
            if BufReader::new(&s).read_line(&mut line).is_err() {
                continue;
            }
            match line.trim() {
                HELLO => {
                    let _ = writeln!(s, "{REPLY}");
                    on_show();
                }
                PING => {
                    let _ = writeln!(s, "{REPLY}");
                }
                _ => {}
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn local() -> (TcpListener, u16) {
        let l = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = l.local_addr().unwrap().port();
        (l, port)
    }

    #[test]
    fn running_window_answers_ping_without_showing() {
        let (l, port) = local();
        let shown = Arc::new(AtomicUsize::new(0));
        let n = shown.clone();
        // Ответ уходит раньше вызова on_show — ждём сам вызов, а не гадаем по счётчику.
        let (called_tx, called) = std::sync::mpsc::channel();
        listen(l, move || {
            n.fetch_add(1, Ordering::SeqCst);
            let _ = called_tx.send(());
        });
        assert_eq!(probe(port, PING, Duration::from_secs(2)), Probe::Ours);
        assert_eq!(probe(port, HELLO, Duration::from_secs(2)), Probe::Ours);
        called.recv_timeout(Duration::from_secs(5)).unwrap();
        // Запросы разбираются по очереди: PING уже обработан, и окно он не показывал.
        assert_eq!(shown.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn window_that_is_still_starting_is_not_foreign() {
        // Порт занят, но соединения ещё никто не разбирает — вторая копия не должна открыться.
        let (_l, port) = local();
        assert_eq!(probe(port, HELLO, Duration::from_millis(200)), Probe::Starting);
    }

    #[test]
    fn foreign_program_and_free_port() {
        let (l, port) = local();
        std::thread::spawn(move || {
            for mut s in l.incoming().flatten() {
                let _ = writeln!(s, "HTTP/1.1 400 Bad Request");
            }
        });
        assert_eq!(probe(port, HELLO, Duration::from_secs(2)), Probe::Foreign);
        let (l, free) = local();
        drop(l);
        assert_eq!(probe(free, HELLO, Duration::from_millis(200)), Probe::Nobody);
    }
}
