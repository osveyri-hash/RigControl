//! Порт Cooler/LcdItem.cs + Cooler/LcdScene.cs + Cooler/LcdThemes.cs: сцена экрана помпы
//! (время, дата, датчики, полоски загрузки, свой текст) поверх градиента или картинки.
//! WPF-разметку заменяет своё рисование: шрифты через ab_glyph, картинка — крейт image.

use crate::telemetry::Snapshot;
use ab_glyph::{point, Font, FontArc, PxScale, ScaleFont};
use chrono::{Datelike, Local, NaiveDateTime, Timelike};
use image::{Rgb, RgbImage};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LcdKind {
    Time, Date, CpuLoad, CpuTemp, GpuLoad, GpuTemp, Ram, Text,
    Weekday, CpuClock, CpuPower, GpuPower, Vram, NetDown, NetUp, Disk, Uptime,
    CpuBar, GpuBar, RamBar,
}

/// Имена для командной строки и подписи как в C# (KindName).
pub const KINDS: &[(&str, LcdKind, &str)] = &[
    ("time", LcdKind::Time, "Время"),
    ("date", LcdKind::Date, "Дата"),
    ("cpu", LcdKind::CpuLoad, "Загрузка CPU"),
    ("cputemp", LcdKind::CpuTemp, "Температура CPU"),
    ("gpu", LcdKind::GpuLoad, "Загрузка GPU"),
    ("gputemp", LcdKind::GpuTemp, "Температура GPU"),
    ("ram", LcdKind::Ram, "Память (RAM)"),
    ("text", LcdKind::Text, "Свой текст"),
    ("weekday", LcdKind::Weekday, "День недели"),
    ("cpuclock", LcdKind::CpuClock, "Частота CPU"),
    ("cpupower", LcdKind::CpuPower, "Мощность CPU"),
    ("gpupower", LcdKind::GpuPower, "Мощность GPU"),
    ("vram", LcdKind::Vram, "Видеопамять"),
    ("netdown", LcdKind::NetDown, "Сеть: загрузка"),
    ("netup", LcdKind::NetUp, "Сеть: отдача"),
    ("disk", LcdKind::Disk, "Диск C:"),
    ("uptime", LcdKind::Uptime, "Время работы ПК"),
    ("cpubar", LcdKind::CpuBar, "Полоска CPU"),
    ("gpubar", LcdKind::GpuBar, "Полоска GPU"),
    ("rambar", LcdKind::RamBar, "Полоска RAM"),
];

impl LcdKind {
    pub fn parse(s: &str) -> Option<Self> {
        KINDS.iter().find(|(n, _, _)| n.eq_ignore_ascii_case(s)).map(|(_, k, _)| *k)
    }
}

#[derive(Clone, Debug)]
pub struct LcdItem {
    pub kind: LcdKind,
    pub x: f64,
    pub y: f64,
    pub scale: f64,
    /// #RRGGBB; пусто — «авто» (у температур цвет по нагреву).
    pub color: String,
    pub font: String,
    pub bold: bool,
    pub label: bool,
    pub seconds: bool,
    pub text: String,
}

pub const KAWAII: &str = "Rounded Mplus 1c";
pub const MONO: &str = "Consolas";
pub const SEGOE: &str = "Segoe UI";

impl LcdItem {
    pub fn base_size(&self) -> f64 {
        match self.kind {
            LcdKind::Time => 46.0,
            LcdKind::Date | LcdKind::Weekday => 18.0,
            LcdKind::Text => 24.0,
            LcdKind::NetDown | LcdKind::NetUp | LcdKind::Disk | LcdKind::Uptime => 20.0,
            _ => 26.0,
        }
    }

    pub fn is_bar(&self) -> bool {
        matches!(self.kind, LcdKind::CpuBar | LcdKind::GpuBar | LcdKind::RamBar)
    }

    pub fn label_text(&self) -> &'static str {
        use LcdKind::*;
        match self.kind {
            CpuLoad | CpuBar => "CPU",
            CpuTemp => "CPU °C",
            GpuLoad | GpuBar => "GPU",
            GpuTemp => "GPU °C",
            Ram | RamBar => "RAM",
            CpuClock | CpuPower => "CPU",
            GpuPower => "GPU",
            Vram => "VRAM",
            NetDown => "↓",
            NetUp => "↑",
            Disk => "C:",
            Uptime => "ПК",
            _ => "",
        }
    }

    pub fn has_label(&self) -> bool {
        !self.label_text().is_empty()
    }

    /// LcdItem.Create: цвет и жирность по умолчанию для типа.
    pub fn create(kind: LcdKind) -> Self {
        use LcdKind::*;
        let color = match kind {
            Date => "#8A90A0",
            CpuLoad => "#4DA3FF",
            CpuTemp | GpuTemp => "",
            GpuLoad => "#7BD36B",
            Ram | RamBar | Vram => "#C084FF",
            CpuBar | CpuClock | CpuPower => "#4DA3FF",
            GpuBar | GpuPower => "#7BD36B",
            NetDown => "#4DE0E0",
            NetUp => "#FF6FB5",
            Weekday | Uptime | Disk => "#8A90A0",
            _ => "#FFFFFF",
        };
        LcdItem {
            kind, x: 0.0, y: 0.0, scale: 1.0, color: color.into(), font: SEGOE.into(),
            bold: !matches!(kind, Date | Weekday), label: true, seconds: false, text: "Текст".into(),
        }
    }

    /// Расстановка по умолчанию для 320×240.
    pub fn default_layout() -> Vec<LcdItem> {
        use LcdKind::*;
        let spots = [(16.0, 6.0), (188.0, 26.0), (16.0, 82.0), (170.0, 82.0), (16.0, 130.0), (170.0, 130.0), (16.0, 178.0)];
        [Time, Date, CpuLoad, CpuTemp, GpuLoad, GpuTemp, Ram]
            .iter()
            .zip(spots)
            .map(|(k, (x, y))| LcdItem { x, y, ..LcdItem::create(*k) })
            .collect()
    }
}

// ── Темы (LcdThemes.cs) ──────────────────────────────────────────────────────

