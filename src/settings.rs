//! Порт Settings.cs: настройки между запусками в %APPDATA%\RigControl\settings.json.
//!
//! Формат файла совместим с C#-версией: имена полей в PascalCase, как у System.Text.Json.
//! Поля, которых Rust-версия пока не знает (например, CoolerLayout в формате C#), не теряются:
//! они попадают в `extra` и записываются обратно как были.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

/// Значения ArgbMode из C# (Argb/ArgbEngine.cs).
pub const ARGB_RAINBOW_WAVE: i32 = 5;
/// Устаревший пункт «Плавная волна» — при загрузке переводится в RainbowWave.
pub const ARGB_COLOR_WAVE: i32 = 9;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase", default)]
pub struct Settings {
    pub last_image: Option<String>,
    pub crop: bool,
    pub max_frames: i32,

    pub effect: i32,
    pub color: String,
    pub rainbow: bool,
    pub brightness: i32,
    pub speed: i32,
    pub direction: i32,
    pub sync_light: bool,
    pub sync_style: i32,

    // Экран помпы
    pub cooler_mode: i32,
    pub cooler_image: Option<String>,
    pub cooler_rotation: i32,
    pub cooler_size: i32,
    /// Раскладка сцены хранится как есть (JSON из C#), чтобы файл оставался общим для обеих версий.
    pub cooler_layout: Option<Value>,
    pub cooler_background: Option<String>,
    pub cooler_bg_top: Option<String>,
    pub cooler_bg_bottom: Option<String>,
    pub cooler_brightness: i32,
    pub cooler_fps: i32,
    pub cooler_theme: Option<String>,

    // Своя раскраска клавиш
    pub key_paint: Option<BTreeMap<String, String>>,
    pub paint_color: Option<String>,
    pub paint_anim: i32,
    pub paint_speed: i32,
    pub paint_mirror: bool,

    // ARGB (ASUS AURA)
    pub argb_mode: i32,
    pub argb_color: String,
    pub argb_color2: String,
    pub argb_color3: String,
    pub argb_brightness: i32,
    pub argb_speed: i32,
    pub argb_leds: i32,
    pub argb_fans: i32,
    pub argb_target: i32,
    pub argb_board_effect: bool,
    pub argb_rainbow: bool,
    pub armoury_stopped: bool,
    pub effect_rates: Option<BTreeMap<String, f64>>,

    // Вентиляторы
    pub fan_modes: Option<BTreeMap<String, i32>>,
    pub fan_percent: Option<BTreeMap<String, i32>>,
    pub fan_curves: Option<BTreeMap<String, Vec<i32>>>,
    pub fan_mode_all: i32,

    // Профили
    /// Профили C#-версии: в Rust-версии их нет, но из файла они не пропадают.
    pub profiles: Option<Value>,
    pub active_profile: Option<String>,

    // Поведение программы
    pub close_to_tray: bool,
    pub auto_output: bool,

    /// Неизвестные поля — сохраняются без изменений.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            last_image: None,
            crop: true,
            max_frames: 40,
            effect: 1,
            color: "#FFFFFF".into(),
            rainbow: false,
            brightness: 5,
            speed: 3,
            direction: 0,
            sync_light: false,
            sync_style: 0,
            cooler_mode: 0,
            cooler_image: None,
            cooler_rotation: 0,
            cooler_size: 0,
            cooler_layout: None,
            cooler_background: None,
            cooler_bg_top: None,
            cooler_bg_bottom: None,
            cooler_brightness: 100,
            cooler_fps: 2,
            cooler_theme: None,
            key_paint: None,
            paint_color: None,
            paint_anim: 0,
            paint_speed: 2,
            paint_mirror: true,
            argb_mode: 0,
            argb_color: "#00B4FF".into(),
            argb_color2: "#FF00A8".into(),
            argb_color3: "#8A2BFF".into(),
            argb_brightness: 100,
            argb_speed: 3,
            argb_leds: 30,
            argb_fans: 3,
            argb_target: 0,
            argb_board_effect: false,
            argb_rainbow: true,
            armoury_stopped: false,
            effect_rates: None,
            fan_modes: None,
            fan_percent: None,
            fan_curves: None,
            fan_mode_all: 0,
            profiles: None,
            active_profile: None,
            close_to_tray: true,
            auto_output: true,
            extra: Map::new(),
        }
    }
}

/// Папка программы: %APPDATA%\RigControl (на других ОС — ~/.config/RigControl).
pub fn app_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("RigControl")
}

pub fn file_path() -> PathBuf {
    app_dir().join("settings.json")
}

/// Effect в settings.json — номер в списке эффектов, как в C# (0–29). Rust-версия до 0.36.5 писала туда
/// режим прошивки: 100–109 для своих эффектов и 0x80 для своей раскраски. Такие значения переводим в номера.
pub fn effect_index_from_old(effect: i32) -> i32 {
    use crate::{effects, keypaint, protocol};
    if effect == keypaint::USER_LIGHT_MODE {
        return protocol::effect_index(effects::PAINT).unwrap_or(effect);
    }
    if effects::is_custom(effect) {
        if let Some(i) = protocol::effect_index(effect) {
            return i;
        }
    }
    effect
}

