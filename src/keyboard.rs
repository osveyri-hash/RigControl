//! Клавиатура AK820 Pro «как получится»: по кабелю (0xFF13/0xFF68), а если кабеля нет —
//! по радио через донгл 2,4 ГГц (0xFF60). Аналог пары `Ak820Device? device, Ak820Dongle? dongle`
//! и функции `Send(...)` из MainWindow.xaml.cs: окно и служба больше не падают с ошибкой
//! «донгл не отдаёт интерфейсы команд», а шлют подсветку по радио.
//!
//! У донгла два канала 0xFF60 (интерфейсы #3 и #4). Раньше брался первый по номеру,
//! но подсветку принимает только один из них. Теперь опрашиваем каждый командой 0x02
//! и берём тот, что ответил (если не ответил никто — первый, как в C#).

use crate::device::Ak820Device;
use crate::dongle::Ak820Dongle;
use crate::keypaint::USER_LIGHT_MODE;
use crate::protocol;
use hidapi::HidApi;

pub enum Keyboard {
    Wired(Ak820Device),
    Radio(Ak820Dongle),
}

impl Keyboard {
    /// Сначала кабель, потом донгл.
    pub fn open(api: &HidApi) -> Result<Self, String> {
        match Ak820Device::open(api) {
            Ok(d) => Ok(Keyboard::Wired(d)),
            Err(wired_err) => {
                if !Ak820Dongle::present(api) {
                    return Err(wired_err);
                }
                open_dongle(api).map(Keyboard::Radio)
            }
        }
    }

    pub fn via_dongle(&self) -> bool {
        matches!(self, Keyboard::Radio(_))
    }

    pub fn describe(&self) -> String {
        match self {
            Keyboard::Wired(_) => "по кабелю USB-C".into(),
            Keyboard::Radio(d) => format!("по радио: {}", d.description),
        }
    }

    /// Встроенный эффект подсветки (режим, цвет #RRGGBB, радуга, яркость 0–5, скорость 0–5, направление 0–3).
    pub fn set_effect(&mut self, mode: i32, color: &str, rainbow: bool, brightness: i32, speed: i32, direction: i32) -> Result<(), String> {
        let (r, g, b) = protocol::parse_color(Some(color), (255, 255, 255));
        let result = match self {
            Keyboard::Wired(d) => d.set_effect(mode, color, rainbow, brightness, speed, direction),
            Keyboard::Radio(d) => d.send_light(mode, r, g, b, rainbow, brightness, speed, direction),
        };
        // Запоминаем, что горит: по этому вентиляторы идут в такт с клавиатурой.
        if result.is_ok() && mode != USER_LIGHT_MODE {
            crate::light_mirror::effect(mode, r, g, b, rainbow, brightness, speed, direction, true);
        }
        result
    }

    /// Быстрая смена цвета без записи во флеш (подсветка в такт гифке). По радио — обычный пакет подсветки.
    #[allow(clippy::too_many_arguments)]
    pub fn set_effect_fast(&mut self, mode: i32, c: crate::Rgb, rainbow: bool, brightness: i32, speed: i32, direction: i32) -> Result<(), String> {
        match self {
            Keyboard::Wired(d) => d.set_effect_fast(mode, c.0, c.1, c.2, rainbow, brightness, speed, direction),
            Keyboard::Radio(d) => {
                let r = d.send_light(mode, c.0, c.1, c.2, rainbow, brightness, speed, direction);
                if r.is_ok() {
                    crate::light_mirror::effect(mode, c.0, c.1, c.2, rainbow, brightness, speed, direction, false);
                }
                r
            }
        }
    }

    /// Своя раскраска по клавишам (таблица 576 байт).
    pub fn set_user_light(&mut self, frame: &[u8]) -> Result<(), String> {
        let result = match self {
            Keyboard::Wired(d) => d.set_key_colors(frame, true, true),
            Keyboard::Radio(d) => d.set_user_light(frame),
        };
        if result.is_ok() {
            crate::light_mirror::user_light(frame);
        }
        result
    }

    pub fn sync_time(&mut self, now: protocol::DateTimeParts) -> Result<(), String> {
        match self {
            Keyboard::Wired(d) => d.sync_time(now),
            Keyboard::Radio(d) => d.sync_time(now),
        }
    }

