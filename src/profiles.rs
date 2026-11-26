//! RigProfile.cs + логика MainWindow.Profiles.cs (без окна): профили «одной кнопкой».
//!
//! Профили лежат в `Settings::profiles` как JSON-массив в формате C# (PascalCase), поэтому
//! поля, которые Rust-версия не трогает (MouseProfile и др.), не пропадают и файл остаётся
//! общим для обеих версий.

use serde_json::{Map, Value};

use crate::settings::Settings;

/// Поля клавиатуры (ApplyKeyboardPart).
pub const KEYBOARD_KEYS: &[&str] = &[
    "Effect", "Color", "Rainbow", "Brightness", "Speed", "Direction", "SyncLight", "SyncStyle", "KeyPaint", "PaintAnim",
    "PaintSpeed",
];
/// Поля подсветки СЖО (ApplyArgbPart).
pub const ARGB_KEYS: &[&str] =
    &["ArgbMode", "ArgbColor", "ArgbColor2", "ArgbColor3", "ArgbBrightness", "ArgbSpeed", "ArgbRainbow"];
/// Поля вентиляторов (ApplyFansPart).
pub const FAN_KEYS: &[&str] = &["FanModes", "FanPercent", "FanCurves", "FanModeAll"];
/// Поля экрана помпы (ApplyPumpPart).
pub const PUMP_KEYS: &[&str] = &[
    "CoolerLayout", "CoolerLayoutBase", "CoolerBackground", "CoolerBgTop", "CoolerBgBottom", "CoolerBrightness", "CoolerFps", "CoolerTheme",
];

/// Части профиля: флажок Use*, его поля и подпись для сообщения.
const PARTS: [(&str, &[&str], &str); 4] = [
    ("UseKeyboard", KEYBOARD_KEYS, "клавиатура"),
    ("UseArgb", ARGB_KEYS, "СЖО"),
    ("UseFans", FAN_KEYS, "вентиляторы"),
    ("UsePump", PUMP_KEYS, "экран помпы"),
];

/// RigProfile.Glyphs: символ Segoe MDL2 (хранится в файле, как в C#), замена-эмодзи для egui, имя.
pub const GLYPHS: &[(char, &str, &str)] = &[
    ('\u{E7FC}', "🎮", "Игры"),
    ('\u{E821}', "💼", "Работа"),
    ('\u{E708}', "🌙", "Тишина"),
    ('\u{E734}', "⭐", "Избранное"),
    ('\u{E945}', "⚡", "Мощность"),
    ('\u{E8D6}', "🎵", "Музыка"),
    ('\u{E714}', "🎬", "Видео"),
    ('\u{E706}', "💡", "Подсветка"),
];

/// FanModeNames из MainWindow.Fans.cs.
pub const FAN_MODE_NAMES: [&str; 7] =
    ["Авто (BIOS)", "Тихий", "Обычный", "Производительный", "Максимум", "Своя скорость", "Своя кривая"];

pub fn glyph_emoji(p: &Value) -> &'static str {
    let c = p.get("Glyph").and_then(Value::as_str).and_then(|g| g.chars().next());
    GLYPHS.iter().find(|g| Some(g.0) == c).map(|g| g.1).unwrap_or("●")
}

/// Копия списка профилей (пустой, если профилей нет).
pub fn list(s: &Settings) -> Vec<Value> {
    match &s.profiles {
        Some(Value::Array(a)) => a.clone(),
        _ => Vec::new(),
    }
}

pub fn list_mut(s: &mut Settings) -> &mut Vec<Value> {
    if !matches!(s.profiles, Some(Value::Array(_))) {
        s.profiles = Some(Value::Array(Vec::new()));
    }
    match s.profiles.as_mut() {
        Some(Value::Array(a)) => a,
        _ => unreachable!(),
    }
}

pub fn name(p: &Value) -> String {
    p.get("Name").and_then(Value::as_str).unwrap_or("Профиль").to_string()
}

