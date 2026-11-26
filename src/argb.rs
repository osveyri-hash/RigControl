//! Порт Argb/ArgbEngine.cs: эффекты ARGB-разъёмов.
//!
//! Контроллер ASUS Aura анимаций программы не знает — каждый кадр считает эта
//! часть и отправляет прямым режимом (см. [`crate::aura`]). Здесь только чистая
//! логика: кадр по времени, гамма и яркость, синхронизация с клавиатурой.

use crate::color::from_hsv;
use crate::{effects, keypaint, Rgb};
use std::f64::consts::PI;

pub const MAX_LEDS: usize = 120;
/// Шаг кадров (≈80 кадров/с), как FrameMs в C#.
pub const FRAME_MS: u64 = 12;

const SPEED_FACTOR: [f64; 5] = [0.35, 0.6, 1.0, 1.6, 2.5];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArgbMode {
    Off,
    Static,
    Breathing,
    Spectrum,
    RainbowWave,
    ColorWave,
    Comet,
    CpuTemp,
    /// Вентиляторы повторяют радужный эффект клавиатуры (106–109).
    Keyboard,
}

/// Имя в консоли, режим, описание.
pub const MODES: &[(&str, ArgbMode, &str)] = &[
    ("off", ArgbMode::Off, "выключить"),
    ("static", ArgbMode::Static, "постоянный цвет"),
    ("breathing", ArgbMode::Breathing, "дыхание цветом"),
    ("spectrum", ArgbMode::Spectrum, "вся лента плавно меняет цвет"),
    ("rainbow", ArgbMode::RainbowWave, "бегущая радуга"),
    ("wave", ArgbMode::ColorWave, "волна из 2–3 своих цветов"),
    ("comet", ArgbMode::Comet, "огонёк с хвостом"),
    ("temp", ArgbMode::CpuTemp, "цвет по температуре: голубой → зелёный → жёлтый → красный"),
    ("sync", ArgbMode::Keyboard, "как радужный эффект клавиатуры, вентиляторы — её продолжение"),
];

impl ArgbMode {
    pub fn parse(name: &str) -> Option<Self> {
        let name = name.to_lowercase();
        MODES.iter().find(|(n, _, _)| *n == name).map(|(_, m, _)| *m)
    }

    pub fn animated(self) -> bool {
        !matches!(self, ArgbMode::Off | ArgbMode::Static)
    }
}

#[derive(Clone, Debug)]
pub struct ArgbConfig {
    pub mode: ArgbMode,
    pub color: Rgb,
    pub color2: Option<Rgb>,
    pub color3: Option<Rgb>,
    /// 0–100 %.
    pub brightness: i32,
    /// 1–5.
    pub speed: i32,
    pub leds: usize,
    /// Сколько вентиляторов на разъёме (для sync): клавиатура делится на столько частей.
    pub fans: usize,
    /// Радужный эффект клавиатуры для sync: 106 спектр, 107 волна от центра, 108 течение, 109 вращение.
    pub keyboard_effect: i32,
    /// Скорость эффекта клавиатуры 0–5 и направление 0–3 — те же, что у `rigcontrol effect`.
    pub keyboard_speed: i32,
    pub keyboard_dir: i32,
    /// «Радуга» у бегущей волны (ArgbRainbow в C#): выключена — волна из трёх своих цветов.
    pub rainbow: bool,
}

impl Default for ArgbConfig {
    fn default() -> Self {
        Self {
            mode: ArgbMode::RainbowWave,
            color: (255, 0, 80),
            color2: None,
            color3: None,
            brightness: 100,
            speed: 3,
            leds: 60,
            fans: 3,
            keyboard_effect: effects::FLOW,
            keyboard_speed: 3,
            keyboard_dir: 0,
            rainbow: true,
        }
    }
}

