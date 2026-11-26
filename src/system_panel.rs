//! Страница «О системе» — порт MainWindow.System.cs.
//!
//! Характеристики ПК и паспорт модели (SystemInfo.cs), Curve Optimizer по ядрам и тайминги памяти
//! (ZenTuning.cs, только чтение через PawnIO) — их отдаёт помощник датчиков ([`crate::lhm`]).
//! Таблицы CPU / GPU: сейчас, пик за запись и паспортное значение; средние под нагрузкой от 90 % (LoadStats),
//! «Запись в фоне», «Сбросить», советы по андервольтингу для известных моделей.
//!
//! Датчики опрашиваются в отдельном потоке раз в секунду, пока открыта страница или включена запись
//! (SystemWanted в C#); Curve Optimizer и тайминги — при открытии страницы, не чаще раза в 30 с.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, RichText};

use crate::lhm::{DramTimings, SpecRow, SystemSpecs, ZenReport};
use crate::telemetry::{Snapshot, Telemetry};
use crate::ui_kit as kit;

/// С какой загрузки CPU/GPU считаем «под нагрузкой» (SysLoadThreshold в C#).
pub const LOAD_THRESHOLD: f64 = 90.0;

const GB: f64 = 1024.0 * 1024.0 * 1024.0;
const TEXT: Color32 = kit::TEXT;
const MUTED: Color32 = kit::MUTED;
const BAD: Color32 = kit::BAD;

// Форматы как в MainWindow.System.cs (Pct, Deg, Volt, MilliVolt, Ghz, Mhz, Watt, Rpm, Duration).
pub fn pct(v: f64) -> String { format!("{v:.0} %") }
pub fn deg(v: f64) -> String { format!("{v:.0} °C") }
pub fn volt(v: f64) -> String { format!("{v:.3} В") }
pub fn millivolt(v: f64) -> String { format!("{:.0} мВ", v * 1000.0) }
pub fn ghz(mhz: f64) -> String { format!("{:.2} ГГц", mhz / 1000.0) }
pub fn mhz(v: f64) -> String { format!("{v:.0} МГц") }
pub fn watt(v: f64) -> String { format!("{v:.0} Вт") }
pub fn rpm(v: f64) -> String { format!("{v:.0} об/мин") }
pub fn gb(v: f64) -> String { format!("{v:.1} ГБ") }
pub fn duration(seconds: u32) -> String {
    if seconds < 60 { format!("{seconds} с") } else { format!("{} мин {} с", seconds / 60, seconds % 60) }
}

/// «−10», «+5», «0» (Signed в C#).
pub fn signed(v: i32) -> String {
    match v {
        v if v > 0 => format!("+{v}"),
        v if v < 0 => format!("−{}", -v),
        _ => "0".into(),
    }
}

/// Средние под нагрузкой: частота, напряжение, мощность и самая высокая температура (LoadStats).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LoadStats {
    clock: f64,
    volt: f64,
    power: f64,
    clock_n: u32,
    volt_n: u32,
    power_n: u32,
    pub seconds: u32,
    pub temp_max: Option<f64>,
}

fn sum(value: Option<f64>, total: &mut f64, count: &mut u32) {
    if let Some(v) = value {
        if v.is_finite() {
            *total += v;
            *count += 1;
        }
    }
}

impl LoadStats {
    pub fn add(&mut self, clock: Option<f64>, volt: Option<f64>, power: Option<f64>, temp: Option<f64>) {
        self.seconds += 1;
        sum(clock, &mut self.clock, &mut self.clock_n);
        sum(volt, &mut self.volt, &mut self.volt_n);
        sum(power, &mut self.power, &mut self.power_n);
        if let Some(t) = temp {
            if t.is_finite() {
                self.temp_max = Some(self.temp_max.map_or(t, |m| m.max(t)));
            }
        }
    }
    pub fn clock(&self) -> Option<f64> { (self.clock_n > 0).then(|| self.clock / self.clock_n as f64) }
    pub fn volt(&self) -> Option<f64> { (self.volt_n > 0).then(|| self.volt / self.volt_n as f64) }
    pub fn power(&self) -> Option<f64> { (self.power_n > 0).then(|| self.power / self.power_n as f64) }
}

/// Строка таблицы: текущее значение, пик за запись и паспортное значение модели (SysMetric).
pub struct Metric {
    pub label: &'static str,
    pub get: fn(&Snapshot) -> Option<f64>,
    pub format: fn(f64) -> String,
    pub peak: bool,
    pub spec: fn(&SystemSpecs) -> Option<String>,
}

