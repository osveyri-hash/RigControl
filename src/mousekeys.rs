//! Порт Mouse/MouseKeys.cs: кнопки мыши MCHOSE, каталог функций и коды клавиш (HID usage)
//! для назначений. WPF-типы Key/ModifierKeys заменены разбором строк вида "ctrl+shift+f5".

use crate::mouse::MouseKey;

pub const LEFT: usize = 0;
pub const MIDDLE: usize = 1;
pub const RIGHT: usize = 2;
pub const FORWARD: usize = 3;
pub const BACK: usize = 4;
pub const COUNT: usize = 5;

pub const SHORT: [&str; COUNT] = ["ЛКМ", "СКМ", "ПКМ", "Вперёд", "Назад"];
pub const TITLES: [&str; COUNT] = ["Левая кнопка", "Колесо (нажатие)", "Правая кнопка", "Боковая передняя", "Боковая задняя"];
/// Имена кнопок для командной строки.
pub const BUTTON_IDS: [&str; COUNT] = ["left", "middle", "right", "forward", "back"];

const fn k(kind: u8, value: u32) -> MouseKey {
    MouseKey { kind, value }
}

pub const DEFAULT: MouseKey = k(0, 0);
const LEFT_CLICK: MouseKey = k(1, 0x010000);

/// Родные функции кнопок (тип 0 в памяти мыши означает именно их).
pub const DEFAULTS: [MouseKey; COUNT] = [LEFT_CLICK, k(1, 0x040000), k(1, 0x020000), k(1, 0x100000), k(1, 0x080000)];

/// Функция для кнопки (как список во вкладке «Настройка клавиш» M HUB). `id` — имя для CLI.
#[derive(Debug, Clone, Copy)]
pub struct MouseAction {
    pub group: &'static str,
    pub name: &'static str,
    pub id: &'static str,
    pub key: MouseKey,
    pub hint: Option<&'static str>,
}

const fn a(group: &'static str, name: &'static str, id: &'static str, key: MouseKey, hint: Option<&'static str>) -> MouseAction {
    MouseAction { group, name, id, key, hint }
}

pub const ACTIONS: &[MouseAction] = &[
    a("Мышь", "Левый клик", "left-click", LEFT_CLICK, None),
    a("Мышь", "Правый клик", "right-click", k(1, 0x020000), None),
    a("Мышь", "Средний клик", "middle-click", k(1, 0x040000), None),
    a("Мышь", "Вперёд", "forward", k(1, 0x100000), None),
    a("Мышь", "Назад", "back", k(1, 0x080000), None),
    a("Мышь", "Прокрутка вверх", "scroll-up", k(1, 0x000200), None),
    a("Мышь", "Прокрутка вниз", "scroll-down", k(1, 0x00FE00), None),
    a("DPI", "DPI по кругу", "dpi-cycle", k(5, 0x010000), None),
    a("DPI", "DPI +", "dpi-up", k(5, 0x020000), None),
    a("DPI", "DPI −", "dpi-down", k(5, 0x030000), None),
    a("Встроенные профили", "Профиль 1", "profile-1", k(10, 0x010000), None),
    a("Встроенные профили", "Профиль 2", "profile-2", k(10, 0x020000), None),
    a("Встроенные профили", "Профиль 3", "profile-3", k(10, 0x030000), None),
    a("Встроенные профили", "Следующий профиль", "profile-next", k(10, 0x040000), None),
    a("Мультимедиа", "Пауза / воспроизведение", "play-pause", k(3, 0xCD0000), None),
    a("Мультимедиа", "Следующий трек", "next-track", k(3, 0xB50000), None),
    a("Мультимедиа", "Предыдущий трек", "prev-track", k(3, 0xB60000), None),
    a("Мультимедиа", "Стоп", "stop", k(3, 0xB70000), None),
    a("Мультимедиа", "Громче", "volume-up", k(3, 0xE90000), None),
    a("Мультимедиа", "Тише", "volume-down", k(3, 0xEA0000), None),
    a("Мультимедиа", "Без звука", "mute", k(3, 0xE20000), None),
    a("Правка", "Копировать", "copy", k(8, 0x070106), Some("Ctrl+C")),
    a("Правка", "Вставить", "paste", k(8, 0x070119), Some("Ctrl+V")),
    a("Правка", "Вырезать", "cut", k(8, 0x07011B), Some("Ctrl+X")),
    a("Правка", "Отменить", "undo", k(2, 0x011D00), Some("Ctrl+Z")),
    a("Правка", "Повторить", "redo", k(2, 0x011C00), Some("Ctrl+Y")),
    a("Правка", "Выделить всё", "select-all", k(2, 0x010400), Some("Ctrl+A")),
    a("Windows", "Рабочий стол", "desktop", k(2, 0x080700), Some("Win+D")),
    a("Windows", "Проводник", "explorer", k(2, 0x080800), Some("Win+E")),
    a("Windows", "Представление задач", "task-view", k(2, 0x082B00), Some("Win+Tab")),
    a("Windows", "Заблокировать ПК", "lock", k(2, 0x080F00), Some("Win+L")),
    a("Windows", "Яркость +", "brightness-up", k(8, 0x0C6F00), None),
    a("Windows", "Яркость −", "brightness-down", k(8, 0x0C7000), None),
    a("Другое", "Отключить кнопку", "disable", k(9, 0xFFFFFF), None),
];

