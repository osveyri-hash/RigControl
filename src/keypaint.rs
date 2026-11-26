//! Порт KeyPaint.cs: «Своя раскраска» — свой цвет на каждую клавишу.
//! Геометрия клавиш — из rgb-keyboard.xml AJAZZ.

use crate::color;
use crate::protocol::parse_color;
use std::collections::HashMap;
use std::f64::consts::PI;

/// Клавиша на схеме: код подсветки прошивки, подпись и прямоугольник (px схемы).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyCap {
    pub code: u8,
    pub label: &'static str,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Сдвиг координат XML на холст схемы (644×276).
pub const ORIGIN_X: f64 = 77.0;
pub const ORIGIN_Y: f64 = 4.0;
pub const WIDTH: f64 = 644.0;
pub const HEIGHT: f64 = 276.0;

/// Экранчик клавиатуры на схеме, координаты XML.
pub const SCREEN_X: f64 = 674.0;
pub const SCREEN_Y: f64 = 184.0;
pub const SCREEN_SIZE: f64 = 32.0;

/// Колёсико над правой колонкой, координаты XML.
pub const KNOB_X: f64 = 691.0;
pub const KNOB_Y: f64 = 36.0;
pub const KNOB_RADIUS: f64 = 18.0;

/// Режим прошивки «пользовательская подсветка».
pub const USER_LIGHT_MODE: i32 = 0x80;

/// Цвет колпачка на схеме.
pub fn cap_color(code: u8) -> &'static str {
    match code {
        102 => "#F7F6F2",
        _ => cap_color_base(code),
    }
}

/// Клавиши с рисунком. Esc и F-ряд — однотонные, «→» — белая.
pub fn has_art(code: u8) -> bool {
    !matches!(code, 1..=13 | 102)
}

fn cap_color_base(code: u8) -> &'static str {
    match code {
        1 => "#F5C21B",
        6..=9 => "#8C8D8B",
        2..=13 => "#D3D3D0",
        30 | 31 | 47 | 49 | 55 | 66 | 85 | 83 | 84 | 101 | 96 | 98 | 99 | 100 | 102 | 117 | 118 | 121 => "#1C1C1C",
        _ => "#F7F6F2",
    }
}

const fn cap(code: u8, label: &'static str, x: f64, y: f64, w: f64, h: f64) -> KeyCap {
    KeyCap { code, label, x, y, w, h }
}