pub struct LcdTheme {
    pub name: &'static str,
    pub top: &'static str,
    pub bottom: &'static str,
    pub items: fn() -> Vec<LcdItem>,
    pub portrait: Option<fn() -> Vec<LcdItem>>,
    /// Имя встроенной картинки из оригинала (Assets/…); в Rust-версии её передают через --bg.
    pub image: Option<&'static str>,
    pub brightness: Option<i32>,
}

fn make(kind: LcdKind, x: f64, y: f64, scale: f64, color: &str, font: &str) -> LcdItem {
    let mut item = LcdItem::create(kind);
    item.x = x;
    item.y = y;
    item.scale = scale;
    item.color = color.into();
    item.font = font.into();
    item.bold = !matches!(kind, LcdKind::Date | LcdKind::Weekday);
    item
}

fn portrait_classic() -> Vec<LcdItem> {
    use LcdKind::*;
    let (time, date, cpu, gpu, ram, font, temp) = ("#FFFFFF", "#8A90A0", "#4DA3FF", "#7BD36B", "#C084FF", SEGOE, "");
    vec![
        make(Time, 18.0, 12.0, 1.45, time, font),
        make(Date, 22.0, 90.0, 1.0, date, font),
        make(Weekday, 22.0, 114.0, 0.9, date, font),
        make(CpuLoad, 18.0, 156.0, 0.95, cpu, font),
        make(CpuTemp, 126.0, 156.0, 0.95, temp, font),
        make(GpuLoad, 18.0, 202.0, 0.95, gpu, font),
        make(GpuTemp, 126.0, 202.0, 0.95, temp, font),
        make(Ram, 18.0, 258.0, 0.85, ram, font),
    ]
}

fn anime(time: &str, muted: &str, accent: &str) -> Vec<LcdItem> {
    use LcdKind::*;
    vec![
        make(Time, 12.0, 4.0, 1.25, time, KAWAII),
        make(Date, 14.0, 66.0, 0.85, muted, KAWAII),
        make(Weekday, 112.0, 66.0, 0.8, muted, KAWAII),
        make(CpuLoad, 10.0, 96.0, 0.78, accent, KAWAII),
        make(CpuTemp, 168.0, 96.0, 0.78, accent, KAWAII),
        make(GpuLoad, 10.0, 124.0, 0.78, accent, KAWAII),
        make(GpuTemp, 168.0, 124.0, 0.78, accent, KAWAII),
        make(Ram, 10.0, 152.0, 0.72, time, KAWAII),
    ]
}

fn anime_portrait(time: &str, muted: &str, accent: &str) -> Vec<LcdItem> {
    use LcdKind::*;
    vec![
        make(Time, 10.0, 6.0, 1.25, time, KAWAII),
        make(Date, 12.0, 64.0, 0.85, muted, KAWAII),
        make(Weekday, 100.0, 64.0, 0.8, muted, KAWAII),
        make(CpuLoad, 8.0, 92.0, 0.75, accent, KAWAII),
        make(CpuTemp, 128.0, 92.0, 0.75, accent, KAWAII),
        make(GpuLoad, 8.0, 118.0, 0.75, accent, KAWAII),
        make(GpuTemp, 128.0, 118.0, 0.75, accent, KAWAII),
        make(Ram, 8.0, 144.0, 0.7, time, KAWAII),
    ]
}

fn ascii(time: &str, muted: &str, accent: &str, text: &str) -> Vec<LcdItem> {
    use LcdKind::*;
    vec![
        make(Time, 16.0, 6.0, 1.4, time, MONO),
        make(Date, 20.0, 82.0, 0.9, muted, MONO),
        make(Weekday, 126.0, 82.0, 0.9, muted, MONO),
        make(CpuLoad, 16.0, 112.0, 0.85, accent, MONO),
        make(CpuTemp, 168.0, 112.0, 0.85, "", MONO),
        make(GpuLoad, 16.0, 146.0, 0.85, accent, MONO),
        make(GpuTemp, 168.0, 146.0, 0.85, "", MONO),
        make(Ram, 16.0, 182.0, 0.75, text, MONO),
        make(NetDown, 16.0, 214.0, 0.6, muted, MONO),
        make(NetUp, 168.0, 214.0, 0.6, muted, MONO),
    ]
}

fn ascii_portrait(time: &str, muted: &str, accent: &str, text: &str) -> Vec<LcdItem> {
    use LcdKind::*;
    vec![
        make(Time, 14.0, 10.0, 1.25, time, MONO),
        make(Date, 18.0, 80.0, 0.9, muted, MONO),
        make(Weekday, 18.0, 104.0, 0.9, muted, MONO),
        make(CpuLoad, 14.0, 142.0, 0.85, accent, MONO),
        make(CpuTemp, 124.0, 142.0, 0.85, "", MONO),
        make(GpuLoad, 14.0, 180.0, 0.85, accent, MONO),
        make(GpuTemp, 124.0, 180.0, 0.85, "", MONO),
        make(Ram, 14.0, 222.0, 0.75, text, MONO),
        make(NetDown, 14.0, 262.0, 0.65, muted, MONO),
        make(NetUp, 14.0, 288.0, 0.65, muted, MONO),
    ]
}

macro_rules! theme {
    ($name:expr, $top:expr, $bottom:expr, $f:ident, $fp:ident, ($($a:expr),*), $img:expr, $br:expr) => {
        LcdTheme {
            name: $name, top: $top, bottom: $bottom,
            items: || $f($($a),*), portrait: Some(|| $fp($($a),*)),
            image: $img, brightness: $br,
        }
    };
}

pub fn default_theme() -> LcdTheme {
    LcdTheme {
        name: "Классика", top: "#10121A", bottom: "#05060A",
        items: LcdItem::default_layout,
        portrait: Some(portrait_classic),
        image: None, brightness: None,
    }
}