fn none(_: &SystemSpecs) -> Option<String> { None }
fn m_cpu_load(s: &Snapshot) -> Option<f64> { Some(s.cpu_load) }
fn m_cpu_temp(s: &Snapshot) -> Option<f64> { s.cpu_temp }
fn m_cpu_clock(s: &Snapshot) -> Option<f64> { s.cpu_clock_mhz }
fn m_cpu_power(s: &Snapshot) -> Option<f64> { s.cpu_power_w }
fn m_cpu_vcore(s: &Snapshot) -> Option<f64> { s.cpu_vcore }
fn m_cpu_clock_max(s: &Snapshot) -> Option<f64> { s.cpu_clock_max_mhz }
fn m_gpu_hot_spot(s: &Snapshot) -> Option<f64> { s.gpu_hot_spot }
fn m_gpu_mem_temp(s: &Snapshot) -> Option<f64> { s.gpu_mem_temp }
fn m_gpu_clock(s: &Snapshot) -> Option<f64> { s.gpu_core_clock_mhz }
fn m_gpu_mem_clock(s: &Snapshot) -> Option<f64> { s.gpu_mem_clock_mhz }
fn m_gpu_volt(s: &Snapshot) -> Option<f64> { s.gpu_core_volt }
fn m_gpu_load(s: &Snapshot) -> Option<f64> { s.gpu_load }
fn m_gpu_temp(s: &Snapshot) -> Option<f64> { s.gpu_temp }
fn m_gpu_power(s: &Snapshot) -> Option<f64> { s.gpu_power_w }
fn m_gpu_fan(s: &Snapshot) -> Option<f64> { s.gpu_fan_rpm }

fn s_cpu_temp(s: &SystemSpecs) -> Option<String> { s.cpu.as_ref().map(|c| format!("предел {:.0} °C", c.tj_max_c)) }
fn s_cpu_base(s: &SystemSpecs) -> Option<String> { s.cpu.as_ref().map(|c| format!("база {}", ghz(c.base_mhz))) }
fn s_cpu_boost(s: &SystemSpecs) -> Option<String> { s.cpu.as_ref().map(|c| format!("буст до {}", ghz(c.boost_mhz))) }
fn s_cpu_ppt(s: &SystemSpecs) -> Option<String> { s.cpu.as_ref().map(|c| format!("PPT {:.0} Вт", c.ppt_w)) }
fn s_gpu_hot(s: &SystemSpecs) -> Option<String> { s.gpu.as_ref().map(|g| format!("предел {:.0} °C", g.hot_spot_limit_c)) }
fn s_gpu_volt(s: &SystemSpecs) -> Option<String> { s.gpu.as_ref().map(|g| format!("сток до {:.0} мВ", g.max_volt_mv)) }
fn s_gpu_boost(s: &SystemSpecs) -> Option<String> { s.gpu.as_ref().map(|g| format!("буст {:.0} МГц", g.boost_mhz)) }
fn s_gpu_mem(s: &SystemSpecs) -> Option<String> { s.gpu.as_ref().map(|g| format!("{:.0} МГц", g.mem_clock_mhz)) }
fn s_gpu_tbp(s: &SystemSpecs) -> Option<String> { s.gpu.as_ref().map(|g| format!("TBP {:.0} Вт", g.board_power_w)) }

pub const CPU_METRICS: &[Metric] = &[
    Metric { label: "Загрузка", get: m_cpu_load, format: pct, peak: false, spec: none },
    Metric { label: "Температура", get: m_cpu_temp, format: deg, peak: true, spec: s_cpu_temp },
    Metric { label: "Напряжение ядер (Vcore)", get: m_cpu_vcore, format: volt, peak: true, spec: none },
    Metric { label: "Частота, средняя", get: m_cpu_clock, format: ghz, peak: true, spec: s_cpu_base },
    Metric { label: "Частота, лучшее ядро", get: m_cpu_clock_max, format: ghz, peak: true, spec: s_cpu_boost },
    Metric { label: "Мощность (Package)", get: m_cpu_power, format: watt, peak: true, spec: s_cpu_ppt },
];

/// Номер строки «Напряжение ядра» в GPU_METRICS: по её пику видно, применён ли андервольт.
const GPU_VOLT_ROW: usize = 4;

