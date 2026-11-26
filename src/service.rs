//! Фоновая служба: держит ARGB (ASUS AURA) и экран помпы по settings.json —
//! то, что в C#-версии делали таймеры главного окна. Используют `rigcontrol daemon` и окно с треем.
//! Каждая часть крутится в своём потоке; если устройство пропало (сон, кабель), поток ждёт 5 с и подключается снова.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::argb::{self, ArgbConfig, ArgbMode, Renderer};
use crate::keyboard::Keyboard;
use crate::settings::Settings;
use crate::{aura, cooler, effects, lcdscene, logfile, protocol, telemetry, timing};

type Status = Arc<Mutex<Vec<(String, String)>>>;

/// ArgbMode из settings.json (нумерация C#) → режим движка. None — «не управлять».
pub fn argb_mode_from_settings(v: i32) -> Option<ArgbMode> {
    match v {
        1 => Some(ArgbMode::Off),
        2 => Some(ArgbMode::Static),
        3 => Some(ArgbMode::Breathing),
        4 => Some(ArgbMode::Spectrum),
        5 | 9 => Some(ArgbMode::RainbowWave),
        6 => Some(ArgbMode::Comet),
        7 => Some(ArgbMode::CpuTemp),
        8 => Some(ArgbMode::Keyboard),
        _ => None,
    }
}

pub fn argb_config(s: &Settings, mode: ArgbMode) -> ArgbConfig {
    let col = |t: &str, d: crate::Rgb| protocol::parse_color(Some(t), d);
    ArgbConfig {
        mode,
        color: col(s.argb_color.as_str(), (0, 180, 255)),
        color2: Some(col(s.argb_color2.as_str(), (255, 0, 168))),
        color3: Some(col(s.argb_color3.as_str(), (138, 43, 255))),
        brightness: argb::led_brightness(s.argb_brightness.clamp(0, 100)),
        speed: s.argb_speed.clamp(1, 5),
        leds: s.argb_leds.clamp(1, aura::MAX_LEDS as i32) as usize,
        fans: s.argb_fans.clamp(1, 6) as usize,
        keyboard_effect: {
            let mode = protocol::effect_mode(s.effect);
            if effects::is_rainbow_flow(mode) { mode } else { effects::FLOW }
        },
        keyboard_speed: s.speed.clamp(0, 5),
        keyboard_dir: s.direction.clamp(0, 3),
        rainbow: s.argb_rainbow,
    }
}

fn set_status(st: &Status, part: &str, text: String) {
    let mut v = st.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(x) = v.iter_mut().find(|x| x.0 == part) {
        x.1 = text;
    } else {
        v.push((part.to_string(), text));
    }
}