pub fn all_themes() -> Vec<LcdTheme> {
    vec![
        default_theme(),
        theme!("ASCII", "#F4F4F4", "#DCDCDC", ascii, ascii_portrait, ("#262626", "#6E6E6E", "#D94C86", "#3A3A3A"), Some("AsciiArt.jpg"), Some(100)),
        theme!("ASCII ночь", "#1C1C1C", "#0C0C0C", ascii, ascii_portrait, ("#F2F2F2", "#9A9A9A", "#FF8FB8", "#D6D6D6"), Some("AsciiArtDark.jpg"), Some(70)),
        theme!("Терминал", "#161616", "#0A0A0A", ascii, ascii_portrait, ("#E6E6E6", "#7A7A7A", "#FF8FB8", "#BDBDBD"), None, None),
        theme!("Манга", "#141414", "#080808", ascii, ascii_portrait, ("#FFFFFF", "#B4B4B4", "#FF8FB8", "#E6E6E6"), Some("ArtMangaDark.jpg"), Some(55)),
        theme!("Точки", "#141414", "#080808", ascii, ascii_portrait, ("#FFFFFF", "#B4B4B4", "#FF8FB8", "#E6E6E6"), Some("ArtDotsDark.jpg"), Some(60)),
        theme!("Хина", "#8E6A7E", "#5A3E4F", anime, anime_portrait, ("#5E4652", "#A2798A", "#D9608D"), Some("AnimeHina.gif"), Some(100)),
        theme!("Кружево", "#8C7A82", "#5A4A52", anime, anime_portrait, ("#57474F", "#9C8894", "#B0798F"), Some("AnimeLace.gif"), Some(100)),
        theme!("Взгляд", "#8C7A82", "#5A4A52", anime, anime_portrait, ("#57474F", "#9C8894", "#B0798F"), Some("AnimeEyes.gif"), Some(100)),
        theme!("Ушки", "#B08A76", "#6B4A3F", anime, anime_portrait, ("#6B4A3F", "#A98476", "#E0798F"), Some("AnimeEars.gif"), Some(100)),
        theme!("Мейд", "#8A8492", "#4E4550", anime, anime_portrait, ("#4E4550", "#908494", "#8D7A95"), Some("AnimeMaid.gif"), Some(100)),
        theme!("Тепло", "#7A5A4E", "#402822", anime, anime_portrait, ("#FFF3EA", "#E9CEC2", "#FFC1A0"), Some("AnimeWarm.jpg"), Some(100)),
    ]
}

/// ApplyTheme из MainWindow.Cooler.cs (без раскладки): цвета фона, встроенная картинка и яркость темы.
pub fn apply_theme_background(theme: &LcdTheme, s: &mut crate::settings::Settings) {
    s.cooler_bg_top = Some(theme.top.to_string());
    s.cooler_bg_bottom = Some(theme.bottom.to_string());
    s.cooler_background = theme.image.map(crate::pump_art::pack_uri);
    if let Some(b) = theme.brightness {
        s.cooler_brightness = b;
    }
    s.cooler_theme = Some(theme.name.to_string());
}

pub fn find_theme(name: &str) -> Option<LcdTheme> {
    all_themes().into_iter().find(|t| t.name.to_lowercase() == name.to_lowercase())
}

// ── Форматирование значений (ValueText) ──────────────────────────────────────

/// Формат .NET "0": округление от нуля (а не к чётному).
pub fn f0(v: f64) -> String {
    format!("{}", v.round() as i64)
}

/// Формат .NET "0.0" / "0.00".
pub fn fdec(v: f64, digits: i32) -> String {
    let k = 10f64.powi(digits);
    format!("{:.*}", digits as usize, (v * k).round() / k)
}

const WEEKDAYS: [&str; 7] = ["Понедельник", "Вторник", "Среда", "Четверг", "Пятница", "Суббота", "Воскресенье"];

pub fn speed(bytes_per_second: f64) -> String {
    let bits = bytes_per_second * 8.0;
    if bits >= 1e9 {
        format!("{} Гбит/с", fdec(bits / 1e9, 1))
    } else if bits >= 1e6 {
        format!("{} Мбит/с", fdec(bits / 1e6, 1))
    } else if bits >= 1e3 {
        format!("{} Кбит/с", f0(bits / 1e3))
    } else {
        "0 Кбит/с".into()
    }
}

pub fn value_text(item: &LcdItem, s: &Snapshot, now: &NaiveDateTime) -> String {
    use LcdKind::*;
    let dash = || "—".to_string();
    match item.kind {
        Time => if item.seconds {
            format!("{:02}:{:02}:{:02}", now.hour(), now.minute(), now.second())
        } else {
            format!("{:02}:{:02}", now.hour(), now.minute())
        },
        Date => format!("{:02}.{:02}.{:04}", now.day(), now.month(), now.year()),
        CpuLoad => format!("{}%", f0(s.cpu_load)),
        CpuTemp => s.cpu_temp.map(|t| format!("{}°", f0(t))).unwrap_or_else(dash),
        GpuLoad => s.gpu_load.map(|t| format!("{}%", f0(t))).unwrap_or_else(dash),
        GpuTemp => s.gpu_temp.map(|t| format!("{}°", f0(t))).unwrap_or_else(dash),
        Ram => if s.ram_total_gb > 0.0 {
            format!("{}/{} ГБ", fdec(s.ram_used_gb, 1), f0(s.ram_total_gb))
        } else { dash() },
        Weekday => WEEKDAYS[now.weekday().num_days_from_monday() as usize].to_string(),
        CpuClock => s.cpu_clock_mhz.map(|m| format!("{} ГГц", fdec(m / 1000.0, 2))).unwrap_or_else(dash),
        CpuPower => s.cpu_power_w.map(|w| format!("{} Вт", f0(w))).unwrap_or_else(dash),
        GpuPower => s.gpu_power_w.map(|w| format!("{} Вт", f0(w))).unwrap_or_else(dash),
        Vram => match (s.vram_used_gb, s.vram_total_gb) {
            (Some(u), Some(t)) if t > 0.0 => format!("{}/{} ГБ", fdec(u, 1), f0(t)),
            (Some(u), _) => format!("{} ГБ", fdec(u, 1)),
            _ => dash(),
        },
        NetDown => speed(s.net_down_bps),
        NetUp => speed(s.net_up_bps),
        Disk => if s.disk_free_gb > 0.0 {
            format!("{}% · своб. {} ГБ", f0(s.disk_used_pct), f0(s.disk_free_gb))
        } else { dash() },
        Uptime => {
            let total = s.uptime_secs;
            let (d, h, m) = (total / 86400, total / 3600 % 24, total / 60 % 60);
            if d >= 1 { format!("{d} д {h} ч") } else { format!("{h} ч {m:02} мин") }
        }
        Text | CpuBar | GpuBar | RamBar => item.text.clone(),
    }
}