/// Что сейчас делает кнопка (Describe).
pub fn describe(button: usize, key: MouseKey) -> String {
    let key = if key.kind == 0 { DEFAULTS[button.min(COUNT - 1)] } else { key };
    if let Some(action) = ACTIONS.iter().find(|x| x.key == key) {
        return action.name.to_string();
    }
    match key.kind {
        2 => shortcut(key.value),
        4 => "Макрос".to_string(),
        9 => "Отключена".to_string(),
        t => format!("Своя функция ({t}:{:06X})", key.value),
    }
}

/// Назначение равно родному — в мышь пишем тип 0, как M HUB (Normalize).
pub fn normalize(button: usize, key: MouseKey) -> MouseKey {
    if button < COUNT && key == DEFAULTS[button] { DEFAULT } else { key }
}

pub fn is_left_click(button: usize, key: MouseKey) -> bool {
    key == LEFT_CLICK || (button == LEFT && key.kind == 0)
}

/// Клавиша: тип 2, значение [модификаторы, HID usage, 0].
pub fn keyboard_key(modifiers: u8, usage: u8) -> MouseKey {
    k(2, (modifiers as u32) << 16 | (usage as u32) << 8)
}

pub fn shortcut(value: u32) -> String {
    let mods = (value >> 16 & 0xFF) as u8;
    let usage = (value >> 8 & 0xFF) as u8;
    let mut parts: Vec<String> = Vec::new();
    if mods & 0x11 != 0 { parts.push("Ctrl".into()); }
    if mods & 0x22 != 0 { parts.push("Shift".into()); }
    if mods & 0x44 != 0 { parts.push("Alt".into()); }
    if mods & 0x88 != 0 { parts.push("Win".into()); }
    if usage != 0 { parts.push(usage_name(usage)); }
    if parts.is_empty() { "Клавиша".into() } else { parts.join("+") }
}

pub fn usage_name(usage: u8) -> String {
    match usage {
        0x04..=0x1D => ((b'A' + usage - 0x04) as char).to_string(),
        0x1E..=0x26 => ((b'1' + usage - 0x1E) as char).to_string(),
        0x3A..=0x45 => format!("F{}", usage - 0x39),
        0x68..=0x73 => format!("F{}", usage - 0x68 + 13),
        0x59..=0x61 => format!("Num {}", usage - 0x58),
        _ => match usage {
            0x27 => "0", 0x28 => "Enter", 0x29 => "Esc", 0x2A => "Backspace", 0x2B => "Tab", 0x2C => "Пробел",
            0x2D => "-", 0x2E => "=", 0x2F => "[", 0x30 => "]", 0x31 => "\\", 0x33 => ";", 0x34 => "'", 0x35 => "`",
            0x36 => ",", 0x37 => ".", 0x38 => "/", 0x39 => "Caps Lock", 0x46 => "Print Screen", 0x47 => "Scroll Lock",
            0x48 => "Pause", 0x49 => "Insert", 0x4A => "Home", 0x4B => "Page Up", 0x4C => "Delete", 0x4D => "End",
            0x4E => "Page Down", 0x4F => "→", 0x50 => "←", 0x51 => "↓", 0x52 => "↑", 0x53 => "Num Lock",
            0x54 => "Num /", 0x55 => "Num *", 0x56 => "Num −", 0x57 => "Num +", 0x58 => "Num Enter", 0x62 => "Num 0",
            0x63 => "Num .", 0x64 => "\\", 0x65 => "Меню",
            _ => return format!("0x{usage:02X}"),
        }
        .to_string(),
    }
}

/// Имя клавиши → HID usage (замена Usage(Key) из WPF). Регистр не важен.
pub fn usage_from_name(name: &str) -> Option<u8> {
    let n = name.trim().to_ascii_lowercase();
    let b = n.as_bytes();
    if b.len() == 1 {
        let c = b[0];
        if c.is_ascii_lowercase() { return Some(0x04 + c - b'a'); }
        if (b'1'..=b'9').contains(&c) { return Some(0x1E + c - b'1'); }
    }
    if let Some(num) = n.strip_prefix('f').and_then(|r| r.parse::<u8>().ok()) {
        return match num { 1..=12 => Some(0x3A + num - 1), 13..=24 => Some(0x68 + num - 13), _ => None };
    }
    if let Some(num) = n.strip_prefix("num").and_then(|r| r.parse::<u8>().ok()) {
        return match num { 0 => Some(0x62), 1..=9 => Some(0x59 + num - 1), _ => None };
    }
    Some(match n.as_str() {
        "0" => 0x27, "enter" => 0x28, "esc" | "escape" => 0x29, "backspace" => 0x2A, "tab" => 0x2B,
        "space" | "пробел" => 0x2C, "-" | "minus" => 0x2D, "=" | "plus" => 0x2E, "[" => 0x2F, "]" => 0x30,
        "\\" => 0x31, ";" => 0x33, "'" => 0x34, "`" | "tilde" => 0x35, "," => 0x36, "." => 0x37, "/" => 0x38,
        "capslock" => 0x39, "printscreen" | "prtsc" => 0x46, "scrolllock" => 0x47, "pause" => 0x48,
        "insert" | "ins" => 0x49, "home" => 0x4A, "pageup" | "pgup" => 0x4B, "delete" | "del" => 0x4C, "end" => 0x4D,
        "pagedown" | "pgdn" => 0x4E, "right" => 0x4F, "left" => 0x50, "down" => 0x51, "up" => 0x52,
        "numlock" => 0x53, "num/" => 0x54, "num*" => 0x55, "num-" => 0x56, "num+" => 0x57, "numenter" => 0x58,
        "num." => 0x63, "menu" | "apps" => 0x65,
        _ => return None,
    })
}

