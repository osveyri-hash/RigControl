//! Подсветка клавиатуры в своём потоке — то, что в C# делали ApplyLightingAsync, StartLightSync,
//! StartCustomLight, StartGifKeysLight и таймер слежения за кабелем (MainWindow.xaml.cs):
//! - встроенный эффект и «Своя раскраска» выставляются один раз (раскраска — и по радио);
//! - свои эффекты (100–109) и анимация раскраски рисуются живыми кадрами 20 раз в секунду — только по кабелю;
//!   по радио вместо своего эффекта горит ближайший встроенный;
//! - «Подсветка в такт гифке» (SyncLight): «Картинка» и «Волна от экрана» — кадрами по кабелю ([`GifLight`]),
//!   «Один цвет кадра» — выбранный эффект перекрашивается в главный цвет кадра (и по радио);
//!   свои эффекты при синхронизации берут цвет текущего кадра; «Гифка → справа налево» — правый край кадров;
//! - перед потоком кадров в клавиатуру один раз сохраняется похожий обычный эффект: им она светит,
//!   когда программу закрыли или ПК уснул;
//! - «Выключить» гасит нулевой яркостью (режим 0 в прошивке «залипает»), «Включить» возвращает как было;
//! - раз в 3 секунды проверяем кабель: воткнули — переходим на провод, выдернули — на радио, без перезапуска.
//!
//! Весь обмен с клавиатурой идёт из одного потока, поэтому команды не перемешиваются.
//! Гифку поток читает сам (LastImage, Crop, MaxFrames из настроек), так что всё это работает и в `rigcontrol daemon`.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::gif_light::{self, GifLight};
use crate::keyboard::{self, Keyboard};
use crate::protocol::{self, ScreenFrame};
use crate::settings::Settings;
use crate::{effects, keypaint, screen, timing, Rgb};

/// Что показать на странице клавиатуры, плитке и в боковой панели.
#[derive(Clone, Debug, Default)]
pub struct KbState {
    /// Ok — как подключена («по кабелю USB-C» / «по радио: …»), Err — почему не найдена. None — ещё ищем.
    pub link: Option<Result<String, String>>,
    /// Программа сама ведёт подсветку: живые кадры или смена цвета в такт гифке.
    pub live: bool,
    /// Подсветка выключена кнопкой «Выключить».
    pub off: bool,
    /// Пояснение: что не получилось или чем заменено (например, свой эффект по радио).
    pub note: Option<String>,
    /// Последнее событие: «Часы выставлены…», ошибка подсветки.
    pub event: Option<(Instant, String)>,
}

impl KbState {
    pub fn wired(&self) -> bool {
        matches!(&self.link, Some(Ok(how)) if how.starts_with("по кабелю"))
    }
    pub fn connected(&self) -> bool {
        matches!(&self.link, Some(Ok(_)))
    }
}

enum Cmd {
    /// Применить настройки. force_on — снять «Выключено» (кнопка «Применить»).
    Apply(Box<Settings>, bool),
    /// Выключить (false) или включить (true).
    Light(bool),
    Time,
    Reconnect,
}