pub const GPU_METRICS: &[Metric] = &[
    Metric { label: "Загрузка", get: m_gpu_load, format: pct, peak: false, spec: none },
    Metric { label: "Температура ядра", get: m_gpu_temp, format: deg, peak: true, spec: none },
    Metric { label: "Хотспот", get: m_gpu_hot_spot, format: deg, peak: true, spec: s_gpu_hot },
    Metric { label: "Температура памяти", get: m_gpu_mem_temp, format: deg, peak: true, spec: none },
    Metric { label: "Напряжение ядра", get: m_gpu_volt, format: millivolt, peak: true, spec: s_gpu_volt },
    Metric { label: "Частота ядра", get: m_gpu_clock, format: mhz, peak: true, spec: s_gpu_boost },
    Metric { label: "Частота памяти", get: m_gpu_mem_clock, format: mhz, peak: true, spec: s_gpu_mem },
    Metric { label: "Мощность", get: m_gpu_power, format: watt, peak: true, spec: s_gpu_tbp },
    Metric { label: "Вентилятор", get: m_gpu_fan, format: rpm, peak: true, spec: none },
];

fn all_metrics() -> Vec<&'static Metric> {
    CPU_METRICS.iter().chain(GPU_METRICS.iter()).collect()
}

#[derive(Default)]
struct State {
    last: Option<Snapshot>,
    peaks: Vec<Option<f64>>,
    cpu: LoadStats,
    gpu: LoadStats,
    specs: Option<SystemSpecs>,
    zen: Option<ZenReport>,
    zen_at: Option<Instant>,
    zen_loading: bool,
}

impl State {
    fn push(&mut self, s: Snapshot) {
        let all = all_metrics();
        if self.peaks.len() != all.len() {
            self.peaks = vec![None; all.len()];
        }
        for (i, m) in all.iter().enumerate() {
            if !m.peak {
                continue;
            }
            if let Some(v) = (m.get)(&s) {
                if v.is_finite() {
                    self.peaks[i] = Some(self.peaks[i].map_or(v, |p| p.max(v)));
                }
            }
        }
        if s.cpu_load >= LOAD_THRESHOLD {
            self.cpu.add(s.cpu_clock_mhz, s.cpu_vcore, s.cpu_power_w, s.cpu_temp);
        }
        if s.gpu_load.map_or(false, |l| l >= LOAD_THRESHOLD) {
            self.gpu.add(s.gpu_core_clock_mhz, s.gpu_core_volt, s.gpu_power_w, s.gpu_hot_spot.or(s.gpu_temp));
        }
        self.last = Some(s);
    }

    fn reset(&mut self) {
        self.peaks.clear();
        self.cpu = LoadStats::default();
        self.gpu = LoadStats::default();
    }
}

fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(|p| p.into_inner())
}

/// «Curve Optimizer −20 на всех ядрах. » / «… от −25 до −15 по ядрам. »
fn co_prefix(zen: Option<&ZenReport>) -> String {
    match zen.and_then(|z| z.curve_optimizer.as_ref()).filter(|v| !v.is_empty()) {
        Some(v) if v.iter().all(|x| *x == v[0]) => format!("Curve Optimizer {} на всех ядрах. ", signed(v[0])),
        Some(v) => format!(
            "Curve Optimizer от {} до {} по ядрам. ",
            signed(*v.iter().min().unwrap_or(&0)),
            signed(*v.iter().max().unwrap_or(&0))
        ),
        None => String::new(),
    }
}

/// Текст «Процессор под нагрузкой…» (CpuSummary).
pub fn cpu_summary(st: &LoadStats, zen: Option<&ZenReport>) -> String {
    let co = co_prefix(zen);
    if st.seconds < 5 {
        return co + "Процессор: нагрузки от 90 % ещё не было — запустите Cinebench R23 или OCCT.";
    }
    let mut parts = Vec::new();
    if let Some(c) = st.clock() { parts.push(ghz(c)); }
    if let Some(v) = st.volt() { parts.push(volt(v)); }
    if let Some(p) = st.power() { parts.push(watt(p)); }
    if let Some(t) = st.temp_max { parts.push(format!("до {}", deg(t))); }
    let mut text = format!("{co}Процессор под нагрузкой ({}): {}.", duration(st.seconds), parts.join(" · "));
    if let (Some(c), Some(p)) = (st.clock(), st.power()) {
        if p > 1.0 {
            text += &format!(" Эффективность {:.0} МГц/Вт.", c / p);
        }
    }
    text
}

