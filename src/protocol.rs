//! Порт Ak820Protocol.cs.
//! Протокол AJAZZ AK820 Pro: команды — 64-байтные feature-отчёты (0xFF13),
//! кадры экрана — 4096-байтные output-отчёты (0xFF68). Каждая команда — сессия 04 18 … 04 02.

use crate::effects;
use crate::Rgb;

pub const VENDOR_ID: u16 = 0x0C45;
/// PID проводного режима; 0x800A — вариант прошивки.
pub const PRODUCT_IDS: [u16; 2] = [0x8009, 0x800A];
pub const CONTROL_USAGE_PAGE: u16 = 0xFF13;
pub const DATA_USAGE_PAGE: u16 = 0xFF68;

pub const SCREEN_SIZE: usize = 128;
pub const CHUNK: usize = 4096;
pub const FRAME_HEADER: usize = 256;
pub const MAX_FRAMES: usize = 140;
/// Пауза между командами — как в официальном конфигураторе (35 мс).
pub const COMMAND_DELAY_MS: u64 = 35;
/// Размер живого кадра (команда 0x20): 128 записей по 4 байта.
pub const LIVE_FRAME_SIZE: usize = 512;

/// 0x04 <op>, в байте 8 — сколько пакетов данных идёт следом.
pub fn command(op: u8, count: i32, extra0: u8) -> [u8; 64] {
    let mut p = [0u8; 64];
    p[0] = 0x04;
    p[1] = op;
    p[2] = extra0;
    p[8] = (count & 0xFF) as u8;
    p[9] = (count >> 8) as u8; // как (byte)(count >> 8) в C#: усечение
    p
}

/// Пакет эффекта подсветки: режим, цвет, радуга, яркость, скорость, направление.
/// Значения по умолчанию в C#: brightness = 5, speed = 3, direction = 0.
#[allow(clippy::too_many_arguments)]
pub fn light_packet(mode: i32, r: u8, g: u8, b: u8, rainbow: bool,
    brightness: i32, speed: i32, direction: i32) -> [u8; 64] {
    let mut p = [0u8; 64];
    p[0] = mode as u8;
    if mode != 0 {
        p[1] = r;
        p[2] = g;
        p[3] = b;
        p[8] = rainbow as u8;
        p[9] = brightness.clamp(0, 5) as u8;
        p[10] = speed.clamp(0, 5) as u8;
        p[11] = direction.clamp(0, 3) as u8;
    }
    p[14] = 0xAA;
    p[15] = 0x55;
    p
}

/// Части даты/времени (вместо System.DateTime, чтобы ядро не зависело от крейтов времени).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateTimeParts {
    pub year: i32,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    /// 0 — воскресенье, как DayOfWeek в .NET.
    pub weekday: u8,
}

/// Пакет времени для экрана: год−2000, месяц, день, часы, минуты, секунды, день недели. slot по умолчанию 1.
pub fn time_packet(now: DateTimeParts, slot: u8) -> [u8; 64] {
    let mut p = [0u8; 64];
    p[1] = slot;
    p[2] = 0x5A;
    p[3] = (now.year % 2000) as u8;
    p[4] = now.month;
    p[5] = now.day;
    p[6] = now.hour;
    p[7] = now.minute;
    p[8] = now.second;
    p[10] = now.weekday % 7;
    p[62] = 0xAA;
    p[63] = 0x55;
    p
}

/// Один кадр экрана: RGB565 (128×128, little-endian) и задержка показа.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenFrame {
    pub rgb565: Vec<u8>,
    pub delay_ms: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScreenBlobError {
    /// "Нет кадров"
    NoFrames,
    /// "Кадр {i}: {len} байт вместо {expected}"
    WrongFrameSize { index: usize, len: usize, expected: usize },
}