pub struct KbLight {
    tx: Sender<Cmd>,
    shared: Arc<Mutex<KbState>>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

fn lock(m: &Mutex<KbState>) -> std::sync::MutexGuard<'_, KbState> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl KbLight {
    /// Запустить поток и сразу выставить подсветку по настройкам.
    pub fn start(s: &Settings) -> Self {
        let (tx, rx) = mpsc::channel();
        let shared = Arc::new(Mutex::new(KbState::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let (sh, st) = (shared.clone(), stop.clone());
        let handle = std::thread::Builder::new()
            .name("keyboard".into())
            .spawn(move || Worker::new(sh, st).run(rx))
            .ok();
        let _ = tx.send(Cmd::Apply(Box::new(s.clone()), false));
        Self { tx, shared, stop, handle }
    }

    /// Настройки подсветки поменялись. Пока подсветка выключена кнопкой — только запомнить.
    pub fn apply(&self, s: &Settings) {
        let _ = self.tx.send(Cmd::Apply(Box::new(s.clone()), false));
    }

    /// «Применить»: выставить и снять «Выключено».
    pub fn apply_now(&self, s: &Settings) {
        let _ = self.tx.send(Cmd::Apply(Box::new(s.clone()), true));
    }

    pub fn set_on(&self, on: bool) {
        let _ = self.tx.send(Cmd::Light(on));
    }

    pub fn sync_time(&self) {
        let _ = self.tx.send(Cmd::Time);
    }

    /// «Найти клавиатуру»: открыть заново и выставить подсветку.
    pub fn reconnect(&self) {
        let _ = self.tx.send(Cmd::Reconnect);
    }

    pub fn state(&self) -> KbState {
        lock(&self.shared).clone()
    }
}

impl KbLight {
    /// Остановить поток сейчас (окно закрывается и отдаёт клавиатуру службе).
    pub fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for KbLight {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// Кабель AK820 Pro (интерфейс команд 0xFF13) сейчас подключён.
fn cable_present() -> bool {
    hidapi::HidApi::new()
        .map(|api| {
            api.device_list().any(|i| {
                i.vendor_id() == protocol::VENDOR_ID
                    && protocol::PRODUCT_IDS.contains(&i.product_id())
                    && i.usage_page() == protocol::CONTROL_USAGE_PAGE
            })
        })
        .unwrap_or(false)
}

fn now_parts() -> protocol::DateTimeParts {
    use chrono::{Datelike, Timelike};
    let now = chrono::Local::now();
    protocol::DateTimeParts {
        year: now.year(),
        month: now.month() as u8,
        day: now.day() as u8,
        hour: now.hour() as u8,
        minute: now.minute() as u8,
        second: now.second() as u8,
        weekday: now.weekday().num_days_from_sunday() as u8,
    }
}

fn hex(c: Rgb) -> String {
    format!("#{:02X}{:02X}{:02X}", c.0, c.1, c.2)
}

/// Средний цвет (AverageOf в C#), с отбрасыванием дробной части.
fn average_of(colors: &[Rgb]) -> Rgb {
    let n = colors.len().max(1) as u64;
    let (r, g, b) = colors.iter().fold((0u64, 0u64, 0u64), |a, c| (a.0 + u64::from(c.0), a.1 + u64::from(c.1), a.2 + u64::from(c.2)));
    ((r / n) as u8, (g / n) as u8, (b / n) as u8)
}

/// Цвета кадров гифки и когда кончается каждый кадр — для своих эффектов «в такт гифке».
pub struct Palette {
    colors: Vec<Rgb>,
    cumulative: Vec<i32>,
    loop_ms: i32,
}

impl Palette {
    pub fn from_frames(frames: &[ScreenFrame]) -> Self {
        let average: Vec<Rgb> = frames.iter().map(|f| gif_light::dominant(&f.rgb565)).collect();
        let colors = protocol::vivid_palette(&average);
        let mut cumulative = Vec::with_capacity(colors.len());
        let mut sum = 0i32;
        for f in frames.iter().take(colors.len()) {
            sum += f.delay_ms.max(20);
            cumulative.push(sum);
        }
        Self { colors, cumulative, loop_ms: sum }
    }

    /// Цвет кадра в момент seconds (PaletteColor в C#).
    pub fn at(&self, seconds: f64, fallback: Rgb) -> Rgb {
        if self.loop_ms <= 0 {
            return fallback;
        }
        let at = ((seconds * 1000.0) as i64 % i64::from(self.loop_ms)) as i32;
        match self.cumulative.iter().position(|&v| at < v) {
            Some(i) if i < self.colors.len() => self.colors[i],
            _ => fallback,
        }
    }

    pub fn average(&self) -> Rgb {
        average_of(&self.colors)
    }
}

/// Откуда берутся живые кадры.
pub enum Source {
    /// Свой эффект или анимация раскраски по настройкам; palette — цвет кадров гифки (синхронизация).
    Effect { palette: Option<Palette> },
    /// «Гифка → справа налево»: полосы правого края кадров и когда кончается каждый кадр.
    Flow { strips: Vec<Vec<Rgb>>, cumulative: Vec<i32>, loop_ms: i32 },
    /// «Картинка» и «Волна от экрана».
    Gif(Box<GifLight>),
}

/// Кадр в момент t секунд (таблица 576 байт, как Build/BuildFlow/Animate в C#).
pub fn live_bank(s: &Settings, src: &Source, t: f64) -> Vec<u8> {
    let brightness = s.brightness.clamp(0, 5);
    let speed = s.speed.clamp(0, 5);
    let mode = protocol::effect_mode(s.effect);
    let color = protocol::parse_color(Some(&s.color), (255, 255, 255));
    let mut bank = match src {
        Source::Gif(g) => return g.build(t, brightness, speed),
        Source::Flow { strips, cumulative, loop_ms } => effects::build_flow(strips, cumulative, *loop_ms, t, speed),
        Source::Effect { palette } => {
            if mode == effects::PAINT {
                let paint: keypaint::Paint = s.key_paint.clone().unwrap_or_default().into_iter().collect();
                let anim = s.paint_anim.clamp(0, keypaint::ANIMATIONS.len() as i32 - 1);
                return keypaint::animate(Some(&paint), brightness, anim, t, s.paint_speed);
            }
            let c = palette.as_ref().map_or(color, |p| p.at(t, color));
            effects::build(mode, t, speed, s.direction, c)
        }
    };
    if brightness < 5 {
        effects::dim(&mut bank, f64::from(brightness) / 5.0);
    }
    bank
}

struct Live {
    src: Source,
    start: Instant,
    pacer: timing::Pacer,
    _timer: timing::HighResTimer,
    failures: u32,
}

/// «Один цвет кадра»: выбранный эффект перекрашивается в главный цвет кадра гифки (StartLightSync в C#).
struct SyncColor {
    mode: i32,
    speed: i32,
    direction: i32,
    brightness: i32,
    tones: Vec<(f64, f64, f64)>,
    starts: Vec<i64>,
    delays: Vec<i64>,
    total: i64,
    loop_color: Rgb,
    loop_ms: u64,
    wired: bool,
    clock: Instant,
    shown: Option<(f64, f64, f64)>,
    last_ms: f64,
    sent: Option<Rgb>,
    first: bool,
    next_at: Instant,
    failures: u32,
}

impl SyncColor {
    fn new(s: &Settings, frames: &[ScreenFrame], wired: bool) -> Self {
        let palette = Palette::from_frames(frames);
        let colors = palette.colors;
        let delays: Vec<i64> = frames.iter().take(colors.len()).map(|f| i64::from(f.delay_ms.max(20))).collect();
        let mut mode = protocol::effect_mode(s.effect);
        if mode == 0 {
            mode = 1;
        }
        let mut speed = s.speed.clamp(0, 5);
        if speed == 0 {
            speed = 3; // на нуле бегущие эффекты стоят на месте
        }
        let loop_color = average_of(&colors);
        let mut starts = Vec::with_capacity(delays.len());
        let mut total = 0i64;
        for d in &delays {
            starts.push(total);
            total += d;
        }
        let tones = colors
            .iter()
            .map(|c| if u32::from(c.0) + u32::from(c.1) + u32::from(c.2) < 24 { loop_color } else { *c })
            .map(|c| (f64::from(c.0), f64::from(c.1), f64::from(c.2)))
            .collect();
        Self {
            mode,
            speed,
            direction: s.direction.clamp(0, 3),
            brightness: s.brightness.max(0),
            tones,
            starts,
            delays,
            total,
            loop_color,
            loop_ms: total.max(1) as u64,
            wired,
            clock: Instant::now(),
            shown: None,
            last_ms: 0.0,
            sent: None,
            first: true,
            next_at: Instant::now(),
            failures: 0,
        }
    }

    /// Сколько ждать до следующего шага.
    fn wait(&self) -> Duration {
        if self.mode == 1 {
            Duration::from_millis(if self.wired { 40 } else { 60 })
        } else {
            self.next_at.saturating_duration_since(Instant::now()).min(Duration::from_millis(250))
        }
    }

    fn send(&self, kb: &mut Keyboard, c: Rgb, committed: bool) -> Result<(), String> {
        // Цвет всегда из гифки: с «радугой» прошивка игнорирует байты цвета.
        if committed {
            kb.set_effect(self.mode, &hex(c), false, self.brightness, self.speed, self.direction)
        } else {
            kb.set_effect_fast(self.mode, c, false, self.brightness, self.speed, self.direction)
        }
    }

    fn step(&mut self, kb: &mut Keyboard) -> Result<(), String> {
        if self.tones.is_empty() {
            return Ok(());
        }
        if self.mode != 1 {
            // Бегущий эффект: прошивка перезапускает волну на каждом пакете, поэтому один пакет на цикл гифки.
            if Instant::now() >= self.next_at {
                self.send(kb, self.loop_color, self.first)?;
                self.first = false;
                self.failures = 0;
                self.next_at = Instant::now() + Duration::from_millis(self.loop_ms);
            }
            return Ok(());
        }
        // «Статичный цвет» не анимируется: цвет за время кадра перетекает в цвет следующего, поверх — сглаживание ~120 мс.
        let now_ms = self.clock.elapsed().as_secs_f64() * 1000.0;
        let n = self.tones.len();
        let at = (now_ms as i64) % self.total.max(1);
        let i = match self.starts.binary_search(&at) {
            Ok(i) => i as isize,
            Err(x) => x as isize - 1,
        }
        .clamp(0, n as isize - 1) as usize;
        let k = (at - self.starts[i]) as f64 / self.delays[i].max(1) as f64;
        let k = k * k * (3.0 - 2.0 * k);
        let (a0, a1) = (self.tones[i], self.tones[(i + 1) % n]);
        let target = (a0.0 + (a1.0 - a0.0) * k, a0.1 + (a1.1 - a0.1) * k, a0.2 + (a1.2 - a0.2) * k);
        let alpha = if self.shown.is_none() { 1.0 } else { 1.0 - (-(now_ms - self.last_ms) / 120.0).exp() };
        self.last_ms = now_ms;
        let s0 = self.shown.unwrap_or(target);
        let shown = (s0.0 + (target.0 - s0.0) * alpha, s0.1 + (target.1 - s0.1) * alpha, s0.2 + (target.2 - s0.2) * alpha);
        self.shown = Some(shown);
        let q = |v: f64| v.round_ties_even().clamp(0.0, 255.0) as u8;
        let color = (q(shown.0), q(shown.1), q(shown.2));
        let differs = match self.sent {
            None => true,
            Some(p) => (i32::from(p.0) - i32::from(color.0)).abs() + (i32::from(p.1) - i32::from(color.1)).abs() + (i32::from(p.2) - i32::from(color.2)).abs() >= 3,
        };
        if differs || self.first {
            self.send(kb, color, self.first)?;
            self.first = false;
            self.sent = Some(color);
            self.failures = 0;
        }
        Ok(())
    }
}

enum Run {
    Live(Live),
    Sync(Box<SyncColor>),
}

struct Worker {
    shared: Arc<Mutex<KbState>>,
    stop: Arc<AtomicBool>,
    kb: Option<Keyboard>,
    cfg: Option<Settings>,
    off: bool,
    run: Option<Run>,
    probe_at: Instant,
    /// Гифка с экрана (LastImage) и по каким настройкам она прочитана.
    frames: Vec<ScreenFrame>,
    frames_key: Option<(String, bool, i32)>,
    /// Что сейчас сохранено в клавиатуре перед потоком кадров (EnsureEffect в C#): не переписывать флеш тем же.
    saved: Option<String>,
}

impl Worker {
    fn new(shared: Arc<Mutex<KbState>>, stop: Arc<AtomicBool>) -> Self {
        Self {
            shared,
            stop,
            kb: None,
            cfg: None,
            off: false,
            run: None,
            probe_at: Instant::now(),
            frames: Vec::new(),
            frames_key: None,
            saved: None,
        }
    }

    fn set(&self, f: impl FnOnce(&mut KbState)) {
        f(&mut lock(&self.shared));
    }

    fn event(&self, text: String) {
        self.set(|s| s.event = Some((Instant::now(), text)));
    }

    fn run(mut self, rx: Receiver<Cmd>) {
        while !self.stop.load(Ordering::Relaxed) {
            let wait = match &self.run {
                Some(Run::Live(_)) => Duration::ZERO,
                Some(Run::Sync(s)) => s.wait(),
                None => Duration::from_millis(250),
            };
            let cmd = if wait.is_zero() {
                match rx.try_recv() {
                    Ok(c) => Some(c),
                    Err(mpsc::TryRecvError::Empty) => None,
                    Err(mpsc::TryRecvError::Disconnected) => return,
                }
            } else {
                match rx.recv_timeout(wait) {
                    Ok(c) => Some(c),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            };
            match cmd {
                Some(Cmd::Apply(s, force_on)) => {
                    if force_on {
                        self.off = false;
                    }
                    self.cfg = Some(*s);
                    if !self.off {
                        self.apply();
                    }
                }
                Some(Cmd::Light(on)) => {
                    self.off = !on;
                    self.run = None;
                    if on {
                        self.apply();
                    } else {
                        self.turn_off();
                    }
                }
                Some(Cmd::Time) => self.sync_time(),
                Some(Cmd::Reconnect) => {
                    self.kb = None;
                    self.run = None;
                    self.apply_or_open();
                }
                None => {}
            }
            self.set(|s| {
                s.off = self.off;
                s.live = self.run.is_some();
            });
            match self.run {
                Some(Run::Live(_)) => self.frame(),
                Some(Run::Sync(_)) => self.sync_step(),
                None => {}
            }
            if !matches!(self.run, Some(Run::Live(_))) && self.probe_at.elapsed() >= Duration::from_secs(3) {
                self.probe_at = Instant::now();
                self.watch_link();
            }
        }
    }

    /// Открыть клавиатуру, если ещё не открыта. false — не нашлась.
    fn ensure_open(&mut self) -> bool {
        if self.kb.is_some() {
            return true;
        }
        let r = hidapi::HidApi::new().map_err(|e| format!("hidapi: {e}")).and_then(|api| Keyboard::open(&api));
        match r {
            Ok(kb) => {
                let how = kb.describe();
                self.set(|s| s.link = Some(Ok(how)));
                self.kb = Some(kb);
                self.saved = None;
                true
            }
            Err(e) => {
                self.set(|s| s.link = Some(Err(e)));
                false
            }
        }
    }

    fn apply_or_open(&mut self) {
        if self.off {
            self.ensure_open();
        } else {
            self.apply();
        }
    }

    /// Прочитать гифку экрана, если поменялись файл, «Кадр» или «Кадров в гифке».
    fn ensure_frames(&mut self, s: &Settings) {
        let key = (s.last_image.clone().unwrap_or_default(), s.crop, s.max_frames);
        if self.frames_key.as_ref() == Some(&key) {
            return;
        }
        self.frames = if key.0.is_empty() {
            Vec::new()
        } else {
            screen::load(Path::new(&key.0), key.1, key.2.max(0) as usize).unwrap_or_default()
        };
        self.frames_key = Some(key);
    }

    /// Выставить подсветку по self.cfg: один раз, потоком кадров или сменой цвета в такт гифке.
    fn apply(&mut self) {
        self.run = None;
        let Some(s) = self.cfg.clone() else {
            self.ensure_open();
            return;
        };
        if !self.ensure_open() {
            return;
        }
        let wired = self.kb.as_mut().is_some_and(|k| k.wired_mut().is_some());
        let mode = protocol::effect_mode(s.effect);
        if s.sync_light || mode == effects::GIF_FLOW {
            self.ensure_frames(&s);
        }
        let mut note = None;
        if let Err(e) = self.start(&s, mode, wired, &mut note) {
            self.fail(e);
            return;
        }
        self.set(|st| st.note = note);
    }

    fn start(&mut self, s: &Settings, mode: i32, wired: bool, note: &mut Option<String>) -> Result<(), String> {
        let brightness = s.brightness.clamp(0, 5);
        let speed = s.speed.clamp(0, 5);
        let direction = s.direction.clamp(0, 3);

        // 1. «Картинка» и «Волна от экрана»: гифка по клавишам — только по кабелю.
        let gif_keys = s.sync_light
            && matches!(s.sync_style, gif_light::PICTURE | gif_light::RIPPLE)
            && !self.frames.is_empty()
            && mode != effects::PAINT
            && mode != effects::GIF_FLOW;
        if gif_keys {
            if wired {
                if let Ok(g) = GifLight::new(&self.frames, s.sync_style) {
                    self.ensure_effect(g.fallback, brightness, speed, 0)?;
                    self.start_live(Source::Gif(Box::new(g)));
                    return Ok(());
                }
            } else {
                *note = Some("Картинка и волна по клавишам работают только по кабелю — по радио подсветка меняет один цвет кадра.".into());
            }
        }

        // 2. Свои эффекты.
        if effects::is_custom(mode) {
            if mode == effects::PAINT {
                if s.paint_anim == 0 || !wired {
                    if s.paint_anim != 0 {
                        *note = Some("Анимация раскраски работает только по кабелю — по радио раскраска без анимации.".into());
                    }
                    return self.apply_plain(s);
                }
                let paint: keypaint::Paint = s.key_paint.clone().unwrap_or_default().into_iter().collect();
                self.ensure_user(&keypaint::build_frame(Some(&paint), brightness))?;
                self.start_live(Source::Effect { palette: None });
                return Ok(());
            }
            if !wired {
                let (m, _) = keyboard::builtin_for_custom(mode, s.rainbow);
                let name = protocol::effect_index(m).map(protocol::effect_name).unwrap_or("Прокрутка");
                *note = Some(format!("Свои эффекты — только по кабелю: по радио горит «{name}»."));
                return self.apply_plain(s);
            }
            let color = protocol::parse_color(Some(&s.color), (255, 255, 255));
            let palette = (s.sync_light && self.frames.len() >= 2).then(|| Palette::from_frames(&self.frames));
            let still = palette.as_ref().map_or(color, Palette::average);
            if mode == effects::GIF_FLOW {
                if self.frames.is_empty() {
                    *note = Some("Для «Гифка → справа налево» выберите гифку слева, в блоке «Экран» — подсветка продолжит её.".into());
                    return Ok(());
                }
                let strips: Vec<Vec<Rgb>> = self.frames.iter().map(|f| protocol::strip_colors(&f.rgb565, 6, 0.3)).collect();
                let mut cumulative = Vec::with_capacity(self.frames.len());
                let mut acc = 0i32;
                for f in &self.frames {
                    acc += f.delay_ms.max(20);
                    cumulative.push(acc);
                }
                self.ensure_effect(still, brightness, speed, direction)?;
                self.start_live(Source::Flow { strips, cumulative, loop_ms: acc });
                return Ok(());
            }
            self.ensure_effect(still, brightness, speed, direction)?;
            self.start_live(Source::Effect { palette });
            return Ok(());
        }

        // 3. «Один цвет кадра» (и запасной вариант по радио).
        if s.sync_light && self.frames.len() >= 2 {
            if matches!(mode, 6 | 8) {
                *note = Some("«Разноцветный» и «Спектр» в прошивке всегда радужные — цвет гифки к ним не применяется.".into());
            }
            self.run = Some(Run::Sync(Box::new(SyncColor::new(s, &self.frames, wired))));
            return Ok(());
        }
        if s.sync_light && note.is_none() {
            *note = Some("Для подсветки в такт выберите гифку слева, в блоке «Экран».".into());
        }

        // 4. Обычный эффект прошивки или раскраска.
        self.apply_plain(s)
    }

    fn apply_plain(&mut self, s: &Settings) -> Result<(), String> {
        let kb = self.kb.as_mut().ok_or("клавиатура не открыта")?;
        kb.apply_settings(s)?;
        self.saved = None;
        Ok(())
    }

    /// Сохранить в клавиатуру статичный цвет, если там сейчас не он.
    fn ensure_effect(&mut self, c: Rgb, brightness: i32, speed: i32, direction: i32) -> Result<(), String> {
        let key = format!("fx|{}|{brightness}|{speed}|{direction}", hex(c));
        if self.saved.as_deref() == Some(key.as_str()) {
            return Ok(());
        }
        let kb = self.kb.as_mut().ok_or("клавиатура не открыта")?;
        kb.set_effect(1, &hex(c), false, brightness, speed, direction)?;
        self.saved = Some(key);
        Ok(())
    }

    /// Сохранить раскраску (без анимации), если там сейчас не она.
    fn ensure_user(&mut self, frame: &[u8]) -> Result<(), String> {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        frame.hash(&mut h);
        let key = format!("user|{:x}", h.finish());
        if self.saved.as_deref() == Some(key.as_str()) {
            return Ok(());
        }
        let kb = self.kb.as_mut().ok_or("клавиатура не открыта")?;
        kb.set_user_light(frame)?;
        self.saved = Some(key);
        Ok(())
    }

    fn start_live(&mut self, src: Source) {
        self.run = Some(Run::Live(Live {
            src,
            start: Instant::now(),
            pacer: timing::Pacer::new(Duration::from_millis(effects::FRAME_INTERVAL_MS)),
            _timer: timing::HighResTimer::begin(),
            failures: 0,
        }));
    }

    fn turn_off(&mut self) {
        let Some(s) = self.cfg.clone() else { return };
        if !self.ensure_open() {
            return;
        }
        self.saved = None;
        if let Some(kb) = self.kb.as_mut() {
            if let Err(e) = kb.set_light_on(&s, false) {
                self.fail(e);
            }
        }
    }

    fn sync_time(&mut self) {
        if !self.ensure_open() {
            self.event("Часы: клавиатура не найдена.".into());
            return;
        }
        let Some(kb) = self.kb.as_mut() else { return };
        let via = if kb.via_dongle() { " (по радио)" } else { "" };
        match kb.sync_time(now_parts()) {
            Ok(()) => {
                let t = chrono::Local::now().format("%H:%M:%S");
                self.event(format!("Часы на экране выставлены: {t}{via}."));
            }
            Err(e) => self.fail(format!("часы: {e}")),
        }
    }

    /// Ошибка обмена: забываем устройство (переподключимся при следующей проверке).
    fn fail(&mut self, e: String) {
        self.kb = None;
        self.run = None;
        self.saved = None;
        self.event(format!("Клавиатура: {e}"));
        self.set(|s| s.link = Some(Err(e)));
        self.probe_at = Instant::now();
    }

    fn frame(&mut self) {
        let (Some(s), Some(Run::Live(live))) = (self.cfg.as_ref(), self.run.as_mut()) else { return };
        let bank = live_bank(s, &live.src, live.start.elapsed().as_secs_f64());
        let frame = protocol::live_frame(&bank);
        let sent = match self.kb.as_mut().and_then(|k| k.wired_mut()) {
            Some(dev) => dev.send_live_frame(&frame),
            None => Err("кабель отключён".to_string()),
        };
        match sent {
            Ok(_) => live.failures = 0,
            Err(e) => {
                live.failures += 1;
                if live.failures >= 6 {
                    self.fail(format!("не принимает кадры подсветки: {e}"));
                    return;
                }
                std::thread::sleep(Duration::from_millis(80));
                return;
            }
        }
        if let Some(Run::Live(live)) = self.run.as_mut() {
            live.pacer.wait();
        }
    }

    fn sync_step(&mut self) {
        let (Some(Run::Sync(sync)), Some(kb)) = (self.run.as_mut(), self.kb.as_mut()) else { return };
        if let Err(e) = sync.step(kb) {
            sync.failures += 1;
            if sync.failures >= 6 {
                self.fail(format!("не принимает пакеты подсветки: {e}"));
                return;
            }
            std::thread::sleep(Duration::from_millis(60));
        }
    }

    /// Раз в 3 с без живых кадров: кабель воткнули или выдернули — переподключиться и выставить подсветку.
    fn watch_link(&mut self) {
        let cable = cable_present();
        let need = match &self.kb {
            None => true,
            Some(kb) => kb.via_dongle() == cable,
        };
        if need {
            self.kb = None;
            self.run = None;
            self.apply_or_open();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(lo: u8, hi: u8, delay_ms: i32) -> ScreenFrame {
        let n = protocol::SCREEN_SIZE * protocol::SCREEN_SIZE;
        ScreenFrame { rgb565: [lo, hi].repeat(n), delay_ms }
    }

    #[test]
    fn palette_follows_frames() {
        let frames = vec![solid(0x00, 0xF8, 100), solid(0x1F, 0x00, 300)];
        let p = Palette::from_frames(&frames);
        let a = p.at(0.05, (1, 2, 3));
        let b = p.at(0.2, (1, 2, 3));
        assert!(a.0 > a.2, "первый кадр красный: {a:?}");
        assert!(b.2 > b.0, "второй кадр синий: {b:?}");
        assert_eq!(p.at(0.45, (1, 2, 3)), a); // цикл 400 мс
    }

    #[test]
    fn live_bank_sources() {
        let mut s = Settings::default();
        s.effect = 20; // «Своя волна →»
        s.brightness = 3;
        let bank = live_bank(&s, &Source::Effect { palette: None }, 1.5);
        assert_eq!(bank.len(), 576);
        assert_eq!((bank[574], bank[575]), (0xAA, 0x55));
        let frames = vec![solid(0x00, 0xF8, 100), solid(0x1F, 0x00, 300)];
        let strips: Vec<Vec<Rgb>> = frames.iter().map(|f| protocol::strip_colors(&f.rgb565, 6, 0.3)).collect();
        s.effect = protocol::effect_index(effects::GIF_FLOW).unwrap();
        let bank = live_bank(&s, &Source::Flow { strips, cumulative: vec![100, 400], loop_ms: 400 }, 0.7);
        assert_eq!(bank.len(), 576);
        let g = GifLight::new(&frames, gif_light::RIPPLE).unwrap();
        assert_eq!(live_bank(&s, &Source::Gif(Box::new(g)), 0.3).len(), 576);
    }

    #[test]
    fn sync_color_setup() {
        let frames = vec![solid(0x00, 0xF8, 100), solid(0x1F, 0x00, 300)];
        let mut s = Settings::default();
        s.effect = 0;
        s.speed = 0;
        let c = SyncColor::new(&s, &frames, true);
        assert_eq!((c.mode, c.speed), (1, 3));
        assert_eq!((c.total, c.loop_ms), (400, 400));
        assert_eq!(c.wait(), Duration::from_millis(40));
    }
}