/// Перезапускает часть после ошибки, пока службу не остановили.
fn supervise(part: &'static str, stop: &AtomicBool, st: &Status, mut f: impl FnMut() -> Result<(), String>) {
    while !stop.load(Ordering::Relaxed) {
        match f() {
            Ok(()) => return,
            Err(e) => {
                set_status(st, part, format!("нет связи: {e} (повтор через 5 с)"));
                logfile::append("service.log", &format!("{} {part}: {e}\n", chrono::Local::now().format("%H:%M:%S")));
                for _ in 0..50 {
                    if stop.load(Ordering::Relaxed) { return; }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
    }
}

pub struct Service {
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
    status: Status,
}

impl Service {
    /// keyboard=true — один раз выставить эффект клавиатуры из настроек.
    pub fn start(s: &Settings, keyboard: bool) -> Self {
        Self::start_with(s, keyboard, true)
    }

    /// pump = false: не выводить на помпу при запуске (переключатель «Автовывод на помпу при запуске»).
    pub fn start_with(s: &Settings, keyboard: bool, pump: bool) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let status: Status = Arc::new(Mutex::new(Vec::new()));
        let mut threads = Vec::new();

        if keyboard {
            let r = hidapi::HidApi::new()
                .map_err(|e| format!("hidapi: {e}"))
                .and_then(|api| Keyboard::open(&api).and_then(|mut d| d.apply_settings(&s).map(|_| d.describe())));
            set_status(&status, "Клавиатура", match r {
                Ok(how) => format!("«{}» выставлен ({how})", protocol::effect_name(s.effect)),
                Err(e) => format!("не найдена: {e}"),
            });
        }

        match argb_mode_from_settings(s.argb_mode) {
            Some(mode) => {
                let cfg = argb_config(s, mode);
                let target = s.argb_target.max(0) as usize;
                let board = s.argb_board_effect;
                let (stop2, st2) = (stop.clone(), status.clone());
                threads.push(std::thread::spawn(move || {
                    supervise("ARGB", &stop2, &st2, || argb_loop(&cfg, target, board, &stop2, &st2));
                }));
            }
            None => set_status(&status, "ARGB", "не управляется (ArgbMode = 0)".into()),
        }

        if pump {
            let s2 = s.clone();
            let (stop3, st3) = (stop.clone(), status.clone());
            threads.push(std::thread::spawn(move || {
                supervise("Помпа", &stop3, &st3, || pump_loop(&s2, &stop3, &st3));
            }));
        } else {
            set_status(&status, "Помпа", "автовывод при запуске выключен".into());
        }

        Self { stop, threads, status }
    }

    pub fn status(&self) -> Vec<(String, String)> {
        self.status.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn is_running(&self) -> bool {
        self.threads.iter().any(|t| !t.is_finished())
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn argb_loop(cfg: &ArgbConfig, target: usize, board: bool, stop: &AtomicBool, st: &Status) -> Result<(), String> {
    let api = hidapi::HidApi::new().map_err(|e| format!("hidapi: {e}"))?;
    let a = aura::AuraUsb::find(&api)?;
    // Как в C#: «все разъёмы» — это и ARGB-разъёмы, и подсветка самой платы (канал эффектов 0, прямой канал 4).
    let all = target == 0 || target > a.argb_headers;
    let headers: Vec<u8> = if all { (0..a.argb_headers as u8).collect() } else { vec![(target - 1) as u8] };
    let mut effect: Vec<u8> = Vec::new();
    if all {
        effect.push(0);
    }
    effect.extend(headers.iter().map(|h| h + 1));
    set_status(st, "ARGB·разъёмы", a.argb_headers.to_string());
    let firmware = a.description().lines().next().unwrap_or_default().to_string();

    let leds = cfg.leds;
    let mut frame = vec![0u8; leds * 3];
    let mut send = vec![0u8; leds * 3];
    let mut last = vec![1u8; leds * 3];
    let mut r = Renderer::new();
    let start = Instant::now();
    let mut tel = if matches!(cfg.mode, ArgbMode::CpuTemp) { Some(telemetry::Telemetry::new()) } else { None };
    let mut temp = f64::NAN;
    let mut last_temp: Option<Instant> = None;
    let mut sample_temp = |tel: &mut Option<telemetry::Telemetry>, temp: &mut f64| {
        if let Some(t) = tel.as_mut() {
            if last_temp.map(|x| x.elapsed() >= Duration::from_secs(1)).unwrap_or(true) {
                *temp = t.sample().cpu_temp.unwrap_or(f64::NAN);
                last_temp = Some(Instant::now());
            }
        }
    };

    // «Эффект платы»: эффект крутит сам контроллер, программа шлёт его только при изменении.
    if board {
        set_status(st, "ARGB", format!("{firmware} · ARGB-разъёмов: {} · эффект платы", a.argb_headers));
        let mut sent: Option<(u8, crate::Rgb)> = None;
        let mut sent_at = Instant::now();
        while !stop.load(Ordering::Relaxed) {
            sample_temp(&mut tel, &mut temp);
            r.render(cfg, start.elapsed().as_secs_f64(), temp, &mut frame);
            let (mode, c) = argb::board_effect_for(cfg, &frame);
            let bri = f64::from(cfg.brightness.clamp(0, 100)) / 100.0;
            let c = (argb::out(c.0, bri), argb::out(c.1, bri), argb::out(c.2, bri));
            // Раз в 30 с — ещё раз: после сна или Armoury Crate плата могла вернуть своё.
            if sent != Some((mode, c)) || sent_at.elapsed() > Duration::from_secs(30) {
                a.set_effect(&effect, mode, c)?;
                sent = Some((mode, c));
                sent_at = Instant::now();
            }
            nap(if matches!(cfg.mode, ArgbMode::CpuTemp | ArgbMode::Keyboard) { 2000 } else { 500 }, stop);
        }
        return Ok(());
    }

    a.enter_direct(&effect)?;
    set_status(st, "ARGB", format!("{firmware} · ARGB-разъёмов: {} · {} LED", a.argb_headers, leds));
    let _timer = timing::HighResTimer::begin();
    let mut pacer = timing::Pacer::new(Duration::from_millis(argb::FRAME_MS));
    let mut last_sent = Instant::now();
    let mut last_loop = Instant::now();
    let mut first = true;
    while !stop.load(Ordering::Relaxed) {
        // Цикл стоял дольше 5 с — компьютер спал: плата вернула свою подсветку, включаем прямой режим заново.
        if last_loop.elapsed() > Duration::from_secs(5) {
            a.enter_direct(&effect)?;
            first = true;
        }
        last_loop = Instant::now();
        sample_temp(&mut tel, &mut temp);
        r.render(cfg, start.elapsed().as_secs_f64(), temp, &mut frame);
        argb::output(&frame, cfg.brightness, &mut send);
        // Неизменный кадр повторяем раз в секунду: если плату перехватили (сон, Armoury Crate) — вернётся.
        if first || send != last || last_sent.elapsed() > Duration::from_secs(1) {
            for &h in &headers {
                a.send_direct(h, &send, leds)?;
            }
            if all && a.mainboard_leds > 0 {
                a.send_direct(aura::FIXED_DIRECT_CHANNEL, &send, leds.min(a.mainboard_leds as usize))?;
            }
            last.copy_from_slice(&send);
            last_sent = Instant::now();
            first = false;
        }
        if cfg.mode.animated() {
            pacer.wait();
        } else {
            std::thread::sleep(Duration::from_millis(200));
        }
    }
    Ok(())
}

fn rotate(img: image::RgbImage, rot: u32) -> image::RgbImage {
    match rot {
        90 => image::imageops::rotate90(&img),
        180 => image::imageops::rotate180(&img),
        270 => image::imageops::rotate270(&img),
        _ => img,
    }
}

/// Сон с проверкой остановки.
fn nap(ms: u64, stop: &AtomicBool) {
    let end = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < end && !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Профилактическое рукопожатие перед кадром (ReArmBeforeFrameAsync в C#): если кадры надолго
/// прервались (сон ПК, тяжёлая загрузка картинки) или просто прошло 30 с, прошивка помпы уже могла
/// показать свой логотип и молча игнорировать кадры — запись в USB при этом проходит без ошибки.
struct ReArm {
    last_frame: Option<Instant>,
    last_rearm: Instant,
    blocked_until: Option<Instant>,
}

/// Как часто повторять рукопожатие «на всякий случай» (KeepAliveMs).
const REARM_KEEP_ALIVE: Duration = Duration::from_secs(30);
/// Помпа не ответила на профилактическое рукопожатие — столько его не повторяем.
const REARM_BACKOFF: Duration = Duration::from_secs(300);

impl ReArm {
    /// opened — когда помпу открыли (тогда же было рукопожатие). Отсчёт паузы — от него: загрузка гифки
    /// или первый опрос датчиков между открытием и первым кадром тоже могут увести помпу на логотип
    /// (в C# перед запуском показа всегда идёт Reinit).
    fn new(opened: Instant) -> Self {
        Self { last_frame: Some(opened), last_rearm: opened, blocked_until: None }
    }

    /// Пора ли повторить рукопожатие перед кадром, который идёт раз в interval_ms.
    fn due(&self, interval_ms: u64, now: Instant) -> bool {
        let pause = Duration::from_millis((interval_ms * 2 + 500).max(1500));
        let after_pause = self.last_frame.is_some_and(|t| now.saturating_duration_since(t) > pause);
        let blocked = self.blocked_until.is_some_and(|t| now < t);
        let after_time = !blocked && now.saturating_duration_since(self.last_rearm) > REARM_KEEP_ALIVE;
        after_pause || after_time
    }

    fn before_frame(&mut self, lcd: &mut cooler::CoolerLcd, interval_ms: u64) {
        let now = Instant::now();
        if !self.due(interval_ms, now) {
            return;
        }
        self.last_rearm = now;
        // Не ответила: занята или другая прошивка. Кадры шлём дальше, профилактику откладываем.
        self.blocked_until = if lcd.rearm(250) { None } else { Some(now + REARM_BACKOFF) };
    }

    fn sent(&mut self) {
        self.last_frame = Some(Instant::now());
    }
}

/// Как часто опрашивать датчики для экрана помпы.
pub const SENSOR_PERIOD: Duration = Duration::from_millis(1000);

/// Размер экрана помпы из настроек (CoolerSize в C#): 0 — по ответу помпы.
pub const PUMP_SIZES: [(u32, u32); 5] = [(0, 0), (320, 320), (320, 240), (240, 320), (640, 480)];

/// Диагностика помпы для окна: описание, ответ рукопожатия, сколько кадров уходит в секунду.
fn pump_diag(st: &Status, lcd: &cooler::CoolerLcd, fps: f64) {
    set_status(st, "Помпа·диагностика", format!("{}\n{}\nОтправлено: {fps:.1} кадр/с", lcd.description(), lcd.diagnostics().trim_end()));
}

fn pump_loop(s: &Settings, stop: &AtomicBool, st: &Status) -> Result<(), String> {
    let api = hidapi::HidApi::new().map_err(|e| format!("hidapi: {e}"))?;
    let mut lcd = cooler::CoolerLcd::open(&api, None)?;
    let opened = Instant::now();
    // Размер экрана выбран вручную — он важнее ответа помпы.
    if let Some(&(w, h)) = PUMP_SIZES.get(s.cooler_size.max(0) as usize).filter(|p| p.0 > 0) {
        lcd.resolution = (w, h);
    }
    let rot = match s.cooler_rotation { 90 => 90, 180 => 180, 270 => 270, _ => 0 };
    let (pw, ph) = lcd.resolution;
    let mut sent = 0u32;
    let mut rate_at = Instant::now();
    let mut fps = 0.0;
    let mut count = |lcd: &cooler::CoolerLcd, sent: &mut u32| {
        *sent += 1;
        let dt = rate_at.elapsed().as_secs_f64();
        if dt >= 2.0 {
            fps = f64::from(*sent) / dt;
            *sent = 0;
            rate_at = Instant::now();
            pump_diag(st, lcd, fps);
        }
    };
    pump_diag(st, &lcd, 0.0);

    // Режим «картинка/GIF» (CoolerMode = 1).
    if s.cooler_mode == 1 {
        if let Some(path) = s.cooler_image.as_deref().filter(|p| !p.is_empty()) {
            let frames = cooler::load_image(Path::new(path), pw, ph, rot, s.crop)?;
            if frames.is_empty() { return Err(format!("{path}: нет кадров")); }
            let name = Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string());
            set_status(st, "Помпа", format!("{} · картинка {name} ({} кадр.)", lcd.description(), frames.len()));
            let mut i = 0usize;
            let mut rearm = ReArm::new(opened);
            while !stop.load(Ordering::Relaxed) {
                let tick = Instant::now();
                let f = &frames[i % frames.len()];
                // Одиночную картинку повторяем дважды в секунду — иначе помпа уходит на логотип.
                let delay = if frames.len() == 1 { 500 } else { (f.delay_ms as u64).max(20) };
                rearm.before_frame(&mut lcd, delay);
                lcd.send_image(&f.image)?;
                rearm.sent();
                count(&lcd, &mut sent);
                nap(delay.saturating_sub(tick.elapsed().as_millis() as u64), stop);
                i += 1;
            }
            return Ok(());
        }
    }

    // Мониторинг: сцена с часами и датчиками.
    let swap = matches!(rot, 90 | 270);
    let (w, h) = if swap { (ph, pw) } else { (pw, ph) };
    let theme = s.cooler_theme.as_deref()
        .and_then(lcdscene::find_theme)
        .unwrap_or_else(lcdscene::default_theme);
    // Своя раскладка из редактора (CoolerLayout), иначе — раскладка темы.
    let custom = lcdscene::saved_layout(s.cooler_layout.as_ref(), s.cooler_layout_base, w, h);
    let layout_name = if custom.is_some() { "своя раскладка" } else { "раскладка темы" };
    let items = custom.unwrap_or_else(|| lcdscene::layout_for(&theme, w, h));
    // Фон: своя картинка или картинка темы (pack://…); гифка проигрывается.
    let bg: Vec<cooler::LcdFrame> = s.cooler_background.as_deref()
        .filter(|p| !p.trim().is_empty())
        .and_then(|p| cooler::load_image(Path::new(p), w, h, 0, true).ok())
        .unwrap_or_default();
    let bg_total: u64 = bg.iter().map(|f| u64::from(f.delay_ms.max(20))).sum();
    let animated = bg.len() > 1 && bg_total > 0;
    let top = s.cooler_bg_top.as_deref().and_then(lcdscene::parse_color)
        .or_else(|| lcdscene::parse_color(theme.top)).unwrap_or((0x10, 0x12, 0x1A));
    let bottom = s.cooler_bg_bottom.as_deref().and_then(lcdscene::parse_color)
        .or_else(|| lcdscene::parse_color(theme.bottom)).unwrap_or((5, 6, 10));
    let k = s.cooler_brightness.clamp(0, 100) as f64 / 100.0;
    // «Частота: до N кадр/с» — как часто обновлять датчики и сцену; кадр уходит не реже двух раз в секунду.
    let step = Duration::from_millis(1000 / s.cooler_fps.clamp(1, 10) as u64);
    let mut fonts = lcdscene::Fonts::default();
    let mut tel = telemetry::Telemetry::new();
    let bg_note = if animated { format!(" · фон-гифка {} кадр.", bg.len()) } else { String::new() };
    set_status(st, "Помпа", format!("{} · сцена «{}» {}×{} · {}{bg_note}", lcd.description(), theme.name, w, h, layout_name));
    nap(500, stop);

    let start = Instant::now();
    let mut snap = tel.sample();
    telemetry::publish(&snap);
    let mut sampled = Instant::now();
    let mut last_sig = String::new();
    let mut last_frame: Option<image::RgbImage> = None;
    let mut last_sent = Instant::now() - Duration::from_secs(1);
    let mut rearm = ReArm::new(opened);
    while !stop.load(Ordering::Relaxed) {
        let tick = Instant::now();
        // Датчики — раз в секунду (LibreHardwareMonitor тяжёлый, чаще смысла нет); частота кадров на это не влияет.
        if sampled.elapsed() >= SENSOR_PERIOD {
            snap = tel.sample();
            telemetry::publish(&snap);
            sampled = Instant::now();
        }
        let now = lcdscene::now();
        // Кадр фона-гифки по времени.
        let (bg_index, bg_delay) = if animated {
            let mut t = start.elapsed().as_millis() as u64 % bg_total;
            let mut idx = 0usize;
            for (n, f) in bg.iter().enumerate() {
                let d = u64::from(f.delay_ms.max(20));
                if t < d {
                    idx = n;
                    break;
                }
                t -= d;
            }
            (idx, u64::from(bg[idx].delay_ms.max(20)).saturating_sub(t).max(1))
        } else {
            (0, u64::MAX)
        };
        let sig = format!("{}|{bg_index}", lcdscene::signature(&items, &snap, &now));
        if sig != last_sig || last_frame.is_none() {
            let background = match bg.get(bg_index) {
                Some(f) => lcdscene::Background::Image(&f.image),
                None => lcdscene::Background::Gradient(top, bottom),
            };
            last_frame = Some(rotate(lcdscene::render(w, h, &items, &snap, &now, background, k, &mut fonts), rot));
            last_sig = sig;
            if let Some(f) = &last_frame {
                rearm.before_frame(&mut lcd, 500);
                lcd.send_image(f)?;
                rearm.sent();
                count(&lcd, &mut sent);
                last_sent = Instant::now();
            }
        } else if last_sent.elapsed() >= Duration::from_millis(500) {
            if let Some(f) = &last_frame {
                rearm.before_frame(&mut lcd, 500);
                lcd.send_image(f)?;
                rearm.sent();
                count(&lcd, &mut sent);
                last_sent = Instant::now();
            }
        }
        // Гифку — с максимальной скоростью, которую держит помпа; иначе — по частоте из настроек.
        let wait = if animated { Duration::from_millis(bg_delay.min(1000)) } else { step.min(Duration::from_millis(500)) };
        nap((wait.as_millis() as u64).saturating_sub(tick.elapsed().as_millis() as u64), stop);
    }
    Ok(())
}

/// «Сбросить USB экрана» (как в C# и TrccSharp): удалить устройство 0416:5302 и пересканировать шину,
/// чтобы Windows подняла его заново. Нужны права администратора. Занимает около 5 секунд.
pub fn reset_pump_usb() -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let script = "$d = Get-PnpDevice -PresentOnly -ErrorAction SilentlyContinue | \
            Where-Object { $_.InstanceId -like 'USB\\VID_0416&PID_5302\\*' }; \
            foreach ($x in $d) { pnputil /remove-device \"$($x.InstanceId)\" | Out-Null }; \
            Start-Sleep -Seconds 2; pnputil /scan-devices | Out-Null; Start-Sleep -Seconds 3";
        let status = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-WindowStyle", "Hidden", "-Command", script])
            .creation_flags(0x0800_0000)
            .status()
            .map_err(|e| format!("powershell: {e}"))?;
        if status.success() { Ok(()) } else { Err(format!("powershell вернул {status}")) }
    }
    #[cfg(not(windows))]
    {
        Err("сброс USB есть только в Windows".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rearm_after_pause_or_keep_alive() {
        let t0 = Instant::now();
        let mut r = ReArm { last_frame: Some(t0), last_rearm: t0, blocked_until: None };
        // Кадры идут ровно — рукопожатие не нужно.
        assert!(!r.due(500, t0 + Duration::from_millis(600)));
        // Пауза дольше max(1500, 2·интервал + 500) мс (сон ПК) — нужно.
        assert!(r.due(500, t0 + Duration::from_millis(1600)));
        assert!(!r.due(2000, t0 + Duration::from_millis(4000)));
        assert!(r.due(2000, t0 + Duration::from_millis(4600)));
        // Раз в 30 с — на всякий случай, но не пока помпа «не отвечает на профилактику».
        r.last_frame = Some(t0 + Duration::from_secs(31));
        assert!(r.due(500, t0 + Duration::from_secs(31)));
        r.blocked_until = Some(t0 + Duration::from_secs(300));
        assert!(!r.due(500, t0 + Duration::from_secs(31)));
        // Первый кадр сразу после открытия: рукопожатие только что было.
        assert!(!ReArm::new(t0).due(500, t0 + Duration::from_millis(600)));
        // Между открытием и первым кадром долго читали гифку или датчики — рукопожатие повторяем.
        assert!(ReArm::new(t0).due(500, t0 + Duration::from_secs(3)));
    }

    #[test]
    fn argb_modes_match_csharp_numbers() {
        assert_eq!(argb_mode_from_settings(0), None);
        assert_eq!(argb_mode_from_settings(5), Some(ArgbMode::RainbowWave));
        assert_eq!(argb_mode_from_settings(9), Some(ArgbMode::RainbowWave));
        assert_eq!(argb_mode_from_settings(8), Some(ArgbMode::Keyboard));
    }

    #[test]
    fn config_from_settings_is_clamped() {
        let mut s = Settings::default();
        s.argb_leds = 999;
        s.argb_speed = 0;
        let c = argb_config(&s, ArgbMode::Static);
        assert_eq!(c.leds, aura::MAX_LEDS);
        assert_eq!(c.speed, 1);
    }
}