impl std::fmt::Display for ScreenBlobError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoFrames => write!(f, "Нет кадров"),
            Self::WrongFrameSize { index, len, expected } =>
                write!(f, "Кадр {index}: {len} байт вместо {expected}"),
        }
    }
}
impl std::error::Error for ScreenBlobError {}

/// Полотно для экрана: 256-байтный заголовок (число кадров + задержки в 2 мс)
/// и кадры RGB565 подряд, добитые 0xFF до границы 4 КиБ.
pub fn screen_blob(frames: &[ScreenFrame]) -> Result<Vec<u8>, ScreenBlobError> {
    if frames.is_empty() {
        return Err(ScreenBlobError::NoFrames);
    }
    let count = frames.len().min(MAX_FRAMES);

    let mut header = [0xFFu8; FRAME_HEADER];
    header[0] = count as u8;
    for i in 0..count {
        header[1 + i] = (frames[i].delay_ms / 2).max(1).clamp(1, 255) as u8;
    }

    let pixels = SCREEN_SIZE * SCREEN_SIZE * 2;
    let total = FRAME_HEADER + count * pixels;
    let mut blob = vec![0xFFu8; total.div_ceil(CHUNK) * CHUNK];
    blob[..FRAME_HEADER].copy_from_slice(&header);

    for (i, f) in frames.iter().take(count).enumerate() {
        if f.rgb565.len() != pixels {
            return Err(ScreenBlobError::WrongFrameSize { index: i, len: f.rgb565.len(), expected: pixels });
        }
        let at = FRAME_HEADER + i * pixels;
        blob[at..at + pixels].copy_from_slice(&f.rgb565);
    }
    Ok(blob)
}

#[inline]
fn rgb565_at(data: &[u8], i: usize) -> u16 {
    data[i] as u16 | ((data[i + 1] as u16) << 8)
}

/// Средний цвет кадра RGB565. Почти чёрные пиксели пропускаем.
pub fn average_color(rgb565: &[u8]) -> Rgb {
    let (mut red, mut green, mut blue, mut count) = (0i64, 0i64, 0i64, 0i64);
    let mut i = 0;
    while i + 1 < rgb565.len() {
        let v = rgb565_at(rgb565, i);
        let pr = ((v >> 11) & 0x1F) as i64;
        let pg = ((v >> 5) & 0x3F) as i64;
        let pb = (v & 0x1F) as i64;
        i += 2;
        if pr + pg + pb < 6 {
            continue; // фон
        }
        red += (pr * 255 + 15) / 31;
        green += (pg * 255 + 31) / 63;
        blue += (pb * 255 + 15) / 31;
        count += 1;
    }
    if count == 0 {
        return (0, 0, 0);
    }
    ((red / count) as u8, (green / count) as u8, (blue / count) as u8)
}

/// Цвета правого края кадра: полосы сверху вниз, в каждой — средний цвет правых
/// `right_fraction` кадра (в C# по умолчанию 0.3).
pub fn strip_colors(rgb565: &[u8], rows: i32, right_fraction: f64) -> Vec<Rgb> {
    assert!(rows >= 0, "rows < 0"); // в C# new[rows] бросает исключение
    let size = SCREEN_SIZE as i32;
    let mut result = vec![(0u8, 0u8, 0u8); rows as usize];
    let x0 = (size as f64 * (1.0 - right_fraction)) as i32;

    for r in 0..rows {
        let y0 = r * size / rows;
        let y1 = (r + 1) * size / rows;
        let (mut red, mut green, mut blue, mut count) = (0i64, 0i64, 0i64, 0i64);
        for y in y0..y1 {
            for x in x0..size {
                let i = (y * size + x) * 2;
                if i < 0 || (i + 1) as usize >= rgb565.len() {
                    continue;
                }
                let v = rgb565_at(rgb565, i as usize) as i64;
                red += (((v >> 11) & 0x1F) * 255 + 15) / 31;
                green += (((v >> 5) & 0x3F) * 255 + 31) / 63;
                blue += ((v & 0x1F) * 255 + 15) / 31;
                count += 1;
            }
        }
        result[r as usize] = if count == 0 {
            (0, 0, 0)
        } else {
            ((red / count) as u8, (green / count) as u8, (blue / count) as u8)
        };
    }
    result
}