    /// Заряд батареи есть только по радио.
    pub fn battery(&mut self) -> Option<u8> {
        match self {
            Keyboard::Wired(_) => None,
            Keyboard::Radio(d) => d.read_battery_for(3000).ok().flatten(),
        }
    }
}

/// Режим своей раскраски — чтобы вызывающий код не тянул keypaint.
pub const USER_MODE: i32 = USER_LIGHT_MODE;

/// Открыть канал донгла, который реально отвечает (см. комментарий к модулю).
pub fn open_dongle(api: &HidApi) -> Result<Ak820Dongle, String> {
    let ifaces = Ak820Dongle::interfaces(api);
    let mut first_err = None;
    for n in &ifaces {
        match Ak820Dongle::open_interface(api, Some(*n)) {
            Ok(mut d) => {
                if let Ok(Some(_)) = d.status() {
                    return Ok(d);
                }
            }
            Err(e) => {
                first_err.get_or_insert(e);
            }
        }
    }
    Ak820Dongle::open_interface(api, ifaces.first().copied())
        .map_err(|e| first_err.unwrap_or(e))
}


impl Keyboard {
    /// Устройство по кабелю — для живых кадров (свои эффекты, анимация раскраски).
    pub fn wired_mut(&mut self) -> Option<&mut Ak820Device> {
        match self {
            Keyboard::Wired(d) => Some(d),
            Keyboard::Radio(_) => None,
        }
    }

    /// Выставляет подсветку по настройкам один раз (без потока кадров), как ApplyLightingAsync в C#:
    /// «Своя раскраска» — таблицей цветов (работает и по радио), свой эффект — ближайшим встроенным
    /// (кадры рисует [`crate::kb_light`] только по кабелю), остальное — встроенным эффектом прошивки.
    pub fn apply_settings(&mut self, s: &crate::settings::Settings) -> Result<(), String> {
        let mode = protocol::effect_mode(s.effect);
        if mode == crate::effects::PAINT {
            let p: crate::keypaint::Paint = s.key_paint.clone().unwrap_or_default().into_iter().collect();
            self.set_user_light(&crate::keypaint::build_frame(Some(&p), s.brightness))
        } else if crate::effects::is_custom(mode) {
            let (m, rainbow) = builtin_for_custom(mode, s.rainbow);
            self.set_effect(m, &s.color, rainbow, s.brightness, s.speed, s.direction)
        } else {
            self.set_effect(mode, &s.color, s.rainbow, s.brightness, s.speed, s.direction)
        }
    }

    /// «Выключить»: режим 0 в прошивке «залипает» (после него обычные пакеты игнорируются),
    /// поэтому гасим нулевой яркостью текущего эффекта, как OnLightOff в C#. on=true — вернуть яркость.
    pub fn set_light_on(&mut self, s: &crate::settings::Settings, on: bool) -> Result<(), String> {
        let mut mode = protocol::effect_mode(s.effect);
        if mode == 0 || crate::effects::is_custom(mode) {
            mode = 1;
        }
        let brightness = if on { s.brightness.clamp(0, 5).max(1) } else { 0 };
        self.set_effect(mode, &s.color, s.rainbow, brightness, s.speed.clamp(0, 5), s.direction.clamp(0, 3))
    }
}

/// Свой эффект без кабеля — ближайший встроенный (как в C#): спектр → «Спектр», волна от центра,
/// течение и вращение — такие же радужные эффекты прошивки, остальные — «Прокрутка».
pub fn builtin_for_custom(mode: i32, rainbow: bool) -> (i32, bool) {
    use crate::effects;
    let m = match mode {
        effects::SPECTRUM => 8,
        effects::CENTER_WAVE => 9,
        effects::FLOW => 16,
        effects::ROTATE => 12,
        _ => 10,
    };
    (m, rainbow || effects::is_rainbow_flow(mode))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_on_radio() {
        assert_eq!(builtin_for_custom(crate::effects::SPECTRUM, false), (8, true));
        assert_eq!(builtin_for_custom(crate::effects::WAVE_RIGHT, false), (10, false));
        assert_eq!(builtin_for_custom(crate::effects::BREATHE, true), (10, true));
    }
}