/// Флажок Use*: по умолчанию, как в RigProfile.cs, включены клавиатура, ARGB и вентиляторы.
pub fn flag(p: &Value, key: &str) -> bool {
    p.get(key).and_then(Value::as_bool).unwrap_or(matches!(key, "UseKeyboard" | "UseArgb" | "UseFans"))
}

pub fn set(p: &mut Value, key: &str, v: Value) {
    if let Value::Object(m) = p {
        m.insert(key.to_string(), v);
    }
}

fn settings_obj(s: &Settings) -> Map<String, Value> {
    match serde_json::to_value(s) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

/// CaptureInto: записать в профиль текущие настройки (все части, флажки не меняются).
pub fn capture_into(p: &mut Value, s: &Settings) {
    let src = settings_obj(s);
    if !p.is_object() {
        *p = Value::Object(Map::new());
    }
    for (_, keys, _) in PARTS {
        for k in keys {
            set(p, k, src.get(*k).cloned().unwrap_or(Value::Null));
        }
    }
}

/// NewProfile: профиль из текущих настроек.
pub fn new_profile(name: &str, glyph: char, s: &Settings) -> Value {
    let mut m = Map::new();
    m.insert("Name".into(), Value::from(name));
    m.insert("Glyph".into(), Value::from(glyph.to_string()));
    for (k, v) in [("UseKeyboard", true), ("UseArgb", true), ("UseFans", true), ("UsePump", false), ("UseMouse", false)] {
        m.insert(k.into(), Value::from(v));
    }
    let mut p = Value::Object(m);
    capture_into(&mut p, s);
    p
}

/// InitProfiles: если профилей нет — «Игра», «Работа», «Тихо» из текущих настроек.
/// Возвращает true, если профили созданы (настройки пора сохранить).
pub fn init_defaults(s: &mut Settings) -> bool {
    if !list(s).is_empty() {
        return false;
    }
    let mut game = new_profile("Игра", '\u{E7FC}', s);
    set(&mut game, "FanModes", Value::Null);
    set(&mut game, "FanModeAll", Value::from(3));
    set(&mut game, "Brightness", Value::from(5));
    set(&mut game, "ArgbBrightness", Value::from(100));

    let mut work = new_profile("Работа", '\u{E821}', s);
    set(&mut work, "FanModes", Value::Null);
    set(&mut work, "FanModeAll", Value::from(0));

    let mut quiet = new_profile("Тихо", '\u{E708}', s);
    set(&mut quiet, "FanModes", Value::Null);
    set(&mut quiet, "FanModeAll", Value::from(1));
    set(&mut quiet, "Brightness", Value::from(1));
    set(&mut quiet, "ArgbBrightness", Value::from(15));

    list_mut(s).extend([game, work, quiet]);
    true
}

/// OnProfileNew: новый профиль «Профиль N» из текущих настроек, он же становится активным.
pub fn new_from_current(s: &mut Settings) -> usize {
    let names: Vec<String> = list(s).iter().map(name).collect();
    let mut n = names.len() + 1;
    let mut nm = format!("Профиль {n}");
    while names.contains(&nm) {
        n += 1;
        nm = format!("Профиль {n}");
    }
    let p = new_profile(&nm, '\u{E734}', s);
    let l = list_mut(s);
    l.push(p);
    let idx = l.len() - 1;
    s.active_profile = Some(nm);
    idx
}

/// ApplyProfileAsync без мыши: переносит в настройки отмеченные части профиля.
/// Возвращает подписи применённых частей.
pub fn apply(p: &Value, s: &mut Settings) -> Result<Vec<&'static str>, String> {
    let mut obj = settings_obj(s);
    let mut parts = Vec::new();
    for (use_key, keys, label) in PARTS {
        if !flag(p, use_key) {
            continue;
        }
        for k in keys {
            let Some(v) = p.get(*k) else { continue };
            // null у вентиляторов значит «своих режимов нет» (как в C#), в остальном — «не задано».
            if v.is_null() && (use_key != "UseFans" || *k == "FanModeAll") {
                continue;
            }
            obj.insert((*k).to_string(), v.clone());
        }
        // Раскладка экрана помпы и отметка её координат ходят вместе: профиль из C#-версии отметки
        // не имеет — значит, его раскладка в координатах экрана, и старую отметку надо снять.
        if use_key == "UsePump" && p.get("CoolerLayout").is_some_and(|v| !v.is_null()) {
            obj.insert("CoolerLayoutBase".to_string(), p.get("CoolerLayoutBase").cloned().unwrap_or(Value::Null));
        }
        parts.push(label);
    }
    let mut next: Settings =
        serde_json::from_value(Value::Object(obj)).map_err(|e| format!("Профиль «{}»: {e}", name(p)))?;
    next.migrate();
    next.active_profile = Some(name(p));
    *s = next;
    Ok(parts)
}

