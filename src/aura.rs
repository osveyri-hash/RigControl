//! Порт Argb/AuraUsb.cs: USB-контроллер подсветки материнских плат ASUS (AURA, «AULA3-…»).
//! VID 0x0B05, usage page 0xFF72, 65-байтные отчёты с Report ID 0xEC.
//! Протокол как в OpenRGB (Asus Aura USB / Mainboard): прямой режим 0x40 — цвета каждого
//! светодиода от программы, по 20 штук в отчёте; встроенные эффекты — 0x35/0x36, применить — 0x3F 0x55.

use hidapi::{HidApi, HidDevice};

pub const VENDOR_ID: u16 = 0x0B05;
pub const USAGE_PAGE: u16 = 0xFF72;
pub const REPORT_ID: u8 = 0xEC;
pub const LEDS_PER_REPORT: usize = 20;
pub const REPORT_LEN: usize = 65;
/// Канал прямого режима для светодиодов платы и RGB-разъёма 12 В.
pub const FIXED_DIRECT_CHANNEL: u8 = 0x04;
/// Максимум светодиодов на ленте (ArgbEngine.MaxLeds).
pub const MAX_LEDS: usize = 120;

/// Встроенные эффекты контроллера (крутит сама плата).
pub const EFFECTS: &[(&str, u8)] = &[
    ("off", 0),
    ("static", 1),
    ("breathing", 2),
    ("flash", 3),
    ("cycle", 4),
    ("rainbow", 5),
];

pub struct AuraUsb {
    hid: HidDevice,
    pub product_id: u16,
    /// Строка прошивки, например «AULA3-AR32-0207».
    pub firmware: String,
    /// Таблица настроек контроллера (60 байт).
    pub config_table: Vec<u8>,
    /// Сколько адресных (ARGB) разъёмов на плате.
    pub argb_headers: usize,
    /// Светодиоды самой платы и 12-вольтовые RGB-разъёмы (байт 0x1B таблицы).
    pub mainboard_leds: usize,
}

fn report(data: &[u8]) -> [u8; REPORT_LEN] {
    let mut r = [0u8; REPORT_LEN];
    r[0] = REPORT_ID;
    r[1..1 + data.len()].copy_from_slice(data);
    r
}

/// Пакеты прямого режима для одного канала (SendDirect в C#) — отдельно, чтобы проверять тестами.
pub fn direct_reports(channel: u8, rgb: &[u8], count: usize) -> Vec<[u8; REPORT_LEN]> {
    let count = count.clamp(1, (rgb.len() / 3).max(1));
    let mut out = Vec::new();
    let mut start = 0;
    while start < count {
        let n = LEDS_PER_REPORT.min(count - start);
        let last = start + n >= count;
        let mut r = [0u8; REPORT_LEN];
        r[0] = REPORT_ID;
        r[1] = 0x40;
        r[2] = channel | if last { 0x80 } else { 0x00 }; // 0x80 — «показать»
        r[3] = start as u8;
        r[4] = n as u8;
        let src = &rgb[start * 3..((start + n) * 3).min(rgb.len())];
        r[5..5 + src.len()].copy_from_slice(src);
        out.push(r);
        start += n;
    }
    out
}

/// Пакет цвета встроенного эффекта (0x36) для канала эффектов; None — маска не помещается в 16 бит.
pub fn effect_color_report(channel: u8, mainboard_leds: usize, rgb: crate::Rgb) -> Option<[u8; REPORT_LEN]> {
    let (start, count) = if channel == 0 { (0, mainboard_leds.max(1)) } else { (mainboard_leds + channel as usize - 1, 1) };
    if start + count > 16 {
        return None;
    }
    let mut r = [0u8; REPORT_LEN];
    r[0] = REPORT_ID;
    r[1] = 0x36;
    let mask: u16 = (((1u32 << count) - 1) << start) as u16;
    r[2] = (mask >> 8) as u8;
    r[3] = mask as u8;
    r[4] = 0;
    for i in start..start + count {
        r[5 + i * 3] = rgb.0;
        r[6 + i * 3] = rgb.1;
        r[7 + i * 3] = rgb.2;
    }
    Some(r)
}

impl AuraUsb {
    /// Ищет контроллер. Ошибка — на плате его нет (или подсветка на SMBus, а не на USB).
    pub fn find(api: &HidApi) -> Result<Self, String> {
        let list: Vec<_> = api.device_list().filter(|d| d.vendor_id() == VENDOR_ID && d.usage_page() == USAGE_PAGE).collect();
        if list.is_empty() {
            return Err("Контроллер ASUS AURA (USB 0B05, usage page 0xFF72) не найден. Закройте Armoury Crate / \
                        Aura Sync / OpenRGB, если запущены, — они занимают устройство."
                .into());
        }
        let mut errors = String::new();
        for d in list {
            let hid = match api.open_path(d.path()) {
                Ok(h) => h,
                Err(e) => {
                    errors += &format!("  {:04X}:{:04X}: не открыть: {e}\n", d.vendor_id(), d.product_id());
                    continue;
                }
            };
            match Self::read_firmware(&hid) {
                Some(firmware) => {
                    let table = Self::read_config_table(&hid);
                    let argb_headers = match table.get(2) { Some(&v) if (1..=8).contains(&v) => v as usize, _ => 1 };
                    let mainboard_leds = match table.get(0x1B) { Some(&v) if v <= 12 => v as usize, _ => 0 };
                    return Ok(Self { hid, product_id: d.product_id(), firmware, config_table: table, argb_headers, mainboard_leds });
                }
                None => errors += &format!("  {:04X}:{:04X}: не ответил на запрос прошивки\n", d.vendor_id(), d.product_id()),
            }
        }
        Err(format!("Контроллер AURA найден, но не отвечает:\n{errors}"))
    }