pub const KEYS: &[KeyCap] = &[
    cap(1, "Esc", 95.0, 22.0, 28.0, 28.0),
    cap(2, "F1", 141.0, 22.0, 28.0, 28.0),
    cap(3, "F2", 179.0, 22.0, 28.0, 28.0),
    cap(4, "F3", 217.0, 22.0, 28.0, 28.0),
    cap(5, "F4", 255.0, 22.0, 28.0, 28.0),
    cap(6, "F5", 302.0, 22.0, 28.0, 28.0),
    cap(7, "F6", 339.0, 22.0, 28.0, 28.0),
    cap(8, "F7", 377.0, 22.0, 28.0, 28.0),
    cap(9, "F8", 415.0, 22.0, 28.0, 28.0),
    cap(10, "F9", 462.0, 22.0, 28.0, 28.0),
    cap(11, "F10", 500.0, 22.0, 28.0, 28.0),
    cap(12, "F11", 538.0, 22.0, 28.0, 28.0),
    cap(13, "F12", 576.0, 22.0, 28.0, 28.0),
    cap(119, "Del", 623.0, 22.0, 28.0, 28.0),
    cap(19, "`", 95.0, 69.0, 28.0, 28.0),
    cap(20, "1", 132.0, 69.0, 28.0, 28.0),
    cap(21, "2", 170.0, 69.0, 28.0, 28.0),
    cap(22, "3", 208.0, 69.0, 28.0, 28.0),
    cap(23, "4", 246.0, 69.0, 28.0, 28.0),
    cap(24, "5", 283.0, 69.0, 28.0, 28.0),
    cap(25, "6", 321.0, 69.0, 28.0, 28.0),
    cap(26, "7", 359.0, 69.0, 28.0, 28.0),
    cap(27, "8", 397.0, 69.0, 28.0, 28.0),
    cap(28, "9", 434.0, 69.0, 28.0, 28.0),
    cap(29, "0", 472.0, 69.0, 28.0, 28.0),
    cap(30, "-", 510.0, 69.0, 28.0, 28.0),
    cap(31, "=", 547.0, 69.0, 28.0, 28.0),
    cap(103, "⌫", 585.0, 69.0, 66.0, 28.0),
    cap(117, "Home", 677.0, 69.0, 28.0, 28.0),
    cap(37, "Tab", 95.0, 108.0, 46.0, 28.0),
    cap(38, "Q", 151.0, 108.0, 28.0, 28.0),
    cap(39, "W", 189.0, 108.0, 28.0, 28.0),
    cap(40, "E", 227.0, 108.0, 28.0, 28.0),
    cap(41, "R", 265.0, 108.0, 28.0, 28.0),
    cap(42, "T", 302.0, 108.0, 28.0, 28.0),
    cap(43, "Y", 340.0, 108.0, 28.0, 28.0),
    cap(44, "U", 378.0, 108.0, 28.0, 28.0),
    cap(45, "I", 416.0, 108.0, 28.0, 28.0),
    cap(46, "O", 453.0, 108.0, 28.0, 28.0),
    cap(47, "P", 491.0, 108.0, 28.0, 28.0),
    cap(48, "[", 529.0, 108.0, 28.0, 28.0),
    cap(49, "]", 566.0, 108.0, 28.0, 28.0),
    cap(67, "\\", 604.0, 108.0, 47.0, 28.0),
    cap(118, "PgUp", 677.0, 108.0, 28.0, 28.0),
    cap(55, "Caps", 95.0, 145.0, 55.0, 28.0),
    cap(56, "A", 160.0, 145.0, 28.0, 28.0),
    cap(57, "S", 198.0, 145.0, 28.0, 28.0),
    cap(58, "D", 236.0, 145.0, 28.0, 28.0),
    cap(59, "F", 274.0, 145.0, 28.0, 28.0),
    cap(60, "G", 312.0, 145.0, 28.0, 28.0),
    cap(61, "H", 350.0, 145.0, 28.0, 28.0),
    cap(62, "J", 387.0, 145.0, 28.0, 28.0),
    cap(63, "K", 425.0, 145.0, 28.0, 28.0),
    cap(64, "L", 462.0, 145.0, 28.0, 28.0),
    cap(65, ";", 500.0, 145.0, 28.0, 28.0),
    cap(66, "'", 538.0, 145.0, 28.0, 28.0),
    cap(85, "Enter", 576.0, 145.0, 75.0, 28.0),
    cap(121, "PgDn", 677.0, 145.0, 28.0, 28.0),
    cap(73, "Shift", 95.0, 183.0, 75.0, 28.0),
    cap(74, "Z", 179.0, 183.0, 28.0, 28.0),
    cap(75, "X", 217.0, 183.0, 28.0, 28.0),
    cap(76, "C", 255.0, 183.0, 28.0, 28.0),
    cap(77, "V", 292.0, 183.0, 28.0, 28.0),
    cap(78, "B", 330.0, 183.0, 28.0, 28.0),
    cap(79, "N", 368.0, 183.0, 28.0, 28.0),
    cap(80, "M", 406.0, 183.0, 28.0, 28.0),
    cap(81, ",", 443.0, 183.0, 28.0, 28.0),
    cap(82, ".", 481.0, 183.0, 28.0, 28.0),
    cap(83, "/", 519.0, 183.0, 28.0, 28.0),
    cap(84, "Shift", 557.0, 183.0, 55.0, 28.0),
    cap(101, "↑", 631.0, 192.0, 28.0, 28.0),
    cap(91, "Ctrl", 95.0, 221.0, 37.0, 28.0),
    cap(92, "Win", 142.0, 221.0, 37.0, 28.0),
    cap(93, "Alt", 189.0, 221.0, 37.0, 28.0),
    cap(94, "", 236.0, 221.0, 225.0, 28.0),
    cap(95, "Alt", 472.0, 221.0, 28.0, 28.0),
    cap(96, "Fn", 510.0, 221.0, 28.0, 28.0),
    cap(98, "Ctrl", 548.0, 221.0, 28.0, 28.0),
    cap(99, "←", 594.0, 230.0, 28.0, 28.0),
    cap(100, "↓", 631.0, 230.0, 28.0, 28.0),
    cap(102, "→", 669.0, 230.0, 28.0, 28.0),
];