/// MouseProfile (0–2), если профиль переключает мышь (UseMouse и номер задан).
pub fn mouse_profile(p: &Value) -> Option<u8> {
    if !flag(p, "UseMouse") {
        return None;
    }
    p.get("MouseProfile").and_then(Value::as_i64).map(|v| v.clamp(0, 2) as u8)
}

/// CaptureInto, часть мыши: запомнить текущий профиль мыши (если мышь прочитана).
pub fn set_mouse_profile(p: &mut Value, mp: Option<u8>) {
    if let Some(m) = mp {
        set(p, "MouseProfile", Value::from(m.min(2)));
    }
}

pub fn rename(s: &mut Settings, idx: usize, new_name: &str) -> Result<(), String> {
    let new_name = new_name.trim();
    if new_name.is_empty() {
        return Err("Имя профиля не может быть пустым.".into());
    }
    let l = list(s);
    let Some(old) = l.get(idx).map(name) else { return Err("Нет такого профиля.".into()) };
    if old == new_name {
        return Ok(());
    }
    if l.iter().enumerate().any(|(i, p)| i != idx && name(p) == new_name) {
        return Err(format!("Профиль «{new_name}» уже есть."));
    }
    set(&mut list_mut(s)[idx], "Name", Value::from(new_name));
    if s.active_profile.as_deref() == Some(old.as_str()) {
        s.active_profile = Some(new_name.to_string());
    }
    Ok(())
}

/// Удаление; последний профиль удалить нельзя (как ProfileDeleteButton.IsEnabled в C#).
pub fn delete(s: &mut Settings, idx: usize) -> Result<String, String> {
    let l = list_mut(s);
    if l.len() <= 1 {
        return Err("Последний профиль удалить нельзя.".into());
    }
    if idx >= l.len() {
        return Err("Нет такого профиля.".into());
    }
    let removed = name(&l.remove(idx));
    if s.active_profile.as_deref() == Some(removed.as_str()) {
        s.active_profile = None;
    }
    Ok(removed)
}

/// FanSummary: один режим на всех или «свои режимы».
pub fn fan_summary(p: &Value) -> String {
    let all = p.get("FanModeAll").and_then(Value::as_i64).unwrap_or(0);
    let mut values: Vec<i64> = p
        .get("FanModes")
        .and_then(Value::as_object)
        .map(|m| m.values().filter_map(Value::as_i64).collect())
        .unwrap_or_default();
    if values.is_empty() {
        values.push(all);
    }
    values.sort_unstable();
    values.dedup();
    if values.len() == 1 {
        FAN_MODE_NAMES[values[0].clamp(0, 6) as usize].to_string()
    } else {
        "свои режимы".into()
    }
}