    /// Запрос и ответ: ответ начинается с 0xEC и кода answer.
    fn request(hid: &HidDevice, command: u8, answer: u8) -> Option<Vec<u8>> {
        hid.write(&report(&[command])).ok()?;
        let mut buf = [0u8; 256];
        for _ in 0..4 {
            let n = hid.read_timeout(&mut buf, 400).ok()?;
            if n == 0 {
                return None;
            }
            let reply = &buf[..n];
            if reply.len() > 2 && reply[0] == REPORT_ID && reply[1] == answer {
                return Some(reply.to_vec());
            }
        }
        None
    }

    fn read_firmware(hid: &HidDevice) -> Option<String> {
        let reply = Self::request(hid, 0x82, 0x02)?;
        let end = reply.len().min(18);
        let text: String = reply[2..end].iter().map(|&b| b as char).collect();
        let text = text.trim_end_matches(['\0', ' ']).to_string();
        Some(if text.is_empty() { "ASUS Aura".into() } else { text })
    }

    /// Таблица настроек (0xB0): байт 2 — число ARGB-разъёмов, 0x1B — светодиодов платы.
    fn read_config_table(hid: &HidDevice) -> Vec<u8> {
        match Self::request(hid, 0xB0, 0x30) {
            Some(r) if r.len() >= 4 + 0x1C => r[4..r.len().min(64)].to_vec(),
            _ => Vec::new(),
        }
    }

    fn write(&self, r: &[u8; REPORT_LEN]) -> Result<(), String> {
        self.hid.write(r).map(|_| ()).map_err(|e| format!("AURA: {e}"))
    }

    fn send(&self, data: &[u8]) -> Result<(), String> {
        self.write(&report(data))
    }

    pub fn description(&self) -> String {
        let table: String = self.config_table.iter().map(|b| format!("{b:02X}")).collect();
        format!(
            "ASUS AURA USB {VENDOR_ID:04X}:{:04X} · прошивка {}\nARGB-разъёмов: {}, светодиодов платы: {}\nТаблица: {}",
            self.product_id, self.firmware, self.argb_headers, self.mainboard_leds, if table.is_empty() { "(нет)".into() } else { table }
        )
    }

    /// Прямой режим (эффект 0xFF). Каналы эффектов: 0 — плата и RGB 12 В, 1… — ARGB-разъёмы.
    pub fn enter_direct(&self, effect_channels: &[u8]) -> Result<(), String> {
        for &ch in effect_channels {
            self.send(&[0x35, ch, 0x00, 0x00, 0xFF])?;
        }
        self.send(&[0x3F, 0x55])
    }

    /// Встроенный эффект платы (0 — выкл, 1 — цвет, 2 — дыхание, 3 — вспышки, 4 — смена цветов, 5 — радуга).
    pub fn set_effect(&self, effect_channels: &[u8], mode: u8, rgb: crate::Rgb) -> Result<(), String> {
        for &ch in effect_channels {
            self.send(&[0x35, ch, 0x00, 0x00, mode])?;
            if let Some(r) = effect_color_report(ch, self.mainboard_leds, rgb) {
                self.write(&r)?;
            }
        }
        self.send(&[0x3F, 0x55])
    }

    /// Цвета светодиодов разъёма: rgb — по 3 байта на светодиод. direct_channel: 0… — ARGB-разъём, 4 — плата.
    pub fn send_direct(&self, direct_channel: u8, rgb: &[u8], count: usize) -> Result<(), String> {
        for r in direct_reports(direct_channel, rgb, count) {
            self.write(&r)?;
        }
        Ok(())
    }
}

/// Цвет радуги для позиции 0..1 (HSV с полной насыщенностью).
pub fn hue(h: f64) -> crate::Rgb {
    let h = (h.rem_euclid(1.0)) * 6.0;
    let x = 1.0 - (h % 2.0 - 1.0).abs();
    let (r, g, b) = match h as u32 {
        0 => (1.0, x, 0.0),
        1 => (x, 1.0, 0.0),
        2 => (0.0, 1.0, x),
        3 => (0.0, x, 1.0),
        4 => (x, 0.0, 1.0),
        _ => (1.0, 0.0, x),
    };
    ((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_splits_by_20_and_marks_last() {
        let rgb: Vec<u8> = (0..45 * 3).map(|i| i as u8).collect();
        let r = direct_reports(1, &rgb, 45);
        assert_eq!(r.len(), 3);
        assert_eq!(&r[0][..5], &[0xEC, 0x40, 0x01, 0, 20]);
        assert_eq!(&r[1][..5], &[0xEC, 0x40, 0x01, 20, 20]);
        assert_eq!(&r[2][..5], &[0xEC, 0x40, 0x81, 40, 5]);
        assert_eq!(r[1][5], 60);
        assert_eq!(r[2][5 + 15], 0); // хвост отчёта — нули
    }

    #[test]
    fn effect_mask() {
        // 3 светодиода платы, канал 2 = второй ARGB-разъём → бит 4.
        let r = effect_color_report(2, 3, (1, 2, 3)).unwrap();
        assert_eq!(&r[..5], &[0xEC, 0x36, 0x00, 0x10, 0x00]);
        assert_eq!(&r[5 + 12..5 + 15], &[1, 2, 3]);
        let b = effect_color_report(0, 3, (9, 9, 9)).unwrap();
        assert_eq!(&b[2..4], &[0x00, 0x07]);
        assert!(effect_color_report(1, 16, (0, 0, 0)).is_none());
    }

    #[test]
    fn hue_primaries() {
        assert_eq!(hue(0.0), (255, 0, 0));
        assert_eq!(hue(1.0 / 3.0).1, 255);
    }
}