/// Текст «Видеокарта под нагрузкой…» (GpuSummary) с проверкой андервольта по пику напряжения.
pub fn gpu_summary(st: &LoadStats, volt_peak: Option<f64>, specs: Option<&SystemSpecs>) -> String {
    if st.seconds < 5 {
        return "Видеокарта: нагрузки от 90 % ещё не было — запустите игру или 3DMark.".into();
    }
    let mut parts = Vec::new();
    if let Some(c) = st.clock() { parts.push(mhz(c)); }
    if let Some(v) = st.volt() { parts.push(millivolt(v)); }
    if let Some(p) = st.power() { parts.push(watt(p)); }
    if let Some(t) = st.temp_max { parts.push(format!("до {}", deg(t))); }
    let mut text = format!("Видеокарта под нагрузкой ({}): {}.", duration(st.seconds), parts.join(" · "));
    if let (Some(c), Some(p)) = (st.clock(), st.power()) {
        if p > 1.0 {
            text += &format!(" Эффективность {:.1} МГц/Вт.", c / p);
        }
    }
    // Андервольт виден по пику напряжения: стоковая кривая хоть раз доходит до своей верхней точки.
    if let (Some(pv), Some(spec)) = (volt_peak, specs.and_then(|s| s.gpu.as_ref())) {
        if st.seconds >= 30 {
            let mv = pv * 1000.0;
            let below = spec.max_volt_mv - mv;
            text += &if below >= 15.0 {
                format!(" Пик напряжения {mv:.0} мВ — на {below:.0} мВ ниже стоковых {:.0} мВ: андервольт работает.", spec.max_volt_mv)
            } else {
                format!(" Пик напряжения {mv:.0} мВ — как в стоке ({:.0} мВ): андервольт не применён.", spec.max_volt_mv)
            };
        }
    }
    text
}

/// Запасной вариант, если помощник датчиков не ответил: что знает sysinfo.
fn local_specs() -> SystemSpecs {
    let mut sys = sysinfo::System::new();
    sys.refresh_cpu_all();
    sys.refresh_memory();
    let cpu = sys.cpus().first().map(|c| c.brand().trim().to_string()).filter(|s| !s.is_empty());
    let mut rows = vec![SpecRow {
        label: "Процессор".into(),
        value: cpu.clone().unwrap_or_else(|| "Не определён".into()),
        detail: Some(format!("потоков: {}", sys.cpus().len())),
    }];
    rows.push(SpecRow { label: "Оперативная память".into(), value: format!("{:.0} ГБ", (sys.total_memory() as f64 / GB).ceil()), detail: None });
    rows.push(SpecRow { label: "Windows".into(), value: sysinfo::System::long_os_version().unwrap_or_else(|| "—".into()), detail: None });
    SystemSpecs { rows, cpu_name: cpu, ..Default::default() }
}

fn worker(state: Arc<Mutex<State>>, wanted: Arc<AtomicBool>, visible: Arc<AtomicBool>, stop: Arc<AtomicBool>) {
    let specs = crate::lhm::specs().unwrap_or_else(|_| local_specs());
    lock(&state).specs = Some(specs);
    let mut tel = Telemetry::new();
    tel.read_board = true; // Vcore — с чипа мониторинга платы
    while !stop.load(Ordering::Relaxed) {
        if wanted.load(Ordering::Relaxed) {
            let s = tel.sample();
            lock(&state).push(s);
        }
        // Curve Optimizer и тайминги меняются только из BIOS: при открытой странице, не чаще раза в 30 с.
        let need_zen = visible.load(Ordering::Relaxed) && {
            let st = lock(&state);
            st.zen_at.map_or(true, |t| t.elapsed() > Duration::from_secs(30))
        };
        if need_zen {
            lock(&state).zen_loading = true;
            let zen = crate::lhm::zen().unwrap_or_else(|e| ZenReport {
                co_note: Some(format!("Не удалось прочитать: {e}")),
                timings_note: Some(format!("Не удалось прочитать: {e}")),
                ..Default::default()
            });
            let mut st = lock(&state);
            st.zen = Some(zen);
            st.zen_at = Some(Instant::now());
            st.zen_loading = false;
        }
        thread::sleep(Duration::from_secs(1));
    }
}

const TIPS_CPU: &str = concat!(
    "Процессор (AMD Ryzen): BIOS → Precision Boost Overdrive → Curve Optimizer → All Cores → Negative. ",
    "Начните с −10 и уменьшайте шагами по 5, после каждого шага — 15 минут OCCT или Cinebench R23. ",
    "Ошибки или перезагрузки — верните предыдущий шаг. Intel: смещение Vcore в BIOS или Intel XTU, шаг −10 мВ."
);
const TIPS_GPU: &str = concat!(
    "Видеокарта: AMD Software → Производительность → Настройка → Ручная, или MSI Afterburner → Ctrl+F (кривая). ",
    "Снижайте напряжение по 10–25 мВ и проверяйте 15–20 минут в игре или 3DMark."
);
const LOAD_HINT: &str = concat!(
    "Запустите игру или тест на 10–15 минут и вернитесь сюда. Пики и средние под нагрузкой (от 90 %) ",
    "копятся, пока открыта эта страница, а с записью в фоне — и на других страницах или в свёрнутом окне."
);