/// ProfileSummary: краткое (для подсказки) или полное описание профиля.
pub fn summary(p: &Value, argb_modes: &[(i32, &str)], full: bool) -> String {
    let int = |k: &str, d: i64| p.get(k).and_then(Value::as_i64).unwrap_or(d);
    let mut parts: Vec<String> = Vec::new();
    if flag(p, "UseKeyboard") {
        let b = int("Brightness", 5);
        parts.push(if full {
            format!("Клавиатура: эффект {}, яркость {b}", int("Effect", 1))
        } else {
            format!("клавиатура {b}/5")
        });
    }
    if flag(p, "UseArgb") {
        let m = int("ArgbMode", 0) as i32;
        let mode = argb_modes.iter().find(|x| x.0 == m).map(|x| x.1.to_lowercase()).unwrap_or_default();
        let argb = if m == 0 {
            "не трогать".to_string()
        } else if full {
            format!("{mode}, {} %", int("ArgbBrightness", 100))
        } else {
            mode
        };
        parts.push(format!("{}{argb}", if full { "Подсветка СЖО: " } else { "СЖО: " }));
    }
    if flag(p, "UseFans") {
        parts.push(format!("{}{}", if full { "Вентиляторы: " } else { "вент.: " }, fan_summary(p).to_lowercase()));
    }
    if flag(p, "UsePump") {
        parts.push(if full {
            match p.get("CoolerTheme").and_then(Value::as_str) {
                Some(t) => format!("Экран помпы: тема «{t}»"),
                None => "Экран помпы: своя расстановка".into(),
            }
        } else {
            "помпа".into()
        });
    }
    if flag(p, "UseMouse") {
        if let Some(mp) = p.get("MouseProfile").and_then(Value::as_i64) {
            parts.push(if full { format!("Мышь: профиль {}", mp + 1) } else { format!("мышь {}", mp + 1) });
        }
    }
    if parts.is_empty() {
        return "Ничего не переключает".into();
    }
    if full {
        parts.join(" · ") + "."
    } else {
        parts.join(" · ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn defaults_once() {
        let mut s = Settings::default();
        assert!(init_defaults(&mut s));
        assert_eq!(list(&s).len(), 3);
        assert!(!init_defaults(&mut s));
        assert_eq!(list(&s)[2]["ArgbBrightness"], 15);
    }

    #[test]
    fn apply_respects_flags() {
        let mut s = Settings::default();
        s.brightness = 2;
        s.argb_brightness = 40;
        let mut p = new_profile("A", '\u{E7FC}', &s);
        set(&mut p, "Brightness", Value::from(5));
        set(&mut p, "ArgbBrightness", Value::from(90));
        set(&mut p, "UseArgb", Value::from(false));
        let parts = apply(&p, &mut s).unwrap();
        assert_eq!(s.brightness, 5);
        assert_eq!(s.argb_brightness, 40);
        assert_eq!(s.active_profile.as_deref(), Some("A"));
        assert!(parts.contains(&"клавиатура") && !parts.contains(&"СЖО"));
    }

    #[test]
    fn fans_null_clears_modes() {
        let mut s = Settings::default();
        s.fan_modes = Some(BTreeMap::from([("x".to_string(), 2)]));
        let mut p = new_profile("Q", '\u{E708}', &s);
        set(&mut p, "FanModes", Value::Null);
        set(&mut p, "FanModeAll", Value::from(1));
        apply(&p, &mut s).unwrap();
        assert!(s.fan_modes.is_none());
        assert_eq!(s.fan_mode_all, 1);
    }

    #[test]
    fn rename_delete_and_unknown_fields() {
        let mut s = Settings::default();
        init_defaults(&mut s);
        set(&mut list_mut(&mut s)[0], "MouseProfile", Value::from(1));
        let p = list(&s)[0].clone();
        apply(&p, &mut s).unwrap();
        assert_eq!(list(&s)[0]["MouseProfile"], 1);
        rename(&mut s, 0, "Игры").unwrap();
        assert_eq!(s.active_profile.as_deref(), Some("Игры"));
        assert!(rename(&mut s, 1, "Игры").is_err());
        delete(&mut s, 0).unwrap();
        assert!(s.active_profile.is_none());
        delete(&mut s, 0).unwrap();
        assert!(delete(&mut s, 0).is_err());
    }
}