/// Делает цвета кадров выразительными: поднимает насыщенность и растягивает яркость.
pub fn vivid_palette(colors: &[Rgb]) -> Vec<Rgb> {
    if colors.is_empty() {
        return Vec::new();
    }
    let hsv: Vec<(f64, f64, f64)> = colors
        .iter()
        .map(|&(r, g, b)| to_hsv(r as f64, g as f64, b as f64))
        .collect();
    let min_v = hsv.iter().map(|x| x.2).fold(f64::INFINITY, f64::min);
    let max_v = hsv.iter().map(|x| x.2).fold(f64::NEG_INFINITY, f64::max);
    let span = 0.03f64.max(max_v - min_v);

    hsv.iter()
        .map(|&(h, s, v)| {
            let vivid_s = (s * 2.0 + 0.25).clamp(0.0, 0.75);
            let vivid_v = (0.72 + 0.28 * (v - min_v) / span).clamp(0.0, 1.0);
            from_hsv(h, vivid_s, vivid_v)
        })
        .collect()
}

fn to_hsv(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let (r, g, b) = (r / 255.0, g / 255.0, b / 255.0);
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

/// Без Clamp — как в Ak820Protocol.FromHsv. Math.Round в .NET — банковское округление.
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
    (
        ((r + m) * 255.0).round_ties_even() as u8,
        ((g + m) * 255.0).round_ties_even() as u8,
        ((b + m) * 255.0).round_ties_even() as u8,
    )
}

/// Палитра встроенных эффектов: имя → номер режима.
pub const EFFECTS: &[(&str, i32)] = &[
    ("Выключено", 0),
    ("Статичный цвет", 1),
    ("Одна клавиша горит", 2),
    ("Одна клавиша гаснет", 3),
    ("Мерцание", 4),
    ("Падение", 5),
    ("Разноцветный", 6),
    ("Дыхание", 7),
    ("Спектр", 8),
    ("Волна от центра", 9),
    ("Прокрутка", 10),
    ("Перекат", 11),
    ("Вращение", 12),
    ("Взрыв", 13),
    ("Запуск", 14),
    ("Круги по воде", 15),
    ("Течение", 16),
    ("Пульсация", 17),
    ("Наклон", 18),
    ("Шаттл", 19),
    // Свои эффекты: рисуются попиксельно (только по кабелю).
    ("Своя волна →", effects::WAVE_RIGHT),
    ("Своя волна ←", effects::WAVE_LEFT),
    ("Свой градиент", effects::GRADIENT),
    ("Своё дыхание", effects::BREATHE),
    ("Гифка → справа налево", effects::GIF_FLOW),
    ("Своя раскраска", effects::PAINT),
    ("Свой спектр", effects::SPECTRUM),
    ("Своя волна от центра", effects::CENTER_WAVE),
    ("Своё течение", effects::FLOW),
    ("Своё вращение", effects::ROTATE),
];

/// Клавиша с подсветкой: код прошивки, ряд (0 — верх) и колонка (0 — левый край).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyPosition {
    pub key: u8,
    pub row: i32,
    pub col: i32,
}

const fn k(key: u8, row: i32, col: i32) -> KeyPosition {
    KeyPosition { key, row, col }
}

