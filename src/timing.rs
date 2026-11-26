//! Плавные анимации: порт TimerResolution.cs + ровный шаг кадров.
//!
//! Почему подсветка дёргалась: по умолчанию Windows будит потоки раз в ~15,6 мс, и `sleep(2)`/`sleep(5)`
//! между пакетами живого кадра превращались в ~15 мс каждый. Кадр клавиатуры (заголовок, 8 пакетов,
//! завершение) уходил за ~180 мс вместо ~45 — получалось ~5 кадров/с с рваным шагом.
//! [`HighResTimer`] включает точный таймер 1 мс (timeBeginPeriod), как C#-версия, а [`Pacer`]
//! держит постоянный шаг по абсолютным отметкам времени, без накопления опозданий.

use std::time::{Duration, Instant};

#[cfg(windows)]
mod win {
    use std::ffi::c_void;

    #[link(name = "winmm")]
    extern "system" {
        pub fn timeBeginPeriod(period: u32) -> u32;
        pub fn timeEndPeriod(period: u32) -> u32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        pub fn GetCurrentProcess() -> *mut c_void;
        pub fn SetProcessInformation(process: *mut c_void, class: i32, info: *mut c_void, size: u32) -> i32;
    }

    #[repr(C)]
    pub struct PowerThrottlingState {
        pub version: u32,
        pub control_mask: u32,
        pub state_mask: u32,
    }

    pub const PROCESS_POWER_THROTTLING: i32 = 4;
    pub const POWER_THROTTLING_CURRENT_VERSION: u32 = 1;
    pub const POWER_THROTTLING_IGNORE_TIMER_RESOLUTION: u32 = 0x4;
}

/// Точный таймер Windows (1 мс), пока жив объект. На других ОС ничего не делает.
pub struct HighResTimer {
    #[cfg(windows)]
    active: bool,
}

impl HighResTimer {
    pub fn begin() -> Self {
        #[cfg(windows)]
        unsafe {
            // Windows 11 игнорирует запрос точного таймера у свёрнутых/фоновых процессов —
            // просим выполнять его всегда. На Windows 10 вызов просто вернёт 0.
            let mut state = win::PowerThrottlingState {
                version: win::POWER_THROTTLING_CURRENT_VERSION,
                control_mask: win::POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
                state_mask: 0,
            };
            win::SetProcessInformation(
                win::GetCurrentProcess(),
                win::PROCESS_POWER_THROTTLING,
                &mut state as *mut _ as *mut std::ffi::c_void,
                std::mem::size_of::<win::PowerThrottlingState>() as u32,
            );
            let active = win::timeBeginPeriod(1) == 0;
            HighResTimer { active }
        }
        #[cfg(not(windows))]
        HighResTimer {}
    }
}

impl Drop for HighResTimer {
    fn drop(&mut self) {
        #[cfg(windows)]
        if self.active {
            unsafe {
                win::timeEndPeriod(1);
            }
        }
    }
}

/// Спать до момента `deadline` с точностью ~0,1 мс: обычный сон до последних ~1,5 мс, дальше — короткое ожидание.
pub fn sleep_until(deadline: Instant) {
    loop {
        let now = Instant::now();
        if now >= deadline {
            return;
        }
        let left = deadline - now;
        if left > Duration::from_micros(1500) {
            std::thread::sleep(left - Duration::from_micros(1000));
        } else {
            std::thread::yield_now();
        }
    }
}

/// Ровный шаг кадров: следующая отметка = прошлая + шаг (а не «сейчас + шаг»),
/// поэтому небольшие задержки отправки не копятся. Если отстали больше чем на кадр — догоняем без «пачки» кадров.
pub struct Pacer {
    step: Duration,
    next: Instant,
}

impl Pacer {
    pub fn new(step: Duration) -> Self {
        Pacer { step, next: Instant::now() + step }
    }

    pub fn wait(&mut self) {
        sleep_until(self.next);
        self.next += self.step;
        let now = Instant::now();
        if self.next < now {
            self.next = now + self.step;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pacer_keeps_cadence() {
        let _t = HighResTimer::begin();
        let start = Instant::now();
        let mut p = Pacer::new(Duration::from_millis(10));
        for _ in 0..10 {
            std::thread::sleep(Duration::from_millis(3)); // «работа» кадра
            p.wait();
        }
        let ms = start.elapsed().as_millis();
        assert!((95..=140).contains(&ms), "10 кадров по 10 мс заняли {ms} мс");
    }
}