fn bar_value(item: &LcdItem, s: &Snapshot) -> Option<f64> {
    match item.kind {
        LcdKind::CpuBar => Some(s.cpu_load),
        LcdKind::GpuBar => s.gpu_load,
        LcdKind::RamBar => Some(s.ram_load),
        _ => None,
    }
}

pub fn heat(temp: Option<f64>) -> crate::Rgb {
    match temp {
        None => (0x6B, 0x70, 0x80),
        Some(t) if t < 60.0 => (0x5F, 0xD3, 0x8D),
        Some(t) if t < 80.0 => (0xFF, 0xC1, 0x4D),
        _ => (0xFF, 0x5C, 0x5C),
    }
}

/// "#RRGGBB" / "RRGGBB" / "#AARRGGBB" (альфа отбрасывается). Пусто или мусор — None.
pub fn parse_color(text: &str) -> Option<crate::Rgb> {
    let t = text.trim().trim_start_matches('#');
    // Только шестнадцатеричные цифры: срез по байтам на «не-ASCII» тексте паниковал бы посреди символа,
    // а from_str_radix принял бы знак («+FFFFF»).
    if !matches!(t.len(), 6 | 8) || !t.bytes().all(|b| b.is_ascii_hexdigit()) { return None; }
    let v = u32::from_str_radix(t, 16).ok()?;
    Some(((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

fn item_color(item: &LcdItem, s: &Snapshot) -> crate::Rgb {
    if item.color.trim().is_empty() {
        return match item.kind {
            LcdKind::CpuTemp => heat(s.cpu_temp),
            LcdKind::GpuTemp => heat(s.gpu_temp),
            _ => (255, 255, 255),
        };
    }
    parse_color(&item.color).unwrap_or((255, 255, 255))
}

const MUTED: crate::Rgb = (0x8A, 0x90, 0xA0);

/// Отпечаток сцены: не поменялся — кадр можно не перерисовывать (LcdScene.Signature).
pub fn signature(items: &[LcdItem], s: &Snapshot, now: &NaiveDateTime) -> String {
    let mut sb = String::with_capacity(256);
    for item in items {
        sb.push_str(&format!(
            "{:?}|{}|{}|{}|{}|{}|{}{}{}|{}|",
            item.kind, item.x, item.y, item.scale, item.color, item.font, item.bold, item.label, item.seconds, item.text
        ));
        if item.is_bar() {
            sb.push_str(&f0(bar_value(item, s).unwrap_or(-1.0)));
        } else {
            sb.push_str(&value_text(item, s, now));
        }
        if item.color.trim().is_empty() && matches!(item.kind, LcdKind::CpuTemp | LcdKind::GpuTemp) {
            sb.push_str(&format!("{:?}", heat(if item.kind == LcdKind::CpuTemp { s.cpu_temp } else { s.gpu_temp })));
        }
        sb.push(';');
    }
    sb
}

// ── Шрифты (AppFonts.cs) ─────────────────────────────────────────────────────

static MPLUS_REGULAR: &[u8] = include_bytes!("../fonts/MPLUSRounded1c-Regular.ttf");
static MPLUS_BOLD: &[u8] = include_bytes!("../fonts/MPLUSRounded1c-Bold.ttf");

/// Шрифты по имени семейства: встроенный Rounded Mplus 1c, Segoe UI / Consolas / Arial из C:\Windows\Fonts,
/// путь к .ttf/.otf — как есть. Не нашлось — M PLUS Rounded (в нём есть кириллица).
#[derive(Default)]
pub struct Fonts {
    cache: HashMap<(String, bool), FontArc>,
    pub fallback_used: Vec<String>,
}

fn windows_fonts_dir() -> std::path::PathBuf {
    let root = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    std::path::Path::new(&root).join("Fonts")
}

fn system_font_files(name: &str, bold: bool) -> Vec<&'static str> {
    match (name.to_lowercase().as_str(), bold) {
        ("segoe ui", true) => vec!["seguisb.ttf", "segoeuib.ttf", "segoeui.ttf"],
        ("segoe ui", false) => vec!["segoeui.ttf"],
        ("consolas", true) => vec!["consolab.ttf", "consola.ttf"],
        ("consolas", false) => vec!["consola.ttf"],
        ("arial", true) => vec!["arialbd.ttf", "arial.ttf"],
        ("arial", false) => vec!["arial.ttf"],
        ("bahnschrift", _) => vec!["bahnschrift.ttf"],
        _ => vec![],
    }
}

impl Fonts {
    pub fn get(&mut self, name: &str, bold: bool) -> FontArc {
        let name = if name.trim().is_empty() { SEGOE } else { name };
        let key = (name.to_lowercase(), bold);
        if let Some(f) = self.cache.get(&key) {
            return f.clone();
        }
        let font = self.load(name, bold);
        self.cache.insert(key, font.clone());
        font
    }

    fn load(&mut self, name: &str, bold: bool) -> FontArc {
        let builtin = || FontArc::try_from_slice(if bold { MPLUS_BOLD } else { MPLUS_REGULAR }).expect("встроенный шрифт");
        if name.eq_ignore_ascii_case(KAWAII) {
            return builtin();
        }
        let path = std::path::Path::new(name);
        let mut candidates: Vec<std::path::PathBuf> = Vec::new();
        if path.extension().is_some() {
            candidates.push(path.to_path_buf());
            candidates.push(windows_fonts_dir().join(path));
        }
        for f in system_font_files(name, bold) {
            candidates.push(windows_fonts_dir().join(f));
        }
        for c in candidates {
            if let Ok(data) = std::fs::read(&c) {
                if let Ok(font) = FontArc::try_from_vec(data) {
                    return font;
                }
            }
        }
        if !self.fallback_used.iter().any(|n| n == name) {
            self.fallback_used.push(name.to_string());
        }
        builtin()
    }
}

// ── Рисование ────────────────────────────────────────────────────────────────

struct Canvas<'a> {
    img: &'a mut RgbImage,
}

impl Canvas<'_> {
    fn blend(&mut self, x: i64, y: i64, c: crate::Rgb, a: f32) {
        if x < 0 || y < 0 || x >= self.img.width() as i64 || y >= self.img.height() as i64 || a <= 0.0 {
            return;
        }
        let a = a.min(1.0);
        let p = self.img.get_pixel_mut(x as u32, y as u32);
        let mix = |d: u8, s: u8| (d as f32 + (s as f32 - d as f32) * a).round().clamp(0.0, 255.0) as u8;
        *p = Rgb([mix(p[0], c.0), mix(p[1], c.1), mix(p[2], c.2)]);
    }

    /// Скруглённый прямоугольник (радиус = половина высоты, как CornerRadius(height/2)) со сглаживанием.
    fn pill(&mut self, x: f64, y: f64, w: f64, h: f64, c: crate::Rgb, alpha: f32) {
        let r = (h / 2.0).min(w / 2.0);
        let (x0, y0) = (x.floor() as i64, y.floor() as i64);
        let (x1, y1) = ((x + w).ceil() as i64, (y + h).ceil() as i64);
        for py in y0..y1 {
            for px in x0..x1 {
                let mut cover = 0;
                for sy in 0..4 {
                    for sx in 0..4 {
                        let fx = px as f64 + (sx as f64 + 0.5) / 4.0;
                        let fy = py as f64 + (sy as f64 + 0.5) / 4.0;
                        if fx < x || fx > x + w || fy < y || fy > y + h { continue; }
                        let cx = fx.clamp(x + r, x + w - r);
                        let cy = fy.clamp(y + r, y + h - r);
                        if (fx - cx).powi(2) + (fy - cy).powi(2) <= r * r { cover += 1; }
                    }
                }
                if cover > 0 {
                    self.blend(px, py, c, alpha * cover as f32 / 16.0);
                }
            }
        }
    }

    /// Строка текста; baseline — y базовой линии. Возвращает ширину.
    fn text(&mut self, font: &FontArc, size: f64, x: f64, baseline: f64, s: &str, c: crate::Rgb) -> f64 {
        let scale = PxScale::from(size as f32);
        let scaled = font.as_scaled(scale);
        let mut caret = x as f32;
        let mut prev = None;
        for ch in s.chars() {
            let id = scaled.glyph_id(ch);
            if let Some(p) = prev { caret += scaled.kern(p, id); }
            let glyph = id.with_scale_and_position(scale, point(caret, baseline as f32));
            caret += scaled.h_advance(id);
            prev = Some(id);
            if let Some(outline) = font.outline_glyph(glyph) {
                let b = outline.px_bounds();
                let (bx, by) = (b.min.x as i64, b.min.y as i64);
                outline.draw(|gx, gy, cov| {
                    self.blend(bx + gx as i64, by + gy as i64, c, cov);
                });
            }
        }
        caret as f64 - x
    }
}

fn ascent(font: &FontArc, size: f64) -> f64 {
    font.as_scaled(PxScale::from(size as f32)).ascent() as f64
}

fn line_height(font: &FontArc, size: f64) -> f64 {
    let s = font.as_scaled(PxScale::from(size as f32));
    (s.ascent() - s.descent() + s.line_gap()) as f64
}

/// Фон сцены: картинка или вертикальный градиент top→bottom.
pub enum Background<'a> {
    Image(&'a RgbImage),
    Gradient(crate::Rgb, crate::Rgb),
}

/// LcdScene.Render: сцена в картинку width×height.
/// brightness 0..1 гасит только фон (в WPF это Opacity слоя фона над чёрным холстом).
#[allow(clippy::too_many_arguments)]
pub fn render(
    width: u32, height: u32, items: &[LcdItem], s: &Snapshot, now: &NaiveDateTime,
    background: Background, brightness: f64, fonts: &mut Fonts,
) -> RgbImage {
    let k = brightness.clamp(0.0, 1.0);
    let mut img = match background {
        Background::Image(bg) => {
            let src = if bg.dimensions() == (width, height) {
                bg.clone()
            } else {
                image::imageops::resize(bg, width, height, image::imageops::FilterType::Triangle)
            };
            RgbImage::from_fn(width, height, |x, y| {
                let p = src.get_pixel(x, y);
                Rgb([(p[0] as f64 * k) as u8, (p[1] as f64 * k) as u8, (p[2] as f64 * k) as u8])
            })
        }
        Background::Gradient(top, bottom) => RgbImage::from_fn(width, height, |_, y| {
            let t = if height > 1 { y as f64 / (height - 1) as f64 } else { 0.0 };
            let l = |a: u8, b: u8| ((a as f64 + (b as f64 - a as f64) * t) * k).round() as u8;
            Rgb([l(top.0, bottom.0), l(top.1, bottom.1), l(top.2, bottom.2)])
        }),
    };

    let mut canvas = Canvas { img: &mut img };
    for item in items {
        if item.is_bar() {
            draw_bar(&mut canvas, item, s, fonts);
        } else {
            draw_text(&mut canvas, item, s, now, fonts);
        }
    }
    img
}

fn draw_text(canvas: &mut Canvas, item: &LcdItem, s: &Snapshot, now: &NaiveDateTime, fonts: &mut Fonts) {
    let size = (item.base_size() * item.scale).max(6.0);
    let main = fonts.get(&item.font, item.bold);
    // Как TextBlock: верх блока в (X, Y), базовая линия — по основному (крупному) шрифту строки.
    let base = item.y + ascent(&main, size);
    let mut x = item.x;
    if item.has_label() && item.label {
        let light = fonts.get(&item.font, false);
        let label = format!("{} ", item.label_text());
        x += canvas.text(&light, (size * 0.5).max(6.0), x, base, &label, MUTED);
    }
    canvas.text(&main, size, x, base, &value_text(item, s, now), item_color(item, s));
}

fn draw_bar(canvas: &mut Canvas, item: &LcdItem, s: &Snapshot, fonts: &mut Fonts) {
    let scale = item.scale.max(0.3);
    let width = 140.0 * scale;
    let height = 14.0 * scale;
    let raw = bar_value(item, s);
    let value = raw.unwrap_or(0.0).clamp(0.0, 100.0);
    let color = item_color(item, s);
    let font_size = (height * 0.95).max(6.0);
    let light = fonts.get(&item.font, false);
    let value_font = fonts.get(&item.font, item.bold);

    // Горизонтальный StackPanel: всё центрируется по вертикали.
    let row_h = height.max(line_height(&light, font_size));
    let center = item.y + row_h / 2.0;
    let text_base = |f: &FontArc| {
        let sc = f.as_scaled(PxScale::from(font_size as f32));
        center + (sc.ascent() + sc.descent()) as f64 / 2.0
    };

    let mut x = item.x;
    if item.label {
        x += canvas.text(&light, font_size, x, text_base(&light), item.label_text(), MUTED);
        x += 6.0 * scale;
    }
    let top = center - height / 2.0;
    canvas.pill(x, top, width, height, (255, 255, 255), 0x40 as f32 / 255.0);
    if value > 0.0 {
        canvas.pill(x, top, height.max(width * value / 100.0), height, color, 1.0);
    }
    x += width + 6.0 * scale;
    let label = if raw.is_none() { "—".to_string() } else { format!("{}%", f0(value)) };
    canvas.text(&value_font, font_size, x, text_base(&value_font), &label, color);
}

pub fn now() -> NaiveDateTime {
    Local::now().naive_local()
}

/// Передвинуть объекты под другой размер сцены, не меняя их величину (поворот экрана в редакторе:
/// туда и обратно размер объектов не должен «усыхать»).
pub fn stretch(items: &[LcdItem], from: (f64, f64), to: (f64, f64)) -> Vec<LcdItem> {
    let (kx, ky) = (to.0 / from.0, to.1 / from.1);
    items.iter().map(|i| LcdItem { x: i.x * kx, y: i.y * ky, ..i.clone() }).collect()
}

/// Подогнать раскладку под экран другого размера, как ThemeItems в C#: места — по осям,
/// величина объектов — по меньшему из двух коэффициентов (иначе на 640×480 объекты разъезжаются,
/// оставаясь мелкими).
pub fn fit(items: &[LcdItem], from: (f64, f64), to: (f64, f64)) -> Vec<LcdItem> {
    let (kx, ky) = (to.0 / from.0, to.1 / from.1);
    let ks = kx.min(ky);
    items
        .iter()
        .map(|i| LcdItem {
            x: (i.x * kx).round_ties_even(),
            y: (i.y * ky).round_ties_even(),
            scale: (i.scale * ks * 100.0).round_ties_even() / 100.0,
            ..i.clone()
        })
        .collect()
}

/// Раскладка темы для сцены w×h (вертикальная — portrait).
pub fn layout_for(theme: &LcdTheme, w: u32, h: u32) -> Vec<LcdItem> {
    if h > w {
        if let Some(p) = theme.portrait {
            let items = p();
            return if (w, h) == (240, 320) { items } else { fit(&items, (240.0, 320.0), (w as f64, h as f64)) };
        }
        return fit(&(theme.items)(), (320.0, 240.0), (w as f64, h as f64));
    }
    let items = (theme.items)();
    if (w, h) == (320, 240) { items } else { fit(&items, (320.0, 240.0), (w as f64, h as f64)) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn t() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, 21).unwrap().and_hms_opt(9, 5, 7).unwrap()
    }

    #[test]
    fn values() {
        let s = Snapshot { cpu_load: 12.5, cpu_temp: Some(64.4), ram_used_gb: 11.26, ram_total_gb: 31.9, uptime_secs: 3 * 3600 + 7 * 60, ..Default::default() };
        let mk = LcdItem::create;
        assert_eq!(value_text(&mk(LcdKind::Time), &s, &t()), "09:05");
        assert_eq!(value_text(&LcdItem { seconds: true, ..mk(LcdKind::Time) }, &s, &t()), "09:05:07");
        assert_eq!(value_text(&mk(LcdKind::Date), &s, &t()), "21.11.2026");
        assert_eq!(value_text(&mk(LcdKind::Weekday), &s, &t()), "Суббота");
        assert_eq!(value_text(&mk(LcdKind::CpuLoad), &s, &t()), "13%"); // .NET: 12.5 → 13
        assert_eq!(value_text(&mk(LcdKind::CpuTemp), &s, &t()), "64°");
        assert_eq!(value_text(&mk(LcdKind::GpuTemp), &s, &t()), "—");
        assert_eq!(value_text(&mk(LcdKind::Ram), &s, &t()), "11.3/32 ГБ");
        assert_eq!(value_text(&mk(LcdKind::Uptime), &s, &t()), "3 ч 07 мин");
    }

    #[test]
    fn speeds_and_heat() {
        assert_eq!(speed(0.0), "0 Кбит/с");
        assert_eq!(speed(1_000.0), "8 Кбит/с");
        assert_eq!(speed(2_500_000.0), "20.0 Мбит/с");
        assert_eq!(heat(None), (0x6B, 0x70, 0x80));
        assert_eq!(heat(Some(59.9)), (0x5F, 0xD3, 0x8D));
        assert_eq!(heat(Some(80.0)), (0xFF, 0x5C, 0x5C));
    }

    #[test]
    fn themes_and_colors() {
        assert_eq!(all_themes().len(), 12);
        assert!(find_theme("тепло").is_some());
        assert_eq!(parse_color("#4DA3FF"), Some((0x4D, 0xA3, 0xFF)));
        assert_eq!(parse_color("FF4DA3FF"), Some((0x4D, 0xA3, 0xFF)));
        assert_eq!(parse_color(""), None);
        assert_eq!(LcdItem::default_layout().len(), 7);
    }

    #[test]
    fn renders_without_panic() {
        let mut fonts = Fonts::default();
        let s = Snapshot { cpu_load: 40.0, ram_load: 55.0, ..Default::default() };
        let mut items = LcdItem::default_layout();
        items.push(LcdItem { x: 16.0, y: 214.0, ..LcdItem::create(LcdKind::CpuBar) });
        let img = render(320, 240, &items, &s, &t(), Background::Gradient((0x10, 0x12, 0x1A), (5, 6, 10)), 1.0, &mut fonts);
        assert_eq!(img.dimensions(), (320, 240));
        assert!(img.pixels().any(|p| p[0] > 200 && p[1] > 200 && p[2] > 200), "текст должен быть нарисован");
    }
}