/// Страница «О системе».
pub struct SystemPanel {
    state: Arc<Mutex<State>>,
    wanted: Arc<AtomicBool>,
    visible_flag: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    started: bool,
    visible: bool,
    /// «Запись в фоне»: копить пики и средние, даже когда страница закрыта или окно в трее.
    pub record: bool,
}

impl Default for SystemPanel {
    fn default() -> Self { Self::new() }
}

impl SystemPanel {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(State::default())),
            wanted: Arc::new(AtomicBool::new(false)),
            visible_flag: Arc::new(AtomicBool::new(false)),
            stop: Arc::new(AtomicBool::new(false)),
            started: false,
            visible: false,
            record: false,
        }
    }

    /// Звать каждый кадр: открыта ли сейчас страница «О системе».
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
        self.visible_flag.store(visible, Ordering::Relaxed);
        self.update_wanted();
    }

    fn update_wanted(&mut self) {
        let want = self.visible || self.record;
        if want && !self.started {
            self.started = true;
            let (st, w, v, s) = (self.state.clone(), self.wanted.clone(), self.visible_flag.clone(), self.stop.clone());
            let _ = thread::Builder::new().name("sysinfo".into()).spawn(move || worker(st, w, v, s));
        }
        self.wanted.store(want, Ordering::Relaxed);
    }

    pub fn reset(&mut self) {
        lock(&self.state).reset();
    }

    /// Нарисовать страницу (свои карточки). Возвращает сообщение для всплывашки.
    pub fn ui(&mut self, ui: &mut egui::Ui) -> Option<String> {
        ui.ctx().request_repaint_after(Duration::from_secs(1));
        let mut msg = None;
        let (last, peaks, cpu, gpu, specs, zen, zen_loading) = {
            let st = lock(&self.state);
            (st.last.clone(), st.peaks.clone(), st.cpu.clone(), st.gpu.clone(), st.specs.clone(), st.zen.clone(), st.zen_loading)
        };
        let gap = 16.0;
        let wide = ui.available_width() >= 900.0;
        let half = if wide { ((ui.available_width() - gap) / 2.0).floor() } else { ui.available_width() };

        // ── Характеристики ──
        kit::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            title(ui, "Характеристики");
            match &specs {
                None => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        kit::cap(ui, "Определяю…");
                    });
                }
                Some(sp) => spec_rows(ui, &sp.rows),
            }
        });
        ui.add_space(gap);

        // ── Curve Optimizer и тайминги памяти ──
        two(ui, wide, half, gap,
            |ui| {
                kit::card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    title(ui, "Curve Optimizer");
                    co_card(ui, zen.as_ref(), zen_loading);
                });
            },
            |ui| {
                kit::card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    title(ui, "Тайминги памяти");
                    timings_card(ui, zen.as_ref(), zen_loading);
                });
            },
        );
        ui.add_space(gap);

        // ── Андервольтинг: запись, итоги под нагрузкой ──
        kit::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                title(ui, "Андервольтинг");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Сбросить").on_hover_text("Обнулить пики и средние").clicked() {
                        self.reset();
                        msg = Some("Пики и средние сброшены".to_string());
                    }
                    ui.add_space(8.0);
                    if ui.checkbox(&mut self.record, "Запись в фоне").changed() {
                        self.update_wanted();
                    }
                });
            });
            kit::cap(ui, if self.record {
                "Запись идёт, пока окно открыто (его можно свернуть). После теста выключите: датчики опрашиваются, пока запись включена."
            } else {
                LOAD_HINT
            });
            ui.add_space(10.0);
            ui.label(RichText::new(cpu_summary(&cpu, zen.as_ref())).color(TEXT));
            ui.add_space(4.0);
            let volt_peak = peaks.get(CPU_METRICS.len() + GPU_VOLT_ROW).copied().flatten();
            ui.label(RichText::new(gpu_summary(&gpu, volt_peak, specs.as_ref())).color(TEXT));
            match &last {
                Some(s) => {
                    if let Some(note) = &s.sensors_note {
                        ui.add_space(6.0);
                        ui.label(RichText::new(format!("Датчики процессора не читаются: {note}.")).size(12.5).color(BAD));
                    }
                    let vcore = match s.cpu_vcore_source.as_deref() {
                        Some("SVI") => "Vcore — телеметрия регулятора напряжения (SVI).",
                        Some("плата") => "Vcore — с чипа мониторинга материнской платы.",
                        Some("VID") => "Показан VID — напряжение, которое запрашивает процессор; реальное обычно немного ниже.",
                        _ => "",
                    };
                    if !vcore.is_empty() {
                        ui.add_space(4.0);
                        kit::cap(ui, vcore);
                    }
                }
                None => {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.spinner();
                        kit::cap(ui, "Читаю датчики…");
                    });
                }
            }
        });
        ui.add_space(gap);

        // ── Таблицы: сейчас, пик, паспорт ──
        let cpu_title = specs.as_ref().and_then(|s| s.cpu_name.clone())
            .or_else(|| last.as_ref().and_then(|s| s.cpu_name.clone()))
            .unwrap_or_else(|| "Процессор".into());
        let gpu_title = specs.as_ref().and_then(|s| s.gpu_name.clone())
            .or_else(|| last.as_ref().and_then(|s| s.gpu_name.clone()))
            .unwrap_or_else(|| "Видеокарта".into());
        two(ui, wide, half, gap,
            |ui| {
                kit::card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    title(ui, &cpu_title);
                    metric_table(ui, "sys_cpu", CPU_METRICS, last.as_ref(), &peaks, 0, specs.as_ref());
                });
            },
            |ui| {
                kit::card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    title(ui, &gpu_title);
                    metric_table(ui, "sys_gpu", GPU_METRICS, last.as_ref(), &peaks, CPU_METRICS.len(), specs.as_ref());
                });
            },
        );
        ui.add_space(gap);

        // ── Как андервольтить: советы под свою модель (паспорт) или общие ──
        kit::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            title(ui, "Как андервольтить");
            let cpu_tips = specs.as_ref().and_then(|s| s.cpu.as_ref()).map(|c| c.tips.clone()).unwrap_or_else(|| TIPS_CPU.to_string());
            let gpu_tips = specs.as_ref().and_then(|s| s.gpu.as_ref()).map(|g| g.tips.clone()).unwrap_or_else(|| TIPS_GPU.to_string());
            ui.label(RichText::new(cpu_tips).color(TEXT));
            ui.add_space(8.0);
            ui.label(RichText::new(gpu_tips).color(TEXT));
        });
        msg
    }
}

