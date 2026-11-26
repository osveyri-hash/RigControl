//! MainWindow.Battery.cs: заряд клавиатуры по радио, раз в 15 секунд (первый замер через 3 с).
//! По кабелю заряд не узнать — клавиатура сообщает его только по радио.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::dongle::Ak820Dongle as Dongle;

pub const LOW_PERCENT: u8 = 15;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BatteryState {
    /// None — клавиатура заряд не сообщила: индикатор прячем, а не рисуем выдуманные 100 %.
    pub level: Option<u8>,
    /// Пора предупредить (один раз за разряд, снова — после зарядки выше LOW+10).
    pub warn: bool,
}

pub struct BatteryWatch {
    state: Arc<Mutex<BatteryState>>,
    stop: Arc<AtomicBool>,
}

/// Логика «один раз за разряд» из ShowBattery, отдельно для тестов.
pub fn next_warned(warned: bool, percent: u8) -> (bool, bool) {
    if percent <= LOW_PERCENT && !warned {
        (true, true) // (warned, показать уведомление)
    } else if percent > LOW_PERCENT + 10 {
        (false, false)
    } else {
        (warned, false)
    }
}

impl BatteryWatch {
    pub fn start(repaint: Option<Box<dyn Fn() + Send>>) -> Self {
        let state = Arc::new(Mutex::new(BatteryState::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let (st, sp) = (state.clone(), stop.clone());
        std::thread::Builder::new()
            .name("battery".into())
            .spawn(move || {
                let mut warned = false;
                let mut last: Option<u8> = None;
                let mut wait = Duration::from_secs(3);
                loop {
                    let mut slept = Duration::ZERO;
                    while slept < wait {
                        if sp.load(Ordering::Relaxed) {
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(250));
                        slept += Duration::from_millis(250);
                    }
                    wait = Duration::from_secs(15);
                    let level = read_once();
                    match level {
                        Some(l) => last = Some(l),
                        // нет донгла — прячем; не ответила один раз — держим прошлое (могла задремать)
                        None if !dongle_present() => last = None,
                        None => {}
                    }
                    let mut warn = false;
                    if let Some(p) = last {
                        let (w, show) = next_warned(warned, p);
                        warned = w;
                        warn = show;
                    }
                    if let Ok(mut s) = st.lock() {
                        s.level = last;
                        s.warn = s.warn || warn;
                    }
                    if let Some(r) = &repaint {
                        r();
                    }
                }
            })
            .ok();
        Self { state, stop }
    }

    pub fn state(&self) -> BatteryState {
        self.state.lock().map(|s| *s).unwrap_or_default()
    }

    /// Забрать флажок «предупредить» (сбрасывается после показа).
    pub fn take_warning(&self) -> Option<u8> {
        let mut s = self.state.lock().ok()?;
        if s.warn {
            s.warn = false;
            s.level
        } else {
            None
        }
    }
}

impl Drop for BatteryWatch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn dongle_present() -> bool {
    hidapi::HidApi::new().map(|api| Dongle::present(&api)).unwrap_or(false)
}

fn read_once() -> Option<u8> {
    let api = hidapi::HidApi::new().ok()?;
    if !Dongle::present(&api) {
        return None;
    }
    let mut d = Dongle::open(&api).ok()?;
    d.read_battery_for(3000).ok().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn warn_once_per_discharge() {
        let (w, show) = next_warned(false, 14);
        assert!(w && show);
        let (w, show) = next_warned(w, 10);
        assert!(w && !show);
        let (w, _) = next_warned(w, 20);
        assert!(w); // 20 <= 25 — ещё не зарядилась
        let (w, _) = next_warned(w, 26);
        assert!(!w);
    }
}
