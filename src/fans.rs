//! Управление вентиляторами платы — порт MainWindow.Fans.cs (UpdateFansAsync, FanTarget).
//!
//! Свой поток раз в секунду читает датчики платы через помощника LibreHardwareMonitor ([`crate::lhm`]),
//! считает скорость каждого канала по режиму из настроек и отдаёт её помощнику. Правила как в C#:
//! от 90 °C на CPU — 100 %; вниз скорость идёт плавно (не больше 3 % в секунду), вверх — сразу;
//! «Авто (BIOS)» возвращает канал плате. Программа вышла или упала — помощник сам отдаёт вентиляторы плате.
//!
//! Датчики платы опрашиваются, только пока открыта страница «Вентиляторы» или хоть один канал
//! не на «Авто» (FansWanted в C#).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use crate::fan_curve::{self, FAN_MIN_PERCENT, FAN_MODE_CURVE, FAN_MODE_FIXED, PRESETS};
use crate::lhm::{self, Fan};
use crate::settings::Settings;

/// Режим «Максимум».
const FAN_MODE_MAX: i32 = 4;

/// Режимы, скорости и кривые из settings.json — то, что нужно потоку.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FanConfig {
    pub mode_all: i32,
    pub modes: BTreeMap<String, i32>,
    pub percent: BTreeMap<String, i32>,
    pub curves: BTreeMap<String, Vec<i32>>,
}

impl FanConfig {
    pub fn from_settings(s: &Settings) -> Self {
        Self {
            mode_all: s.fan_mode_all,
            modes: s.fan_modes.clone().unwrap_or_default(),
            percent: s.fan_percent.clone().unwrap_or_default(),
            curves: s.fan_curves.clone().unwrap_or_default(),
        }
    }

    /// FanMode: свой режим канала, а если его не задавали — общий (0 — авто/BIOS).
    pub fn mode(&self, id: &str) -> i32 {
        self.modes.get(id).copied().unwrap_or(self.mode_all)
    }

    /// FanPercent: «Своя скорость», по умолчанию 60 %.
    pub fn percent(&self, id: &str) -> i32 {
        self.percent.get(id).copied().unwrap_or(60)
    }

    pub fn curve(&self, id: &str) -> Vec<(f64, f64)> {
        fan_curve::from_flat(self.curves.get(id))
    }

    /// Хоть один канал не на «Авто»: датчики платы нужны, даже когда окно в трее.
    pub fn any_manual(&self) -> bool {
        self.mode_all != 0 || self.modes.values().any(|&m| m != 0)
    }
}

/// FanTarget: скорость канала, % (20–100), или None — отдать плате.
pub fn fan_target(mode: i32, percent: i32, curve: &[(f64, f64)], temp: Option<f64>) -> Option<f64> {
    if mode <= 0 {
        return None;
    }
    if temp.is_some_and(|t| t >= 90.0) {
        return Some(100.0); // защита от перегрева
    }
    let value = match mode {
        FAN_MODE_MAX => 100.0,
        FAN_MODE_FIXED => f64::from(percent),
        FAN_MODE_CURVE => temp.map_or(80.0, |t| fan_curve::evaluate(curve, t)),
        _ => temp.map_or(80.0, |t| fan_curve::evaluate(PRESETS[mode.clamp(1, 3) as usize], t)),
    };
    Some(value.clamp(f64::from(FAN_MIN_PERCENT), 100.0))
}

/// Вниз — плавно (не больше 3 % за шаг), вверх — сразу.
pub fn smooth(target: Option<f64>, last: Option<f64>) -> Option<f64> {
    match (target, last) {
        (Some(t), Some(l)) if t < l => Some(t.max(l - 3.0)),
        _ => target,
    }
}

/// Что видно на странице «Вентиляторы».
#[derive(Clone, Debug, Default)]
pub struct FanState {
    /// Датчики платы сейчас опрашиваются.
    pub active: bool,
    /// Хоть раз прочитали.
    pub read: bool,
    pub admin: bool,
    pub board: Option<String>,
    pub cpu_temp: Option<f64>,
    pub fans: Vec<Fan>,
    pub error: Option<String>,
}

struct Shared {
    config: FanConfig,
    generation: u64,
    page: bool,
    state: FanState,
}

impl FanState {
    /// Одна строка для консоли и журнала: плата, сколько каналов под управлением, ошибка.
    /// Без оборотов и температуры, чтобы строка менялась только по делу.
    pub fn summary(&self) -> String {
        if let Some(e) = &self.error {
            return format!("датчики платы недоступны: {e}");
        }
        if !self.active {
            return "все на «Авто (BIOS)» — управляет плата".to_string();
        }
        if !self.read {
            return "читаю датчики платы…".to_string();
        }
        if !self.admin {
            return "нет прав администратора — скорость не меняется".to_string();
        }
        let board = self.board.as_deref().unwrap_or("плата");
        let controlled = self.fans.iter().filter(|f| f.controlled).count();
        format!("{board}: каналов {}, под управлением программы {controlled}", self.fans.len())
    }
}

/// Поток управления вентиляторами. Остановится, когда FanControl удалят.
pub struct FanControl {
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
}