/// Раскладка из rgb-keyboard.xml: light_index → row_col.
pub const KEY_LAYOUT: &[KeyPosition] = &[
    k(1, 0, 0), k(2, 0, 1), k(3, 0, 2), k(4, 0, 3), k(5, 0, 5),
    k(6, 0, 6), k(7, 0, 7), k(8, 0, 8), k(9, 0, 9), k(10, 0, 10),
    k(11, 0, 11), k(12, 0, 12), k(13, 0, 13),

    k(19, 1, 0), k(20, 1, 1), k(21, 1, 2), k(22, 1, 3), k(23, 1, 4),
    k(24, 1, 5), k(25, 1, 6), k(26, 1, 7), k(27, 1, 8), k(28, 1, 9),
    k(29, 1, 10), k(30, 1, 11), k(31, 1, 12), k(103, 1, 14),
    k(117, 1, 15),

    k(37, 2, 0), k(38, 2, 1), k(39, 2, 2), k(40, 2, 3), k(41, 2, 4),
    k(42, 2, 5), k(43, 2, 6), k(44, 2, 7), k(45, 2, 8), k(46, 2, 9),
    k(47, 2, 10), k(48, 2, 11), k(49, 2, 12), k(67, 2, 14), k(118, 2, 15),

    k(55, 3, 0), k(56, 3, 2), k(57, 3, 3), k(58, 3, 4), k(59, 3, 5),
    k(60, 3, 6), k(61, 3, 7), k(62, 3, 8), k(63, 3, 9), k(64, 3, 10),
    k(65, 3, 11), k(66, 3, 12), k(85, 3, 14), k(121, 3, 15),

    k(73, 4, 0), k(74, 4, 2), k(75, 4, 3), k(76, 4, 4), k(77, 4, 5),
    k(78, 4, 6), k(79, 4, 7), k(80, 4, 8), k(81, 4, 9), k(82, 4, 10),
    k(83, 4, 11), k(84, 4, 14), k(101, 4, 16),

    k(91, 5, 0), k(92, 5, 1), k(93, 5, 3), k(94, 5, 6), k(95, 5, 10),
    k(96, 5, 12), k(98, 5, 14), k(99, 5, 15), k(100, 5, 16),
    k(102, 5, 17), k(119, 0, 14),
];

/// Таблица цветов (576 байт, клавиша по смещению код×4) → живой кадр для команды 0x20:
/// записи [код, R, G, B] подряд, без пропусков. Остаток кадра — нули.
pub fn live_frame(bank: &[u8]) -> [u8; LIVE_FRAME_SIZE] {
    let mut live = [0u8; LIVE_FRAME_SIZE];
    let mut n = 0usize;
    let limit = bank.len() as i64 - 2;
    let mut code: i64 = 1;
    while code * 4 + 3 < limit && n < LIVE_FRAME_SIZE / 4 {
        let at = (code * 4) as usize;
        if bank[at] as i64 == code {
            live[n * 4..n * 4 + 4].copy_from_slice(&bank[at..at + 4]);
            n += 1;
        }
        code += 1;
    }
    live
}

/// Цвет «#RRGGBB» (или «#RGB») → байты; при ошибке — fallback (в C# по умолчанию (0,0,0)).
pub fn parse_color(hex: Option<&str>, fallback: Rgb) -> Rgb {
    let Some(hex) = hex else { return fallback };
    if hex.trim().is_empty() {
        return fallback;
    }
    let trimmed = hex.trim().trim_start_matches('#');
    // Длины в C# — в UTF-16 единицах; для валидного hex это то же самое, что символы.
    let mut text: Vec<char> = trimmed.chars().collect();
    if trimmed.encode_utf16().count() == 3 {
        text = text.iter().flat_map(|&c| [c, c]).collect();
    }
    if text.len() != 6 {
        return fallback;
    }
    // NumberStyles.HexNumber разрешает пробелы по краям и не разрешает знак.
    let part = |a: usize, b: usize| -> Option<u8> {
        let s: String = text[a..b].iter().collect();
        let s = s.trim_matches(|c: char| matches!(c, '\u{9}'..='\u{D}' | ' '));
        if s.is_empty() || !s.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        u8::from_str_radix(s, 16).ok()
    };
    match (part(0, 2), part(2, 4), part(4, 6)) {
        (Some(r), Some(g), Some(b)) => (r, g, b),
        _ => fallback,
    }
}