impl Drop for SystemPanel {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn title(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).font(kit::semibold(15.0)).color(TEXT));
    ui.add_space(8.0);
}

/// Две карточки рядом (широкое окно) или друг под другом.
fn two(ui: &mut egui::Ui, wide: bool, half: f32, gap: f32, left: impl FnOnce(&mut egui::Ui), right: impl FnOnce(&mut egui::Ui)) {
    if wide {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            ui.vertical(|ui| {
                ui.set_width(half);
                left(ui);
            });
            ui.vertical(|ui| {
                ui.set_width(half);
                right(ui);
            });
        });
    } else {
        left(ui);
        ui.add_space(gap);
        right(ui);
    }
}

fn spec_rows(ui: &mut egui::Ui, rows: &[SpecRow]) {
    egui::Grid::new("sys_specs").num_columns(2).spacing([24.0, 10.0]).min_col_width(170.0).show(ui, |ui| {
        for r in rows {
            ui.label(RichText::new(&r.label).size(12.5).color(MUTED));
            ui.vertical(|ui| {
                ui.label(RichText::new(&r.value).font(kit::semibold(14.0)).color(TEXT));
                if let Some(d) = r.detail.as_deref().filter(|d| !d.trim().is_empty()) {
                    ui.add(egui::Label::new(RichText::new(d).size(12.0).color(MUTED)).wrap());
                }
            });
            ui.end_row();
        }
    });
}

fn loading(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.spinner();
        kit::cap(ui, "Читаю из процессора…");
    });
}

