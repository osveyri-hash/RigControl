//! Порт LightEffects.cs: свои эффекты подсветки, рисуются попиксельно и уходят
//! живыми кадрами. Работают только по кабелю.

use crate::protocol::KEY_LAYOUT;
use crate::Rgb;
use std::f64::consts::PI;

pub const WAVE_RIGHT: i32 = 100;
pub const WAVE_LEFT: i32 = 101;
pub const GRADIENT: i32 = 102;
pub const BREATHE: i32 = 103;
pub const GIF_FLOW: i32 = 104;
pub const PAINT: i32 = 105;
pub const SPECTRUM: i32 = 106;
pub const CENTER_WAVE: i32 = 107;
pub const FLOW: i32 = 108;
pub const ROTATE: i32 = 109;

/// Сколько кадров в секунду держим: больше 20 не нужно.
pub const FRAME_INTERVAL_MS: u64 = 50;

/// Полный круг цветов для волны, течения и вращения в секундах, скорости 0–5.
const FLOW_PERIOD: [f64; 6] = [9.0, 6.5, 5.0, 3.8, 2.8, 2.0];
/// Полный круг «Своего спектра» в секундах для скоростей 0–5.
const SPECTRUM_PERIOD: [f64; 6] = [24.0, 16.0, 11.0, 8.0, 5.5, 3.5];
/// Ширина клавиатуры к высоте (17 клавиш на 6 рядов).
const ASPECT: f64 = 2.8;
const FRAME_LEN: usize = 9 * 64;

pub fn is_rainbow_flow(mode: i32) -> bool {
    matches!(mode, SPECTRUM | CENTER_WAVE | FLOW | ROTATE)
}

pub fn is_custom(mode: i32) -> bool {
    mode >= 100
}

/// Цвет радужного эффекта в точке (x, y — 0…1) на момент `seconds`.
/// `ring` — для вентилятора: место светодиода на кольце (0 — верх, по часовой).
pub fn rainbow_at(mode: i32, x: f64, y: f64, ring: Option<f64>, seconds: f64, speed: i32, direction: i32) -> Rgb {
    let speed = speed.clamp(0, 5) as usize;
    if mode == SPECTRUM {
        return from_hsv(240.0 - seconds / SPECTRUM_PERIOD[speed] * 360.0, 1.0, 1.0);
    }
    let phase = seconds / FLOW_PERIOD[speed];
    let lag = match mode {
        CENTER_WAVE => {
            let dx = (x - 0.5) * ASPECT;
            let dy = y - 0.5;
            (dx * dx + dy * dy).sqrt() / 1.45
        }
        ROTATE => {
            let angle = ring.unwrap_or_else(|| (y - 0.5).atan2((x - 0.5) * ASPECT) / (2.0 * PI) + 0.25);
            let mut lag = (angle % 1.0 + 1.0) % 1.0;
            if direction.clamp(0, 3) % 2 == 1 {
                lag = 1.0 - lag;
            }
            lag
        }
        _ => match direction.clamp(0, 3) {
            1 => 1.0 - x,
            2 => y,
            3 => 1.0 - y,
            _ => x,
        },
    };
    from_hsv(240.0 - (phase - lag) * 360.0, 1.0, 1.0)
}

fn new_frame() -> Vec<u8> {
    let mut f = vec![0u8; FRAME_LEN];
    f[FRAME_LEN - 2] = 0xAA;
    f[FRAME_LEN - 1] = 0x55;
    f
}

/// Кадр своих цветов для момента `seconds`. Клавиши не гаснут полностью (~30 %).
pub fn build(mode: i32, seconds: f64, speed: i32, direction: i32, color: Rgb) -> Vec<u8> {
    let speed = speed.clamp(0, 5);
    let rate = 0.12 + 0.10 * speed as f64;
    let phase = seconds * rate % 1.0;

    let color = saturate(color, 1.35);
    let (base_h, base_s, _) = to_hsv(color);
    let mut frame = new_frame();

    for key in KEY_LAYOUT {
        let x = key.col as f64 / 17.0;
        let y = key.row as f64 / 5.0;
        match mode {
            SPECTRUM | CENTER_WAVE | FLOW | ROTATE => {
                put(&mut frame, key.key, rainbow_at(mode, x, y, None, seconds, speed, direction));
            }
            GRADIENT => {
                let hue = base_h + x * 90.0 + y * 20.0 + seconds * 22.0;
                let band = 0.5 + 0.5 * (2.0 * PI * (x * 0.5 - y * 0.25 - seconds * 0.12)).cos();
                let value = 0.62 + 0.30 * band;
                put(&mut frame, key.key, from_hsv(hue, 0.72f64.max(base_s), value));
            }
            BREATHE => {
                let pulse_rate = 0.10 + 0.06 * speed as f64;
                let wave = 0.5 + 0.5 * (2.0 * PI * (seconds * pulse_rate - x * 0.08)).sin();
                let value = 0.35 + 0.65 * wave;
                let rgb = blend(color, (255, 255, 255), 0.10 * wave);
                put(&mut frame, key.key, scale(rgb, value));
            }
            _ => {
                let d = wave_distance(mode, direction, x, y);
                let crest = 0.5 + 0.5 * (2.0 * PI * (d - phase)).cos();
                let value = 0.32 + 0.68 * crest.powf(1.3);
                let rgb = blend(color, (255, 255, 255), 0.08 * crest.powi(6));
                put(&mut frame, key.key, scale(rgb, value));
            }
        }
    }
    frame
}