/// Settings.Effect — номер в списке EFFECTS (EffectCombo.SelectedIndex в C#), а не режим прошивки.
/// Режим по номеру; номер вне списка прижимается к краю, как Math.Clamp в C#.
pub fn effect_mode(index: i32) -> i32 {
    EFFECTS[index.clamp(0, EFFECTS.len() as i32 - 1) as usize].1
}

/// Номер в EFFECTS по режиму прошивки (обратное к [`effect_mode`]).
pub fn effect_index(mode: i32) -> Option<i32> {
    EFFECTS.iter().position(|e| e.1 == mode).map(|i| i as i32)
}

/// Имя эффекта по номеру в списке.
pub fn effect_name(index: i32) -> &'static str {
    EFFECTS[index.clamp(0, EFFECTS.len() as i32 - 1) as usize].0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effect_index_like_csharp() {
        assert_eq!(effect_mode(8), 8);
        assert_eq!(effect_mode(20), effects::WAVE_RIGHT);
        assert_eq!(effect_mode(25), effects::PAINT);
        assert_eq!(effect_mode(99), effects::ROTATE);
        assert_eq!(effect_index(effects::PAINT), Some(25));
        assert_eq!(effect_name(8), "Спектр");
    }

    #[test]
    fn command_layout() {
        let p = command(0x18, 0x1234, 7);
        assert_eq!(&p[..3], &[0x04, 0x18, 7]);
        assert_eq!((p[8], p[9]), (0x34, 0x12));
    }

    #[test]
    fn light_packet_off_keeps_only_marker() {
        let p = light_packet(0, 1, 2, 3, true, 5, 3, 0);
        assert_eq!(p[1..4], [0, 0, 0]);
        assert_eq!((p[14], p[15]), (0xAA, 0x55));
        let p = light_packet(7, 1, 2, 3, true, 9, -1, 8);
        assert_eq!(&p[..4], &[7, 1, 2, 3]);
        assert_eq!(&p[8..12], &[1, 5, 0, 3]);
    }

    #[test]
    fn blob_size_and_header() {
        let px = SCREEN_SIZE * SCREEN_SIZE * 2;
        let frames = vec![ScreenFrame { rgb565: vec![0; px], delay_ms: 1 }; 2];
        let blob = screen_blob(&frames).unwrap();
        assert_eq!(blob.len() % CHUNK, 0);
        assert_eq!(blob[0], 2);
        assert_eq!(blob[1], 1);
        assert_eq!(blob[3], 0xFF);
        assert_eq!(screen_blob(&[]), Err(ScreenBlobError::NoFrames));
    }

    #[test]
    fn average_white_and_black() {
        assert_eq!(average_color(&[0xFF, 0xFF, 0xFF, 0xFF]), (255, 255, 255));
        assert_eq!(average_color(&[0, 0, 0, 0]), (0, 0, 0));
    }

    #[test]
    fn parse_color_cases() {
        assert_eq!(parse_color(Some("#FF8000"), (1, 1, 1)), (255, 128, 0));
        assert_eq!(parse_color(Some(" #f80 "), (1, 1, 1)), (255, 136, 0));
        assert_eq!(parse_color(Some("zzzzzz"), (1, 1, 1)), (1, 1, 1));
        assert_eq!(parse_color(None, (1, 1, 1)), (1, 1, 1));
    }

    #[test]
    fn live_frame_packs_keys() {
        let mut bank = vec![0u8; 576];
        bank[4..8].copy_from_slice(&[1, 10, 20, 30]);
        bank[12..16].copy_from_slice(&[3, 40, 50, 60]);
        let live = live_frame(&bank);
        assert_eq!(&live[..8], &[1, 10, 20, 30, 3, 40, 50, 60]);
        assert!(live[8..].iter().all(|&b| b == 0));
    }
}