// ── Своя раскладка (Settings.CoolerLayout из C#) и редактор ─────────────────
// В settings.json раскладка хранится как в C#: [{"Kind":0,"X":16,"Y":6,"Scale":1,"Color":"#FFFFFF",...}],
// Kind — номер в enum LcdKind (новые типы только в конец). Так файл общий для обеих версий.

/// Порядок как в C# enum LcdKind — по нему Kind хранится числом.
pub const KIND_ORDER: [LcdKind; 20] = [
    LcdKind::Time, LcdKind::Date, LcdKind::CpuLoad, LcdKind::CpuTemp, LcdKind::GpuLoad, LcdKind::GpuTemp,
    LcdKind::Ram, LcdKind::Text, LcdKind::Weekday, LcdKind::CpuClock, LcdKind::CpuPower, LcdKind::GpuPower,
    LcdKind::Vram, LcdKind::NetDown, LcdKind::NetUp, LcdKind::Disk, LcdKind::Uptime,
    LcdKind::CpuBar, LcdKind::GpuBar, LcdKind::RamBar,
];

/// Размер, под который сохранена раскладка: 320×240 или 240×320 (портрет).
pub const LAYOUT_BASE: (u32, u32) = (320, 240);

impl LcdKind {
    pub fn index(self) -> usize {
        KIND_ORDER.iter().position(|k| *k == self).unwrap_or(0)
    }
    /// Подпись по-русски (KindName в C#).
    pub fn title(self) -> &'static str {
        KINDS.iter().find(|(_, k, _)| *k == self).map(|t| t.2).unwrap_or("?")
    }
}