impl Settings {
    /// Разбор JSON + перенос старых значений (как в Settings.Load).
    pub fn from_json(text: &str) -> Result<Self, String> {
        // System.Text.Json пишет BOM не всегда, но File.WriteAllText может — срезаем.
        let text = text.trim_start_matches('\u{feff}');
        let mut s: Settings = serde_json::from_str(text).map_err(|e| e.to_string())?;
        s.migrate();
        Ok(s)
    }

    pub fn migrate(&mut self) {
        if self.argb_mode == ARGB_COLOR_WAVE {
            self.argb_mode = ARGB_RAINBOW_WAVE;
        }
        self.effect = effect_index_from_old(self.effect);
        if let Some(Value::Array(list)) = &mut self.profiles {
            for p in list.iter_mut() {
                if p.get("ArgbMode").and_then(|v| v.as_i64()) == Some(ARGB_COLOR_WAVE as i64) {
                    p["ArgbMode"] = Value::from(ARGB_RAINBOW_WAVE);
                }
                if let Some(e) = p.get("Effect").and_then(|v| v.as_i64()) {
                    let fixed = effect_index_from_old(e as i32);
                    if i64::from(fixed) != e {
                        p["Effect"] = Value::from(fixed);
                    }
                }
            }
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into())
    }

    /// Загрузка. Битый файл копируется в settings.json.bad, а программа стартует с умолчаний.
    pub fn load() -> Self {
        let path = file_path();
        let Ok(text) = fs::read_to_string(&path) else { return Self::default() };
        match Self::from_json(&text) {
            Ok(s) => s,
            Err(e) => {
                crate::logfile::append("errors.log", &format!("settings.json: {e}\n"));
                let _ = fs::copy(&path, path.with_extension("json.bad"));
                Self::default()
            }
        }
    }

    /// Сохранение через временный файл и подмену (не оставляет обрезанный JSON при сбое).
    pub fn save(&self) -> Result<(), String> {
        let path = file_path();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, self.to_json()).map_err(|e| e.to_string())?;
        fs::rename(&tmp, &path).map_err(|e| e.to_string())
    }

    /// Изменение одного поля по имени (`Effect`, `ArgbColor` …) — для `rigcontrol settings set`.
    /// Значение разбирается как JSON, а если не получилось — берётся строкой.
    pub fn set_field(&mut self, key: &str, raw: &str) -> Result<(), String> {
        let mut obj = match serde_json::to_value(&*self).map_err(|e| e.to_string())? {
            Value::Object(m) => m,
            _ => return Err("внутренняя ошибка".into()),
        };
        let real_key = obj
            .keys()
            .find(|k| k.eq_ignore_ascii_case(key))
            .cloned()
            .ok_or_else(|| format!("нет такой настройки: {key}"))?;
        let value = serde_json::from_str::<Value>(raw).unwrap_or_else(|_| Value::String(raw.into()));
        obj.insert(real_key, value);
        let mut next: Settings = serde_json::from_value(Value::Object(obj)).map_err(|e| format!("неверное значение: {e}"))?;
        next.migrate();
        *self = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_csharp() {
        let s = Settings::from_json("{}").unwrap();
        assert_eq!(s, Settings::default());
        assert!(s.crop && s.paint_mirror && s.argb_rainbow && s.close_to_tray && s.auto_output);
        assert_eq!((s.max_frames, s.effect, s.brightness, s.speed), (40, 1, 5, 3));
        assert_eq!(s.argb_leds, 30);
    }

    #[test]
    fn color_wave_migrates() {
        let s = Settings::from_json(r#"{"ArgbMode":9,"Profiles":[{"Name":"A","ArgbMode":9}]}"#).unwrap();
        assert_eq!(s.argb_mode, ARGB_RAINBOW_WAVE);
        assert_eq!(s.profiles.unwrap()[0]["ArgbMode"], 5);
    }

    #[test]
    fn old_rust_effect_values_migrate() {
        let s = Settings::from_json(r#"{"Effect":128,"Profiles":[{"Name":"A","Effect":106}]}"#).unwrap();
        assert_eq!(s.effect, 25);
        assert_eq!(s.profiles.unwrap()[0]["Effect"], 26);
        assert_eq!(Settings::from_json(r#"{"Effect":8}"#).unwrap().effect, 8);
        assert_eq!(Settings::from_json(r#"{"Effect":27}"#).unwrap().effect, 27);
    }

    #[test]
    fn unknown_fields_survive() {
        let s = Settings::from_json(r#"{"FutureThing":{"a":1},"CoolerLayout":[{"Kind":0}]}"#).unwrap();
        let back: Value = serde_json::from_str(&s.to_json()).unwrap();
        assert_eq!(back["FutureThing"]["a"], 1);
        assert_eq!(back["CoolerLayout"][0]["Kind"], 0);
    }

    #[test]
    fn set_field_case_insensitive() {
        let mut s = Settings::default();
        s.set_field("argbcolor", "#112233").unwrap();
        s.set_field("Effect", "7").unwrap();
        assert_eq!(s.argb_color, "#112233");
        assert_eq!(s.effect, 7);
        assert!(s.set_field("Nope", "1").is_err());
    }
}