/// Готовые наборы: имя → функция «клавиша → цвет».
pub const PRESETS: &[(&str, fn(&KeyCap) -> String)] = &[
    ("Сакура", |k| if k.y < 60.0 { "#FF2D7A".into() } else { "#FF7EB6".into() }),
    ("Зефир", |k| lerp("#FF3D9A", "#7A3DFF", (k.x - ORIGIN_X) / WIDTH)),
    ("Мята", |k| lerp("#00FFB0", "#00A0FF", (k.y - ORIGIN_Y) / HEIGHT)),
    ("Закат", |k| lerp("#FF9A00", "#FF0050", (k.x - ORIGIN_X) / WIDTH)),
    ("Радуга", |k| hue((k.x - ORIGIN_X) / WIDTH * 300.0)),
    ("WASD", |k| if matches!(k.label, "W" | "A" | "S" | "D") { "#FF0050".into() } else { "#6A3DFF".into() }),
];

/// Анимации поверх раскраски (индекс = Settings.PaintAnim).
pub const ANIMATIONS: &[&str] = &["Без анимации", "Дыхание", "Волна", "Переливы", "Мерцание", "Пульс из центра"];

/// Раскраска: код клавиши (строкой, как в settings.json) → «#RRGGBB».
pub type Paint = HashMap<String, String>;

fn new_frame() -> Vec<u8> {
    let mut f = vec![0u8; 9 * 64];
    let n = f.len();
    f[n - 2] = 0xAA;
    f[n - 1] = 0x55;
    f
}

/// Таблица цветов, как её шлёт официальная программа: 576 байт, [код, R, G, B] по смещению код×4,
/// в конце 0xAA 0x55. Яркость 0–5 применяем к самим цветам.
pub fn build_frame(paint: Option<&Paint>, brightness: i32) -> Vec<u8> {
    let scale = brightness.clamp(0, 5) as f64 / 5.0;
    let mut frame = new_frame();
    for key in KEYS {
        let at = key.code as usize * 4;
        frame[at] = key.code;
        if let Some(hex) = paint.and_then(|p| p.get(&key.code.to_string())) {
            let (r, g, b) = parse_color(Some(hex), (0, 0, 0));
            frame[at + 1] = led(r, scale);
            frame[at + 2] = led(g, scale);
            frame[at + 3] = led(b, scale);
        }
    }
    frame
}