pub fn item_from_json(v: &serde_json::Value) -> Option<LcdItem> {
    use serde_json::Value;
    let kind = match v.get("Kind")? {
        Value::Number(n) => *KIND_ORDER.get(n.as_u64()? as usize)?,
        Value::String(s) => KIND_ORDER.iter().copied()
            .find(|k| format!("{k:?}").eq_ignore_ascii_case(s))
            .or_else(|| LcdKind::parse(s))?,
        _ => return None,
    };
    let mut it = LcdItem::create(kind);
    let f = |k: &str| v.get(k).and_then(Value::as_f64);
    let b = |k: &str| v.get(k).and_then(Value::as_bool);
    let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
    if let Some(x) = f("X") { it.x = x; }
    if let Some(y) = f("Y") { it.y = y; }
    if let Some(k) = f("Scale") { it.scale = k.clamp(0.2, 6.0); }
    if let Some(c) = s("Color") { it.color = c; }
    if let Some(c) = s("Font") { if !c.is_empty() { it.font = c; } }
    if let Some(x) = b("Bold") { it.bold = x; }
    if let Some(x) = b("Label") { it.label = x; }
    if let Some(x) = b("Seconds") { it.seconds = x; }
    if let Some(t) = s("Text") { it.text = t; }
    Some(it)
}