/// Гамма 2,2: светодиоды светят линейно, без коррекции розовый выглядит почти белым.
/// Яркость — после гаммы, иначе на малой яркости слабые каналы округляются в ноль.
pub fn out(value: u8, brightness: f64) -> u8 {
    (255.0 * (value as f64 / 255.0).powf(2.2) * brightness).round().clamp(0.0, 255.0) as u8
}

/// Кадр → то, что уходит на светодиоды (гамма + яркость 0–100).
pub fn output(rgb: &[u8], brightness: i32, dst: &mut [u8]) {
    let bri = brightness.clamp(0, 100) as f64 / 100.0;
    for (d, &v) in dst.iter_mut().zip(rgb) {
        *d = out(v, bri);
    }
}

/// ≤ 40 °C — голубой, 60 — зелёный, 75 — жёлтый, ≥ 90 — красный.
pub fn temp_hue(temp: f64) -> f64 {
    const P: [(f64, f64); 4] = [(40.0, 195.0), (60.0, 120.0), (75.0, 55.0), (90.0, 0.0)];
    if temp <= P[0].0 {
        return P[0].1;
    }
    for w in P.windows(2) {
        let ((t0, h0), (t1, h1)) = (w[0], w[1]);
        if temp <= t1 {
            return h0 + (h1 - h0) * (temp - t0) / (t1 - t0);
        }
    }
    0.0
}