/// Сочетание вида "ctrl+shift+f5" → назначение типа 2. Биты модификаторов — левые (как Modifiers()).
pub fn parse_shortcut(text: &str) -> Result<MouseKey, String> {
    let mut mods = 0u8;
    let mut usage = 0u8;
    for part in text.split('+').filter(|p| !p.is_empty()) {
        match part.trim().to_ascii_lowercase().as_str() {
            "ctrl" | "control" => mods |= 1,
            "shift" => mods |= 2,
            "alt" => mods |= 4,
            "win" | "windows" => mods |= 8,
            other => {
                if usage != 0 { return Err(format!("В сочетании «{text}» больше одной обычной клавиши")); }
                usage = usage_from_name(other).ok_or_else(|| format!("Неизвестная клавиша «{other}»"))?;
            }
        }
    }
    if mods == 0 && usage == 0 { return Err("Пустое сочетание клавиш".into()); }
    Ok(keyboard_key(mods, usage))
}

pub fn button_from_name(name: &str) -> Option<usize> {
    let n = name.to_ascii_lowercase();
    if let Ok(i) = n.parse::<usize>() { return (1..=COUNT).contains(&i).then(|| i - 1); }
    BUTTON_IDS.iter().position(|b| *b == n)
}

/// Функция для CLI: id из ACTIONS, "default" или "key:ctrl+c".
pub fn parse_action(text: &str) -> Result<MouseKey, String> {
    let t = text.trim();
    if t.eq_ignore_ascii_case("default") { return Ok(DEFAULT); }
    if let Some(rest) = t.strip_prefix("key:") { return parse_shortcut(rest); }
    ACTIONS.iter().find(|x| x.id.eq_ignore_ascii_case(t)).map(|x| x.key)
        .ok_or_else(|| format!("Неизвестная функция «{t}». Список: rigcontrol mouse actions"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describe_defaults_and_shortcuts() {
        assert_eq!(describe(LEFT, DEFAULT), "Левый клик");
        assert_eq!(describe(BACK, DEFAULT), "Назад");
        assert_eq!(describe(RIGHT, k(2, 0x011D00)), "Отменить");
        assert_eq!(describe(RIGHT, keyboard_key(0x03, 0x3E)), "Ctrl+Shift+F5");
        assert_eq!(describe(RIGHT, k(9, 0xFFFFFF)), "Отключить кнопку");
        assert_eq!(describe(RIGHT, k(4, 1)), "Макрос");
        assert_eq!(describe(RIGHT, k(7, 0xABCDEF)), "Своя функция (7:ABCDEF)");
    }

    #[test]
    fn normalize_to_default() {
        assert_eq!(normalize(MIDDLE, k(1, 0x040000)), DEFAULT);
        assert_eq!(normalize(MIDDLE, k(1, 0x020000)), k(1, 0x020000));
        assert!(is_left_click(LEFT, DEFAULT));
        assert!(!is_left_click(RIGHT, DEFAULT));
    }

    #[test]
    fn shortcut_parse_roundtrip() {
        assert_eq!(parse_shortcut("ctrl+z").unwrap(), k(2, 0x011D00));
        assert_eq!(parse_shortcut("Win+Tab").unwrap(), k(2, 0x082B00));
        assert_eq!(parse_shortcut("f13").unwrap(), k(2, 0x006800));
        assert!(parse_shortcut("a+b").is_err());
        assert_eq!(parse_action("key:alt+f4").unwrap(), keyboard_key(4, 0x3D));
        assert_eq!(parse_action("dpi-up").unwrap(), k(5, 0x020000));
        assert_eq!(button_from_name("back"), Some(BACK));
        assert_eq!(button_from_name("4"), Some(FORWARD));
    }

    #[test]
    fn usage_names() {
        for u in 0x04u8..=0x65 {
            let name = usage_name(u);
            if name.starts_with("0x") { continue; }
            let _ = usage_from_name(&name); // часть имён (стрелки, «Num −») только для вывода
        }
        assert_eq!(usage_name(0x3A), "F1");
        assert_eq!(usage_name(0x73), "F24");
        assert_eq!(usage_name(0x61), "Num 9");
    }
}