/// «Продолжение гифки»: цвета правого края кадра уходят на клавиши и «вытекают» влево.
/// Паникует там же, где C# бросает исключение (пустые strips / неверный cumulative).
pub fn build_flow(strips: &[Vec<Rgb>], cumulative: &[i32], loop_ms: i32, seconds: f64, speed: i32) -> Vec<u8> {
    let speed = speed.clamp(0, 5);
    let travel = 2.6 - 0.35 * speed as f64;
    let mut frame = new_frame();
    let total = loop_ms.max(1) as i64;

    for key in KEY_LAYOUT {
        let distance = 1.0 - key.col as f64 / 17.0;
        let ms = ((seconds - distance * travel) * 1000.0).floor() as i64;
        let at = (((ms % total) + total) % total) as i32;

        let index = match cumulative.iter().position(|&v| at < v) {
            Some(i) => i,
            None => strips.len().checked_sub(1).expect("strips пуст"),
        };
        let strip = &strips[index];
        let row = key.row.clamp(0, strip.len() as i32 - 1) as usize;
        put(&mut frame, key.key, vivid(strip[row]));
    }
    frame
}

fn vivid(c: Rgb) -> Rgb {
    let (h, mut s, v) = to_hsv(c);
    if s > 0.04 {
        s = (s * 2.6 + 0.2).clamp(0.0, 0.95);
    }
    let v = (v * 1.15).clamp(0.35, 1.0);
    from_hsv(h, s, v)
}

fn wave_distance(mode: i32, direction: i32, mut x: f64, y: f64) -> f64 {
    if mode == WAVE_LEFT {
        x = 1.0 - x;
    }
    match direction.clamp(0, 3) {
        1 => 0.78 * (1.0 - x) + 0.22 * y,
        2 => 0.78 * (1.0 - x) + 0.22 * (1.0 - y),
        3 => 0.62 * (1.0 - x) + 0.38 * (y - 0.5).abs() * 2.0,
        _ => 1.0 - x,
    }
}

/// Яркость 0…1 поверх готового кадра. Меняет кадр на месте, как C#.
pub fn dim(frame: &mut [u8], scale: f64) {
    if scale >= 0.999 {
        return;
    }
    let scale = scale.clamp(0.0, 1.0);
    let limit = frame.len() as i64 - 2;
    let mut key: i64 = 1;
    while key * 4 + 3 < limit {
        let at = (key * 4) as usize;
        if frame[at] as i64 == key {
            for j in 1..=3 {
                frame[at + j] = (frame[at + j] as f64 * scale).round_ties_even() as u8;
            }
        }
        key += 1;
    }
}

fn put(frame: &mut [u8], key: u8, c: Rgb) {
    let o = key as usize * 4;
    frame[o] = key;
    frame[o + 1] = c.0;
    frame[o + 2] = c.1;
    frame[o + 3] = c.2;
}

fn saturate(c: Rgb, boost: f64) -> Rgb {
    let (h, s, v) = to_hsv(c);
    from_hsv(h, (s * boost + 0.08).clamp(0.0, 0.92), v.max(0.85))
}

/// (byte)Math.Clamp(...) — усечение к нулю, как `as u8`.
fn scale(c: Rgb, f: f64) -> Rgb {
    (
        (c.0 as f64 * f).clamp(0.0, 255.0) as u8,
        (c.1 as f64 * f).clamp(0.0, 255.0) as u8,
        (c.2 as f64 * f).clamp(0.0, 255.0) as u8,
    )
}

fn blend(a: Rgb, b: Rgb, k: f64) -> Rgb {
    let ch = |x: u8, y: u8| (x as f64 + (y as f64 - x as f64) * k).clamp(0.0, 255.0) as u8;
    (ch(a.0, b.0), ch(a.1, b.1), ch(a.2, b.2))
}

fn to_hsv(c: Rgb) -> (f64, f64, f64) {
    let (r, g, b) = (c.0 as f64 / 255.0, c.1 as f64 / 255.0, c.2 as f64 / 255.0);
    let max = r.max(g.max(b));
    let min = r.min(g.min(b));
    let delta = max - min;
    let mut h = 0.0;
    if delta > 0.0 {
        h = if max == r {
            60.0 * (((g - b) / delta) % 6.0)
        } else if max == g {
            60.0 * (((b - r) / delta) + 2.0)
        } else {
            60.0 * (((r - g) / delta) + 4.0)
        };
    }
    if h < 0.0 {
        h += 360.0;
    }
    (h, if max <= 0.0 { 0.0 } else { delta / max }, max)
}

/// С Clamp — как LightEffects.FromHsv. Округление банковское (Math.Round в .NET).
fn from_hsv(mut h: f64, s: f64, v: f64) -> Rgb {
    h %= 360.0;
    if h < 0.0 {
        h += 360.0;
    }
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    let q = |t: f64| ((t + m) * 255.0).clamp(0.0, 255.0).round_ties_even() as u8;
    (q(r), q(g), q(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_marker_and_keys() {
        let f = build(WAVE_RIGHT, 1.0, 3, 0, (255, 0, 0));
        assert_eq!(f.len(), 576);
        assert_eq!((f[574], f[575]), (0xAA, 0x55));
        for k in KEY_LAYOUT {
            assert_eq!(f[k.key as usize * 4], k.key);
        }
    }

    #[test]
    fn spectrum_starts_blue() {
        assert_eq!(rainbow_at(SPECTRUM, 0.0, 0.0, None, 0.0, 0, 0), (0, 0, 255));
    }

    #[test]
    fn dim_halves() {
        let mut f = new_frame();
        put(&mut f, 1, (200, 101, 3));
        dim(&mut f, 0.5);
        // 101*0.5 = 50.5 → 50 (банковское), 3*0.5 = 1.5 → 2
        assert_eq!(&f[4..8], &[1, 100, 50, 2]);
    }
}