/// Раскладка из settings.json; None — её нет или это не массив (тогда берётся тема).
/// Пустой массив — пустая сцена: в редакторе удалили последний объект, и помпа должна показать то же,
/// а не вернуть объекты темы («Сбросить к теме» записывает None).
pub fn items_from_json(v: &serde_json::Value) -> Option<Vec<LcdItem>> {
    Some(v.as_array()?.iter().filter_map(item_from_json).collect())
}

pub fn items_to_json(items: &[LcdItem]) -> serde_json::Value {
    serde_json::Value::Array(items.iter().map(|i| serde_json::json!({
        "Kind": i.kind.index(), "X": (i.x * 10.0).round() / 10.0, "Y": (i.y * 10.0).round() / 10.0,
        "Scale": (i.scale * 100.0).round() / 100.0, "Color": i.color, "Font": i.font,
        "Bold": i.bold, "Label": i.label, "Seconds": i.seconds, "Text": i.text,
    })).collect())
}

/// Размер сцены для экрана panel: при повороте 90°/270° рисуем «боком» (SceneSize в C#).
pub fn scene_size(panel: (u32, u32), rotation: i32) -> (u32, u32) {
    if matches!(rotation, 90 | 270) { (panel.1, panel.0) } else { panel }
}

/// Своя раскладка из настроек (CoolerLayout). Она хранится в координатах сцены — то есть экрана помпы,
/// как её пишет C#-версия (settings.json общий), поэтому берётся как есть, без подгонки под размер:
/// и редактор, и служба работают в настоящем размере сцены. Под размер подгоняются только темы.
pub fn saved_layout(v: Option<&serde_json::Value>) -> Option<Vec<LcdItem>> {
    items_from_json(v?)
}

fn measure(font: &FontArc, size: f64, text: &str) -> f64 {
    let sc = font.as_scaled(PxScale::from(size as f32));
    let mut w = 0.0f32;
    let mut prev = None;
    for c in text.chars() {
        let id = sc.glyph_id(c);
        if let Some(p) = prev { w += sc.kern(p, id); }
        w += sc.h_advance(id);
        prev = Some(id);
    }
    w as f64
}

/// Прямоугольник объекта на сцене (x, y, ширина, высота) — для выделения и перетаскивания.
pub fn bounds(item: &LcdItem, s: &Snapshot, now: &NaiveDateTime, fonts: &mut Fonts) -> (f64, f64, f64, f64) {
    if item.is_bar() {
        let scale = item.scale.max(0.3);
        let height = 14.0 * scale;
        let font_size = (height * 0.95).max(6.0);
        let light = fonts.get(&item.font, false);
        let value_font = fonts.get(&item.font, item.bold);
        let mut w = 140.0 * scale + 6.0 * scale + measure(&value_font, font_size, "100%");
        if item.label { w += measure(&light, font_size, item.label_text()) + 6.0 * scale; }
        let h = height.max(line_height(&light, font_size));
        return (item.x, item.y, w, h);
    }
    let size = (item.base_size() * item.scale).max(6.0);
    let main = fonts.get(&item.font, item.bold);
    let mut w = measure(&main, size, &value_text(item, s, now));
    if item.has_label() && item.label {
        let light = fonts.get(&item.font, false);
        w += measure(&light, (size * 0.5).max(6.0), &format!("{} ", item.label_text()));
    }
    (item.x, item.y, w.max(8.0), line_height(&main, size))
}