fn blend(a: Rgb, b: Rgb, k: f64) -> Rgb {
    let k = k.clamp(0.0, 1.0);
    let m = |x: u8, y: u8| (x as f64 + (y as f64 - x as f64) * k).round() as u8;
    (m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

/// Три цвета по кругу: третий плавно возвращается в первый, на стыке нет скачка.
pub fn palette_color(c: &ArgbConfig, position: f64) -> Rgb {
    let second = c.color2.unwrap_or(c.color);
    let third = c.color3.unwrap_or(second);
    let p = position.rem_euclid(1.0);
    if p < 1.0 / 3.0 {
        blend(c.color, second, p * 3.0)
    } else if p < 2.0 / 3.0 {
        blend(second, third, (p - 1.0 / 3.0) * 3.0)
    } else {
        blend(third, c.color, (p - 2.0 / 3.0) * 3.0)
    }
}

/// Где на клавиатуре «стоит» каждый светодиод вентиляторов: вентиляторы делят
/// клавиатуру на части слева направо, светодиоды вентилятора — кольцо в своей части.
#[derive(Clone, Debug)]
pub struct KeyboardSampler {
    pub leds: usize,
    pub fans: usize,
    points: Vec<(f64, f64)>,
    ring: Vec<f64>,
}

impl KeyboardSampler {
    pub fn new(leds: usize, fans: usize) -> Self {
        let leds = leds.max(1);
        let fans = fans.clamp(1, 6);
        let per_fan = (leds / fans).max(1);
        let (w, h) = (keypaint::WIDTH, keypaint::HEIGHT);
        let mut points = Vec::with_capacity(leds);
        let mut ring = Vec::with_capacity(leds);
        for i in 0..leds {
            let fan = (i / per_fan).min(fans - 1);
            let j = i - fan * per_fan;
            let n = if fan == fans - 1 { leds - fan * per_fan } else { per_fan }.max(1);
            let angle = 2.0 * PI * j as f64 / n as f64 - PI / 2.0;
            let cell = w / fans as f64;
            let radius = cell.min(h) * 0.38;
            let px = cell * (fan as f64 + 0.5) + radius * angle.cos();
            let py = h / 2.0 + radius * angle.sin();
            points.push((px / w, py / h));
            ring.push(j as f64 / n as f64);
        }
        Self { leds, fans, points, ring }
    }

    /// x, y — 0…1.
    pub fn position(&self, led: usize) -> (f64, f64) {
        self.points[led]
    }

    /// 0 — верх кольца, дальше по часовой (0…1).
    pub fn ring(&self, led: usize) -> f64 {
        self.ring[led]
    }
}

/// Состояние между кадрами (сглаживание температуры, раскладка для sync).
/// Ползунок яркости 0–100 → мощность светодиодов (ArgbLedBrightness в C#): 0 — выключено, 1 — 10 %,
/// 100 — 100 %. Ниже 10 % слабые каналы пропадают и цвет уходит в красный.
pub fn led_brightness(slider: i32) -> i32 {
    if slider <= 0 { 0 } else { (10.0 + f64::from(slider.min(100) - 1) * 90.0 / 99.0).round() as i32 }
}

/// «Эффект платы» (BoardEffectFor в C#): что включить встроенным эффектом контроллера.
/// Своих эффектов (огонёк, температура, клавиатура) плата не умеет — берётся ближайший: радуга, цвет
/// или текущий цвет кадра одним тоном (огрублённый, чтобы не слать на плату каждую мелочь).
/// Режимы контроллера: 0 — выкл, 1 — цвет, 2 — дыхание, 4 — смена цветов, 5 — радуга.
pub fn board_effect_for(c: &ArgbConfig, frame: &[u8]) -> (u8, Rgb) {
    let col = c.color;
    match c.mode {
        ArgbMode::Off => (0, (0, 0, 0)),
        ArgbMode::Static | ArgbMode::Comet => (1, col),
        ArgbMode::Breathing => (2, col),
        ArgbMode::Spectrum => (4, (0, 0, 0)),
        ArgbMode::RainbowWave | ArgbMode::ColorWave if c.rainbow => (5, (0, 0, 0)),
        ArgbMode::RainbowWave | ArgbMode::ColorWave => (1, col),
        _ => {
            let at = |i: usize| frame.get(i).copied().unwrap_or(0) & 0xF0;
            (1, (at(0), at(1), at(2)))
        }
    }
}

pub struct Renderer {
    temp: f64,
    sampler: Option<KeyboardSampler>,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer {
    pub fn new() -> Self {
        Self { temp: f64::NAN, sampler: None }
    }

    /// Кадр в полную силу (R, G, B на светодиод) на момент `t` секунд.
    /// `cpu_temp` — NaN, если датчика нет. Яркость и гамма — потом, в [`output`].
    pub fn render(&mut self, c: &ArgbConfig, t: f64, cpu_temp: f64, rgb: &mut [u8]) {
        let leds = c.leds.clamp(1, MAX_LEDS).min(rgb.len() / 3);
        let speed = SPEED_FACTOR[(c.speed.clamp(1, 5) - 1) as usize];
        let col = c.color;
        let put = |rgb: &mut [u8], i: usize, r: f64, g: f64, b: f64| {
            rgb[i * 3] = r.round().clamp(0.0, 255.0) as u8;
            rgb[i * 3 + 1] = g.round().clamp(0.0, 255.0) as u8;
            rgb[i * 3 + 2] = b.round().clamp(0.0, 255.0) as u8;
        };
        let f = |v: u8| v as f64;

        match c.mode {
            ArgbMode::Off => rgb.iter_mut().for_each(|v| *v = 0),
            ArgbMode::Static => {
                for i in 0..leds {
                    put(rgb, i, f(col.0), f(col.1), f(col.2));
                }
            }
            ArgbMode::Breathing => {
                let k = (1.0 - (t * speed / 4.0 * 2.0 * PI).cos()) / 2.0;
                let k = 0.03 + 0.97 * k * k;
                for i in 0..leds {
                    put(rgb, i, f(col.0) * k, f(col.1) * k, f(col.2) * k);
                }
            }
            ArgbMode::Spectrum => {
                let x = from_hsv((t * 36.0 * speed) % 360.0, 1.0, 1.0);
                for i in 0..leds {
                    put(rgb, i, f(x.0), f(x.1), f(x.2));
                }
            }
            ArgbMode::RainbowWave if !c.rainbow => {
                // Как в C#: без радуги бегущая волна идёт тремя своими цветами (та же скорость).
                for i in 0..leds {
                    let position = i as f64 / (leds.max(2) - 1) as f64;
                    let x = palette_color(c, position - t * speed / 3.0);
                    put(rgb, i, f(x.0), f(x.1), f(x.2));
                }
            }
            ArgbMode::RainbowWave => {
                for i in 0..leds {
                    let hue = (i as f64 * 360.0 / leds.min(24) as f64 - t * 120.0 * speed).rem_euclid(360.0);
                    let x = from_hsv(hue, 1.0, 1.0);
                    put(rgb, i, f(x.0), f(x.1), f(x.2));
                }
            }
            ArgbMode::ColorWave => {
                for i in 0..leds {
                    let position = i as f64 / (leds.max(2) - 1) as f64;
                    let x = palette_color(c, position - t * speed / 3.0);
                    put(rgb, i, f(x.0), f(x.1), f(x.2));
                }
            }
            ArgbMode::Comet => {
                let n = leds as f64;
                let tail = (n / 4.0).max(3.0);
                let head = (t * speed * 18.0) % n;
                for i in 0..leds {
                    let mut d = head - i as f64;
                    if d < 0.0 {
                        d += n;
                    }
                    let k = if d < tail { (1.0 - d / tail).powi(2) } else { 0.0 };
                    put(rgb, i, f(col.0) * k, f(col.1) * k, f(col.2) * k);
                }
            }
            ArgbMode::CpuTemp => {
                if cpu_temp.is_nan() {
                    // Датчика нет — тихое белое дыхание, видно, что программа работает.
                    let k = 0.15 + 0.1 * (t * 2.0).sin();
                    for i in 0..leds {
                        put(rgb, i, 255.0 * k, 255.0 * k, 255.0 * k);
                    }
                } else {
                    // Плавно догоняем показание, без скачков.
                    self.temp = if self.temp.is_nan() { cpu_temp } else { self.temp + (cpu_temp - self.temp) * 0.026 };
                    let x = from_hsv(temp_hue(self.temp), 1.0, 1.0);
                    for i in 0..leds {
                        put(rgb, i, f(x.0), f(x.1), f(x.2));
                    }
                }
            }
            ArgbMode::Keyboard => {
                let fans = c.fans.clamp(1, 6);
                if !matches!(&self.sampler, Some(s) if s.leds == leds && s.fans == fans) {
                    self.sampler = Some(KeyboardSampler::new(leds, fans));
                }
                let s = self.sampler.as_ref().unwrap();
                let mode = if effects::is_rainbow_flow(c.keyboard_effect) { c.keyboard_effect } else { effects::FLOW };
                // На клавиатуре горит встроенный эффект прошивки — повторяем его (LightMirror).
                let firmware = crate::light_mirror::active_effect_mode().is_some();
                for i in 0..leds {
                    let (x, y) = s.position(i);
                    if firmware {
                        if let Some(k) = crate::light_mirror::effect_sample(x, y, (i as f64 * 0.618) % 1.0) {
                            put(rgb, i, f(k.0), f(k.1), f(k.2));
                            continue;
                        }
                    }
                    // Та же формула и то же время, что у клавиатуры, — цвета идут в такт.
                    let k = effects::rainbow_at(mode, x, y, Some(s.ring(i)), t, c.keyboard_speed, c.keyboard_dir);
                    put(rgb, i, f(k.0), f(k.1), f(k.2));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn led_brightness_like_csharp() {
        assert_eq!(led_brightness(0), 0);
        assert_eq!(led_brightness(1), 10);
        assert_eq!(led_brightness(100), 100);
        assert_eq!(led_brightness(50), 55);
    }

    #[test]
    fn wave_without_rainbow_uses_palette() {
        let mut r = Renderer::new();
        let c = ArgbConfig {
            mode: ArgbMode::RainbowWave, rainbow: false, leds: 12,
            color: (255, 0, 0), color2: Some((0, 255, 0)), color3: Some((0, 0, 255)), ..Default::default()
        };
        let mut rgb = vec![0u8; 36];
        r.render(&c, 0.0, f64::NAN, &mut rgb);
        assert_eq!(&rgb[..3], &[255, 0, 0]);
        assert_eq!(board_effect_for(&c, &rgb).0, 1);
        assert_eq!(board_effect_for(&ArgbConfig { rainbow: true, ..c }, &rgb).0, 5);
    }

    fn cfg(mode: ArgbMode) -> ArgbConfig {
        ArgbConfig { mode, color: (10, 200, 30), leds: 12, ..Default::default() }
    }

    #[test]
    fn static_and_off() {
        let mut r = Renderer::new();
        let mut rgb = vec![7u8; 36];
        r.render(&cfg(ArgbMode::Static), 0.0, f64::NAN, &mut rgb);
        assert!(rgb.chunks(3).all(|p| p == [10, 200, 30]));
        r.render(&cfg(ArgbMode::Off), 0.0, f64::NAN, &mut rgb);
        assert!(rgb.iter().all(|&v| v == 0));
    }

    #[test]
    fn gamma_and_brightness() {
        assert_eq!(out(255, 1.0), 255);
        assert_eq!(out(0, 1.0), 0);
        assert!(out(128, 1.0) < 64);
        let mut dst = [0u8; 3];
        output(&[255, 255, 255], 50, &mut dst);
        assert_eq!(dst, [128, 128, 128]);
    }

    #[test]
    fn temperature_colors() {
        assert_eq!(temp_hue(20.0), 195.0);
        assert_eq!(temp_hue(60.0), 120.0);
        assert_eq!(temp_hue(100.0), 0.0);
        assert!((temp_hue(50.0) - 157.5).abs() < 1e-9);
    }

    #[test]
    fn palette_loops() {
        let c = ArgbConfig { color: (255, 0, 0), color2: Some((0, 255, 0)), color3: Some((0, 0, 255)), ..Default::default() };
        assert_eq!(palette_color(&c, 0.0), (255, 0, 0));
        assert_eq!(palette_color(&c, 1.0 / 3.0), (0, 255, 0));
        assert_eq!(palette_color(&c, 1.0), (255, 0, 0));
    }

    #[test]
    fn comet_head_is_brightest() {
        let mut r = Renderer::new();
        let mut rgb = vec![0u8; 36];
        r.render(&cfg(ArgbMode::Comet), 0.0, f64::NAN, &mut rgb);
        assert_eq!(&rgb[0..3], &[10, 200, 30]);
        assert_eq!(rgb[3 * 6 + 1], 0);
    }

    #[test]
    fn sampler_inside_keyboard() {
        let s = KeyboardSampler::new(48, 3);
        for i in 0..48 {
            let (x, y) = s.position(i);
            assert!((0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y));
        }
        assert_eq!(s.ring(0), 0.0);
        assert_eq!(s.ring(16), 0.0); // второй вентилятор начинается сверху
        assert!(s.position(0).0 < s.position(16).0 && s.position(16).0 < s.position(32).0);
    }

    #[test]
    fn sync_matches_keyboard_formula() {
        let mut r = Renderer::new();
        let c = ArgbConfig { mode: ArgbMode::Keyboard, leds: 24, fans: 2, ..Default::default() };
        let mut rgb = vec![0u8; 72];
        r.render(&c, 1.5, f64::NAN, &mut rgb);
        let s = KeyboardSampler::new(24, 2);
        let (x, y) = s.position(5);
        let k = effects::rainbow_at(effects::FLOW, x, y, Some(s.ring(5)), 1.5, 3, 0);
        assert_eq!(&rgb[15..18], &[k.0, k.1, k.2]);
    }
}