/// Карточка Curve Optimizer (DrawZen в C#): значение, пояснение, ядра — если значения разные.
fn co_card(ui: &mut egui::Ui, zen: Option<&ZenReport>, loading_now: bool) {
    let Some(z) = zen else {
        if loading_now { loading(ui) } else { kit::cap(ui, "Откройте страницу ещё раз через пару секунд."); }
        return;
    };
    match z.curve_optimizer.as_ref().filter(|v| !v.is_empty()) {
        Some(co) => {
            let same = co.iter().all(|v| *v == co[0]);
            let (min, max) = (*co.iter().min().unwrap_or(&0), *co.iter().max().unwrap_or(&0));
            let value = if same { signed(co[0]) } else { format!("{}…{}", signed(min), signed(max)) };
            ui.label(RichText::new(value).font(kit::semibold(30.0)).color(TEXT));
            ui.add_space(4.0);
            let avg = co.iter().sum::<i32>() as f64 / co.len() as f64;
            kit::cap(ui, if co.iter().all(|v| *v == 0) {
                "Андервольт не настроен: на всех ядрах 0.".to_string()
            } else if same {
                format!("На всех {} ядрах. Отрицательное значение — андервольт: на той же частоте ниже напряжение.", co.len())
            } else {
                format!("По ядрам, в среднем {}. Отрицательное значение — андервольт.", signed(avg.round() as i32))
            });
            if !same {
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                    for (i, v) in co.iter().enumerate() {
                        egui::Frame::none().fill(kit::SURFACE2).rounding(6.0).inner_margin(egui::Margin::symmetric(8.0, 3.0)).show(ui, |ui| {
                            ui.label(RichText::new(format!("Ядро {i}: {}", signed(*v))).size(12.5).color(TEXT));
                        });
                    }
                });
            }
        }
        None => {
            ui.label(RichText::new("—").font(kit::semibold(30.0)).color(TEXT));
            kit::cap(ui, z.co_note.clone().unwrap_or_default());
        }
    }
}

/// Карточка таймингов: «DDR5-6000 · CL30-36-36-76», режимы, сетка таймингов в тактах.
fn timings_card(ui: &mut egui::Ui, zen: Option<&ZenReport>, loading_now: bool) {
    let Some(z) = zen else {
        if loading_now { loading(ui) } else { kit::cap(ui, "Откройте страницу ещё раз через пару секунд."); }
        return;
    };
    let Some(t) = &z.timings else {
        ui.label(RichText::new("—").font(kit::semibold(19.0)).color(TEXT));
        kit::cap(ui, z.timings_note.clone().unwrap_or_default());
        return;
    };
    ui.label(RichText::new(format!("DDR5-{} · CL{}-{}-{}-{}", t.data_rate, t.cl, t.rcd_rd, t.rp, t.ras)).font(kit::semibold(19.0)).color(TEXT));
    ui.add_space(4.0);
    let on = |b: bool| if b { "вкл" } else { "выкл" };
    kit::cap(ui, format!(
        "MEMCLK {:.0} МГц · Command Rate {} · Gear Down {} · Power Down {}. Тайминги в тактах, как в BIOS и ZenTimings.",
        t.mem_clk_mhz, if t.cmd2t { "2T" } else { "1T" }, on(t.gdm), on(t.power_down)
    ));
    ui.add_space(8.0);
    let cells = timing_cells(t);
    let cols = ((ui.available_width() / 96.0).floor() as usize).clamp(3, 6);
    egui::Grid::new("sys_timings").num_columns(cols).spacing([12.0, 8.0]).min_col_width(80.0).show(ui, |ui| {
        for (n, (label, value)) in cells.iter().enumerate() {
            ui.vertical(|ui| {
                ui.label(RichText::new(*label).size(12.0).color(MUTED));
                ui.label(RichText::new(value).font(kit::semibold(14.0)).color(TEXT));
            });
            if n % cols == cols - 1 {
                ui.end_row();
            }
        }
    });
}

/// Ячейки сетки таймингов, как в DrawZen.
pub fn timing_cells(t: &DramTimings) -> Vec<(&'static str, String)> {
    let dash = |v: u32, s: String| if v > 0 { s } else { "—".to_string() };
    vec![
        ("tCL", t.cl.to_string()), ("tRCD RD", t.rcd_rd.to_string()), ("tRCD WR", t.rcd_wr.to_string()),
        ("tRP", t.rp.to_string()), ("tRAS", t.ras.to_string()), ("tRC", t.rc.to_string()),
        ("tRRD_S", t.rrds.to_string()), ("tRRD_L", t.rrdl.to_string()), ("tFAW", t.faw.to_string()),
        ("tWR", t.wr.to_string()), ("tWTR_S", t.wtrs.to_string()), ("tWTR_L", t.wtrl.to_string()),
        ("tRTP", t.rtp.to_string()), ("tCWL", t.cwl.to_string()),
        ("tRFC", dash(t.rfc, format!("{} ({:.0} нс)", t.rfc, t.ns(t.rfc)))),
        ("tRFC2", dash(t.rfc2, t.rfc2.to_string())),
        ("tRFCsb", dash(t.rfc_sb, t.rfc_sb.to_string())),
        ("tREFI", dash(t.refi, format!("{} ({:.1} мкс)", t.refi, t.ns(t.refi) / 1000.0))),
    ]
}