/// Кадр раскраски с анимацией в момент `seconds`. Клавиши не гаснут полностью.
pub fn animate(paint: Option<&Paint>, brightness: i32, animation: i32, seconds: f64, speed: i32) -> Vec<u8> {
    let scale = brightness.clamp(0, 5) as f64 / 5.0;
    let rate = 0.15 + 0.12 * speed.clamp(0, 5) as f64;
    let phase = seconds * rate;
    let mut frame = new_frame();

    for key in KEYS {
        let at = key.code as usize * 4;
        frame[at] = key.code;
        let Some(hex) = paint.and_then(|p| p.get(&key.code.to_string())) else { continue };

        let (mut r, mut g, mut b) = parse_color(Some(hex), (0, 0, 0));
        let x = (key.x + key.w / 2.0 - ORIGIN_X) / WIDTH;
        let y = (key.y + key.h / 2.0 - ORIGIN_Y) / HEIGHT;
        let mut k = 1.0;

        match animation {
            1 => k = wave01(phase),
            2 => k = wave01(x * 1.2 - phase),
            3 => {
                let (h, sat, val) = color::to_hsv((r, g, b));
                (r, g, b) = color::from_hsv(h + phase * 120.0 + x * 40.0, sat, val);
            }
            4 => k = wave01(phase * 1.7 + hash(key.code)),
            5 => {
                let dx = (x - 0.5) * 2.3;
                let dy = y - 0.5;
                k = wave01((dx * dx + dy * dy).sqrt() * 1.4 - phase);
            }
            _ => {}
        }

        let level = scale * if matches!(animation, 0 | 3) { 1.0 } else { 0.22 + 0.78 * k };
        frame[at + 1] = led(r, level);
        frame[at + 2] = led(g, level);
        frame[at + 3] = led(b, level);
    }
    frame
}

/// Плавная волна 0…1 с периодом 1.
fn wave01(t: f64) -> f64 {
    0.5 + 0.5 * (2.0 * PI * t).cos()
}

fn hash(code: u8) -> f64 {
    (code as f64 * 0.61803398875) % 1.0
}

/// Цвет экрана → яркость светодиода (гамма 2,2, как у sRGB).
fn led(value: u8, scale: f64) -> u8 {
    (255.0 * (value as f64 / 255.0).powf(2.2) * scale).round_ties_even() as u8
}

fn lerp(from: &str, to: &str, t: f64) -> String {
    let t = t.clamp(0.0, 1.0);
    let a = parse_color(Some(from), (0, 0, 0));
    let b = parse_color(Some(to), (0, 0, 0));
    let mix = |x: u8, y: u8| (x as f64 + (y as f64 - x as f64) * t).round_ties_even() as u8;
    format!("#{:02X}{:02X}{:02X}", mix(a.0, b.0), mix(a.1, b.1), mix(a.2, b.2))
}

/// Чистый оттенок по кругу (насыщенность и яркость 100 %).
fn hue(h: f64) -> String {
    let h = (h % 360.0 + 360.0) % 360.0;
    let c = 1.0;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = 0.0;
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
    let q = |t: f64| ((t + m) * 255.0).round_ties_even() as u8;
    format!("#{:02X}{:02X}{:02X}", q(r), q(g), q(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_has_all_codes_and_marker() {
        let f = build_frame(None, 5);
        assert_eq!(f.len(), 576);
        assert_eq!((f[574], f[575]), (0xAA, 0x55));
        for k in KEYS {
            assert_eq!(f[k.code as usize * 4], k.code);
            assert_eq!(&f[k.code as usize * 4 + 1..k.code as usize * 4 + 4], &[0, 0, 0]);
        }
    }

    #[test]
    fn painted_key_uses_gamma() {
        let mut p = Paint::new();
        p.insert("1".into(), "#FF0000".into());
        let f = build_frame(Some(&p), 5);
        assert_eq!(&f[4..8], &[1, 255, 0, 0]);
        let f = build_frame(Some(&p), 0);
        assert_eq!(&f[4..8], &[1, 0, 0, 0]);
    }

    #[test]
    fn presets_and_helpers() {
        assert_eq!(hue(0.0), "#FF0000");
        assert_eq!(lerp("#000000", "#FFFFFF", 0.0), "#000000");
        assert_eq!(lerp("#000000", "#FFFFFF", 1.0), "#FFFFFF");
        let esc = &KEYS[0];
        assert_eq!((PRESETS[0].1)(esc), "#FF2D7A");
        assert_eq!(cap_color(1), "#F5C21B");
        assert!(!has_art(1) && has_art(20));
    }
}