fn lock(m: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl FanControl {
    pub fn start(s: &Settings) -> Self {
        let shared = Arc::new(Mutex::new(Shared {
            config: FanConfig::from_settings(s),
            generation: 0,
            page: false,
            state: FanState::default(),
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let (sh, st) = (shared.clone(), stop.clone());
        let _ = thread::Builder::new().name("fans".into()).spawn(move || worker(&sh, &st));
        Self { shared, stop }
    }

    /// Настройки поменялись (звать после сохранения). Новая скорость — сразу, без плавного спуска.
    pub fn configure(&self, s: &Settings) {
        let config = FanConfig::from_settings(s);
        let mut sh = lock(&self.shared);
        if sh.config != config {
            sh.config = config;
            sh.generation += 1;
        }
    }

    /// Звать каждый кадр: открыта ли страница «Вентиляторы».
    pub fn set_page_visible(&self, visible: bool) {
        lock(&self.shared).page = visible;
    }

    pub fn state(&self) -> FanState {
        lock(&self.shared).state.clone()
    }

    /// Остановить: в течение секунды поток вернёт каналы плате и выйдет.
    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for FanControl {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn worker(shared: &Mutex<Shared>, stop: &AtomicBool) {
    let mut last: BTreeMap<String, f64> = BTreeMap::new();
    let mut generation = u64::MAX;
    let mut applied = false;
    let mut ids: Vec<String> = Vec::new();
    while !stop.load(Ordering::Relaxed) {
        let (config, current, page) = {
            let sh = lock(shared);
            (sh.config.clone(), sh.generation, sh.page)
        };
        if current != generation {
            generation = current;
            last.clear();
        }
        if page || applied || config.any_manual() {
            let state = tick(&config, &mut last, &mut applied, &mut ids);
            lock(shared).state = state;
        } else {
            lock(shared).state.active = false;
        }
        for _ in 0..10 {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
    // Окно закрыли: отдать каналы плате.
    if applied {
        let restore: BTreeMap<String, Option<f64>> = ids.into_iter().map(|id| (id, None)).collect();
        let _ = lhm::apply_fans(&restore);
    }
}

fn tick(config: &FanConfig, last: &mut BTreeMap<String, f64>, applied: &mut bool, ids: &mut Vec<String>) -> FanState {
    let mut st = FanState { active: true, ..Default::default() };
    let r = match lhm::read(true) {
        Ok(r) => r,
        Err(e) => {
            st.error = Some(e);
            return st;
        }
    };
    st.read = true;
    st.admin = r.admin;
    st.board = r.board.clone().filter(|b| !b.trim().is_empty() && !b.trim().eq_ignore_ascii_case("unknown"));
    st.cpu_temp = r.cpu_temp;
    let fans = r.fans.unwrap_or_default();

    let mut targets = BTreeMap::new();
    for f in &fans {
        let target = fan_target(config.mode(&f.id), config.percent(&f.id), &config.curve(&f.id), r.cpu_temp);
        let target = smooth(target, last.get(&f.id).copied());
        match target {
            Some(v) => {
                last.insert(f.id.clone(), v);
            }
            None => {
                last.remove(&f.id);
            }
        }
        targets.insert(f.id.clone(), target);
    }
    if !targets.is_empty() {
        if let Err(e) = lhm::apply_fans(&targets) {
            st.error = Some(e);
        }
    }
    *applied = targets.values().any(Option::is_some);
    *ids = fans.iter().map(|f| f.id.clone()).collect();
    st.fans = fans;
    st
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_like_csharp() {
        let curve = fan_curve::DEFAULT_CURVE.to_vec();
        assert_eq!(fan_target(0, 60, &curve, Some(50.0)), None); // Авто — плате
        assert_eq!(fan_target(4, 60, &curve, Some(30.0)), Some(100.0)); // Максимум
        assert_eq!(fan_target(5, 10, &curve, Some(30.0)), Some(20.0)); // Своя скорость, не ниже 20 %
        assert_eq!(fan_target(5, 60, &curve, Some(95.0)), Some(100.0)); // от 90 °C — 100 %
        assert_eq!(fan_target(6, 60, &curve, Some(50.0)), Some(40.0)); // Своя кривая: 50 °C → 40 %
        assert_eq!(fan_target(6, 60, &curve, None), Some(80.0)); // нет температуры — 80 %
        assert_eq!(fan_target(2, 60, &curve, Some(60.0)), Some(50.0)); // «Обычный»: 60 °C → 50 %
        assert_eq!(fan_target(9, 60, &curve, Some(40.0)), Some(50.0)); // неизвестный — как «Производительный»
    }

    #[test]
    fn smooth_down_only() {
        assert_eq!(smooth(Some(40.0), Some(60.0)), Some(57.0));
        assert_eq!(smooth(Some(80.0), Some(60.0)), Some(80.0));
        assert_eq!(smooth(None, Some(60.0)), None);
        assert_eq!(smooth(Some(40.0), None), Some(40.0));
    }

    #[test]
    fn config_modes() {
        let mut s = Settings::default();
        s.fan_mode_all = 0;
        let c = FanConfig::from_settings(&s);
        assert!(!c.any_manual());
        assert_eq!(c.mode("x"), 0);
        assert_eq!(c.percent("x"), 60);
        s.fan_modes = Some(BTreeMap::from([("x".to_string(), 6)]));
        let c = FanConfig::from_settings(&s);
        assert!(c.any_manual());
        assert_eq!(c.mode("x"), 6);
        assert_eq!(c.mode("y"), 0);
    }
}