/// Верхний объект под точкой (последний нарисованный — сверху).
pub fn hit(items: &[LcdItem], x: f64, y: f64, s: &Snapshot, now: &NaiveDateTime, fonts: &mut Fonts) -> Option<usize> {
    (0..items.len()).rev().find(|&i| {
        let (bx, by, bw, bh) = bounds(&items[i], s, now, fonts);
        x >= bx - 3.0 && x <= bx + bw + 3.0 && y >= by - 3.0 && y <= by + bh + 3.0
    })
}

/// Правдоподобные цифры для предпросмотра в редакторе (без датчиков).
pub fn sample_snapshot() -> Snapshot {
    Snapshot {
        cpu_load: 23.0, cpu_temp: Some(48.0), cpu_clock_mhz: Some(4650.0), cpu_power_w: Some(65.0),
        gpu_load: Some(37.0), gpu_temp: Some(56.0), gpu_power_w: Some(120.0),
        vram_used_gb: Some(4.2), vram_total_gb: Some(12.0),
        ram_load: 41.0, ram_used_gb: 13.1, ram_total_gb: 32.0,
        net_down_bps: 2_400_000.0, net_up_bps: 310_000.0,
        disk_used_pct: 62.0, disk_free_gb: 180.0, uptime_secs: 3 * 3600 + 25 * 60,
        ..Default::default()
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    #[test]
    fn kind_order_matches_csharp() {
        assert_eq!(LcdKind::Time.index(), 0);
        assert_eq!(LcdKind::Text.index(), 7);
        assert_eq!(LcdKind::RamBar.index(), 19);
    }

    #[test]
    fn json_round_trip() {
        let mut items = LcdItem::default_layout();
        items[0].seconds = true;
        items.push(LcdItem { text: "Привет".into(), x: 5.5, ..LcdItem::create(LcdKind::Text) });
        let v = items_to_json(&items);
        assert_eq!(v[0]["Kind"], 0);
        let back = items_from_json(&v).unwrap();
        assert_eq!(back.len(), items.len());
        assert!(back[0].seconds);
        assert_eq!(back[7].kind, LcdKind::Text);
        assert_eq!(back[7].text, "Привет");
        assert_eq!(back[7].x, 5.5);
    }

    #[test]
    fn reads_csharp_json() {
        let v: serde_json::Value = serde_json::from_str(
            r##"[{"Kind":3,"X":170,"Y":82,"Scale":1.5,"Color":"","Font":"Consolas","Bold":true,"Label":false,"Seconds":false,"Text":"Текст"}]"##,
        ).unwrap();
        let it = &items_from_json(&v).unwrap()[0];
        assert_eq!(it.kind, LcdKind::CpuTemp);
        assert_eq!(it.scale, 1.5);
        assert!(!it.label);
        assert_eq!(it.color, "");
    }

    #[test]
    fn saved_layout_stretches() {
        let v = items_to_json(&LcdItem::default_layout());
        // Своя раскладка — в координатах экрана (как пишет C#): берётся как есть при любом размере.
        let saved = saved_layout(Some(&v)).unwrap();
        assert_eq!(saved[1].x, 188.0);
        assert!(saved_layout(None).is_none());
        let csharp: serde_json::Value = serde_json::from_str(r#"[{"Kind":9,"X":376,"Y":40,"Scale":2,"Text":"A"}]"#).unwrap();
        let same = saved_layout(Some(&csharp)).unwrap();
        assert_eq!((same[0].x, same[0].y, same[0].scale), (376.0, 40.0, 2.0));
        assert_eq!(scene_size((640, 480), 90), (480, 640));
        assert_eq!(scene_size((640, 480), 180), (640, 480));
    }

    #[test]
    fn fit_scales_objects_like_theme_items() {
        // ThemeItems в C#: X·kx, Y·ky с округлением, Scale·min(kx, ky) до сотых.
        let items = vec![LcdItem { x: 188.0, y: 81.0, scale: 1.25, ..LcdItem::create(LcdKind::Text) }];
        let big = fit(&items, (320.0, 240.0), (640.0, 480.0));
        assert_eq!((big[0].x, big[0].y, big[0].scale), (376.0, 162.0, 2.5));
        let square = fit(&items, (320.0, 240.0), (320.0, 320.0));
        assert_eq!(square[0].scale, 1.25); // min(1, 1.33)
        // Тема под экран 640×480 — тоже крупнее.
        let theme = default_theme();
        let base = layout_for(&theme, 320, 240);
        let large = layout_for(&theme, 640, 480);
        assert_eq!(large[0].scale, (base[0].scale * 2.0 * 100.0).round_ties_even() / 100.0);
    }

    #[test]
    fn empty_layout_is_an_empty_scene() {
        // Удалили последний объект: это «пусто», а не «вернуть тему».
        let empty = serde_json::json!([]);
        assert_eq!(saved_layout(Some(&empty)).map(|v| v.len()), Some(0));
        assert!(items_from_json(&serde_json::json!("мусор")).is_none());
    }

    #[test]
    fn parse_color_rejects_garbage_without_panic() {
        assert_eq!(parse_color("#FF0050"), Some((0xFF, 0x00, 0x50)));
        assert_eq!(parse_color("80FF0050"), Some((0xFF, 0x00, 0x50)));
        assert_eq!(parse_color(" ff0050 "), Some((0xFF, 0x00, 0x50)));
        // 8 байт UTF-8, граница символа посреди среза [2..] — раньше паника.
        assert_eq!(parse_color("\u{1F3A8}FFFF"), None);
        assert_eq!(parse_color("жжжж"), None);
        assert_eq!(parse_color("+FFFFF"), None);
        assert_eq!(parse_color(""), None);
    }
}