fn metric_table(
    ui: &mut egui::Ui, id: &str, metrics: &[Metric], last: Option<&Snapshot>, peaks: &[Option<f64>], offset: usize,
    specs: Option<&SystemSpecs>,
) {
    let w = ui.available_width();
    egui::Grid::new(id).num_columns(4).striped(true).spacing([10.0, 8.0]).min_col_width(w * 0.18).show(ui, |ui| {
        ui.label("");
        ui.label(RichText::new("Сейчас").size(12.0).color(MUTED));
        ui.label(RichText::new("Пик за запись").size(12.0).color(MUTED));
        ui.label(RichText::new(if specs.is_some_and(|s| s.cpu.is_some() || s.gpu.is_some()) { "Паспорт" } else { "" }).size(12.0).color(MUTED));
        ui.end_row();
        for (i, m) in metrics.iter().enumerate() {
            let now = last.and_then(|s| (m.get)(s)).filter(|v| v.is_finite()).map(m.format).unwrap_or_else(|| "—".into());
            let top = if m.peak {
                peaks.get(offset + i).copied().flatten().map(m.format).unwrap_or_else(|| "—".into())
            } else {
                String::new()
            };
            let spec = specs.and_then(|s| (m.spec)(s)).unwrap_or_default();
            ui.label(RichText::new(m.label).color(MUTED));
            ui.label(RichText::new(now).font(kit::semibold(14.0)).color(TEXT));
            ui.label(RichText::new(top).color(TEXT));
            ui.label(RichText::new(spec).size(12.5).color(MUTED));
            ui.end_row();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_stats_average_and_max() {
        let mut s = LoadStats::default();
        s.add(Some(4000.0), None, Some(100.0), Some(70.0));
        s.add(Some(5000.0), None, None, Some(80.0));
        s.add(None, None, Some(f64::NAN), Some(75.0));
        assert_eq!(s.seconds, 3);
        assert_eq!(s.clock(), Some(4500.0));
        assert_eq!(s.power(), Some(100.0));
        assert_eq!(s.volt(), None);
        assert_eq!(s.temp_max, Some(80.0));
    }

    #[test]
    fn summaries() {
        let mut s = LoadStats::default();
        assert!(cpu_summary(&s, None).starts_with("Процессор: нагрузки"));
        let zen = ZenReport { curve_optimizer: Some(vec![-20; 6]), ..Default::default() };
        assert!(cpu_summary(&s, Some(&zen)).starts_with("Curve Optimizer −20 на всех ядрах. "));
        for _ in 0..65 {
            s.add(Some(4500.0), None, Some(90.0), Some(81.0));
        }
        let t = cpu_summary(&s, None);
        assert!(t.contains("1 мин 5 с"), "{t}");
        assert!(t.contains("4.50 ГГц"), "{t}");
        assert!(t.contains("Эффективность 50 МГц/Вт"), "{t}");
    }

    #[test]
    fn gpu_undervolt_check() {
        let mut s = LoadStats::default();
        for _ in 0..40 {
            s.add(Some(1900.0), Some(1.05), Some(180.0), Some(80.0));
        }
        let specs = SystemSpecs { gpu: Some(crate::lhm::GpuSpec { max_volt_mv: 1200.0, ..Default::default() }), ..Default::default() };
        assert!(gpu_summary(&s, Some(1.1), Some(&specs)).contains("андервольт работает"));
        assert!(gpu_summary(&s, Some(1.195), Some(&specs)).contains("не применён"));
    }

    #[test]
    fn signed_like_csharp() {
        assert_eq!(signed(-10), "−10");
        assert_eq!(signed(5), "+5");
        assert_eq!(signed(0), "0");
    }

    #[test]
    fn peaks_skip_load() {
        let mut st = State::default();
        st.push(Snapshot { cpu_load: 95.0, cpu_temp: Some(70.0), ..Default::default() });
        st.push(Snapshot { cpu_load: 50.0, cpu_temp: Some(60.0), ..Default::default() });
        assert_eq!(st.peaks[0], None); // загрузка CPU — без пика
        assert_eq!(st.peaks[1], Some(70.0));
        assert_eq!(st.cpu.seconds, 1);
    }

    #[test]
    fn timing_cells_have_all() {
        let t = DramTimings { data_rate: 6000, cl: 30, rfc: 884, refi: 11700, ..Default::default() };
        let c = timing_cells(&t);
        assert_eq!(c.len(), 18);
        assert_eq!(c[14].1, "884 (295 нс)");
        assert_eq!(c[15].1, "—");
        assert_eq!(c[17].1, "11700 (3.9 мкс)");
    }
}
