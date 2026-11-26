//! Порт Mouse/McMouse.cs: мыши MCHOSE (A7 / A7 Pro / A7 Ultra, L7, M7 …).
//! Протокол из официального веб-драйвера M HUB: feature-отчёты 0x11 (20 байт) и 0x12 (64 байта),
//! каждый байт данных инвертирован (XOR 0xFF). Запрос: [id, ~команда, ~данные…, 0xFF…];
//! ответ: [id, ~команда, ~данные…].

use hidapi::{HidApi, HidDevice};
use std::thread::sleep;
use std::time::Duration;

pub const SHORT_REPORT: u8 = 0x11;
pub const LONG_REPORT: u8 = 0x12;
pub const SHORT_LEN: usize = 20;
pub const LONG_LEN: usize = 64;

const MCHOSE_PIDS: &[u16] = &[
    0x1001, 0x1009, 0x100A, 0x100B, 0x100C, 0x4010, 0x4011, 0x4015, 0x4016, 0x4018, 0x4019, 0x401B, 0x4021, 0x4022,
    0x4023, 0x4110, 0x4150,
];
const DONGLES_1K: &[u16] = &[0x1021];
const DONGLES: &[u16] = &[0x1020, 0x1021, 0x1001, 0x1009, 0x100A, 0x100B, 0x100C];
const ULTRA_PIDS: &[u16] = &[0x0011, 0x1020, 0x1021, 0x4110];

pub const RATES_8K: &[u32] = &[125, 500, 1000, 2000, 4000, 8000];
pub const RATES_1K: &[u32] = &[125, 500, 1000];

/// Состояние мыши из команды 0x11 0x06.
#[derive(Debug, Clone)]
pub struct MouseStatus {
    pub vid: u16,
    pub pid: u16,
    pub firmware: u32,
    pub connect_mode: u8,
    pub online: bool,
    pub battery: u8,
    pub charge: u8,
    pub raw: String,
}

impl MouseStatus {
    /// Мышь на связи (донгл без мыши отдаёт pid 0 и заряд 0).
    pub fn has_mouse(&self) -> bool {
        self.pid != 0 && (1..=100).contains(&self.battery)
    }
    pub fn charging(&self) -> bool {
        self.charge != 0
    }
}

/// Назначение кнопки: тип и значение (3 байта). Тип 0 — родная функция кнопки.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MouseKey {
    pub kind: u8,
    pub value: u32,
}

/// Настройки текущего профиля из команды 0x12 0x67.
#[derive(Debug, Clone)]
pub struct MouseConfig {
    pub profile: u8,
    pub g_dpi: u8,
    pub g_rate: u8,
    pub usb_dpi: u8,
    pub usb_rate: u8,
    pub reserved: u8,
    pub dpi: [u16; 6],
    pub dpi_y: [u16; 6],
    pub count: u8,
    pub valid: bool,
    pub sensor: u8,
    pub debounce: u8,
    pub sleep: u8,
    pub rotate: i8,
    pub keys: [MouseKey; 6],
    pub raw: String,
}

impl MouseConfig {
    /// Высота отрыва: 0 — 0,7 мм, 1 — 1 мм, 2 — 2 мм.
    pub fn lod(&self) -> u8 {
        match self.sensor & 3 {
            1 => 1,
            2 => 2,
            _ => 0,
        }
    }
    pub fn ripple(&self) -> bool {
        self.sensor >> 2 & 1 != 0
    }
    pub fn angle_snap(&self) -> bool {
        self.sensor >> 3 & 1 != 0
    }
    pub fn motion_sync(&self) -> bool {
        self.sensor >> 4 & 1 != 0
    }
    /// 0 — «Мощность», 1 — «Игровой», 2 — «Супер».
    pub fn mode(&self) -> u8 {
        if self.sensor >> 7 & 1 == 0 {
            0
        } else if self.sensor >> 6 & 1 == 0 {
            1
        } else {
            2
        }
    }
}

fn hex(data: &[u8], count: usize) -> String {
    data.iter().take(count).map(|b| format!("{b:02X}")).collect()
}

/// Пакет записи: данные инвертируются, остаток — 0xFF (то есть инвертированный 0).
pub fn encode(data: &[u8], length: usize) -> Vec<u8> {
    let mut payload = vec![0xFFu8; length];
    for (i, &b) in data.iter().take(length).enumerate() {
        payload[i] = !b;
    }
    payload
}

/// Разбор ответа 0x11 0x06.
pub fn parse_status(d: &[u8]) -> Option<MouseStatus> {
    if d.len() < 11 {
        return None;
    }
    let flags = d[8];
    Some(MouseStatus {
        vid: d[0] as u16 | (d[1] as u16) << 8,
        pid: d[2] as u16 | (d[3] as u16) << 8,
        firmware: d[4] as u32 | (d[5] as u32) << 8 | (d[6] as u32) << 16 | (d[7] as u32) << 24,
        connect_mode: flags & 0x07,
        online: flags >> 3 & 1 != 0,
        battery: d[9],
        charge: d[10],
        raw: hex(d, 12),
    })
}

/// Разбор ответа 0x12 0x67.
pub fn parse_config(d: &[u8]) -> Option<MouseConfig> {
    if d.len() < 63 {
        return None;
    }
    let mut dpi = [0u16; 6];
    let mut dpi_y = [0u16; 6];
    for i in 0..6 {
        dpi[i] = d[4 + i * 2] as u16 | (d[5 + i * 2] as u16) << 8;
        dpi_y[i] = d[51 + i * 2] as u16 | (d[52 + i * 2] as u16) << 8;
        if !(50..=50000).contains(&dpi_y[i]) {
            dpi_y[i] = dpi[i];
        }
    }
    let mut keys = [MouseKey::default(); 6];
    for (i, k) in keys.iter_mut().enumerate() {
        let at = 20 + i * 4;
        *k = MouseKey { kind: d[at] & 0x0F, value: (d[at + 1] as u32) << 16 | (d[at + 2] as u32) << 8 | d[at + 3] as u32 };
    }
    let count = d[16];
    let usb_dpi = d[1] & 0x0F;
    let g_dpi = d[2] & 0x0F;
    let valid = (1..=6).contains(&count)
        && g_dpi < 6
        && usb_dpi < 6
        && dpi.iter().take(count as usize).all(|v| (50..=50000).contains(v));
    Some(MouseConfig {
        profile: d[0],
        g_dpi,
        g_rate: d[2] >> 4,
        usb_dpi,
        usb_rate: d[1] >> 4,
        reserved: d[3],
        dpi,
        dpi_y,
        count,
        valid,
        sensor: d[17],
        debounce: d[18],
        sleep: d[19],
        rotate: d[49] as i8,
        keys,
        raw: hex(d, 24),
    })
}

/// Данные команды 0x12 0x40 (уровни DPI X/Y и выбранный уровень) — как в M HUB.
pub fn dpi_command(reserved: u8, dpi: &[u16; 6], dpi_y: &[u16; 6], count: usize, selected: usize) -> Vec<u8> {
    let count = count.clamp(1, 6);
    let selected = selected.min(count - 1);
    let mut data = vec![0x40, selected as u8, selected as u8, reserved];
    for v in dpi {
        let v = (*v).clamp(50, 50000);
        data.extend_from_slice(&v.to_le_bytes());
    }
    data.push(count as u8);
    data.push(0xFF); // diff — всегда 0xFF, как в M HUB
    for v in dpi_y {
        let v = (*v).clamp(50, 50000);
        data.extend_from_slice(&v.to_le_bytes());
    }
    data
}

fn is_candidate(vid: u16, pid: u16, usage_page: u16) -> bool {
    (vid == 0x5253
        || (vid == 0x3837 && MCHOSE_PIDS.contains(&pid))
        || (vid == 0x41E4 && (pid == 0x1101 || pid == 0x1102))
        || (vid == 0x0BDA && pid == 0x4762))
        && (usage_page == 0xFF01 || usage_page == 0xFF0B)
}

pub struct McMouse {
    hid: HidDevice,
    pub name: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub usage_page: u16,
}

impl McMouse {
    /// Ищет мышь: интерфейс, который отвечает на запрос состояния. Второе значение — журнал поиска.
    pub fn find(api: &HidApi) -> (Option<(Self, Option<MouseStatus>)>, String) {
        let mut log = Vec::new();
        let mut best: Option<(Self, Option<MouseStatus>)> = None;
        for d in api.device_list().filter(|d| is_candidate(d.vendor_id(), d.product_id(), d.usage_page())) {
            let iface = format!("{:04X}:{:04X} 0x{:04X}", d.vendor_id(), d.product_id(), d.usage_page());
            let hid = match api.open_path(d.path()) {
                Ok(h) => h,
                Err(e) => {
                    log.push(format!("{iface}: не открыть: {e}"));
                    continue;
                }
            };
            let name = d.product_string().unwrap_or("").trim().to_string();
            let probe = McMouse {
                hid,
                name: if name.is_empty() { "Мышь MCHOSE".into() } else { name.clone() },
                vendor_id: d.vendor_id(),
                product_id: d.product_id(),
                usage_page: d.usage_page(),
            };
            let status = probe.read_status();
            log.push(format!("{iface} «{name}»: {}", status.as_ref().map(|s| s.raw.clone()).unwrap_or_else(|| "нет ответа".into())));
            // Лучше тот, через который видно саму мышь (заряд); иначе — первый ответивший.
            let better = match (&status, &best) {
                (Some(_), None) => true,
                (Some(s), Some((_, bs))) => s.has_mouse() && !bs.as_ref().map(|b| b.has_mouse()).unwrap_or(false),
                _ => false,
            };
            if better {
                best = Some((probe, status));
            }
        }
        let log = if log.is_empty() { "Подходящих HID-интерфейсов MCHOSE нет.".to_string() } else { log.join("\n") };
        (best, log)
    }

    /// Подключено через донгл (по USB-ID интерфейса).
    pub fn via_dongle(&self) -> bool {
        DONGLES.contains(&self.product_id)
    }

    /// Частоты опроса для текущего подключения.
    pub fn rates(&self) -> &'static [u32] {
        if self.via_dongle() && DONGLES_1K.contains(&self.product_id) {
            RATES_1K
        } else {
            RATES_8K
        }
    }

    pub fn is_ultra(&self, status: Option<&MouseStatus>) -> bool {
        ULTRA_PIDS.contains(&self.product_id) || status.map(|s| ULTRA_PIDS.contains(&s.pid)).unwrap_or(false)
    }

    /// Потолок DPI: у A7 Ultra — 42000, у остальных A7 — 26000.
    pub fn max_dpi(&self, status: Option<&MouseStatus>) -> u16 {
        if self.is_ultra(status) { 42000 } else { 26000 }
    }

    pub fn model(&self, status: Option<&MouseStatus>) -> String {
        if self.is_ultra(status) {
            return "A7 Ultra".into();
        }
        let n = self.name.replace("MCHOSE", "").trim().to_string();
        let low = n.to_lowercase();
        if n.is_empty() || low.contains("dongle") || low.contains("receiver") { "Мышь MCHOSE".into() } else { n }
    }

    /// SetFeature с Report ID; как в C#, пробуем точную длину, затем 65 байт (дополнение 0xFF).
    fn set_feature(&self, id: u8, payload: &[u8]) -> Result<(), String> {
        let mut last = String::new();
        for len in [payload.len() + 1, 65] {
            let mut buf = vec![0xFFu8; len.max(payload.len() + 1)];
            buf[0] = id;
            buf[1..1 + payload.len()].copy_from_slice(payload);
            match self.hid.send_feature_report(&buf) {
                Ok(()) => return Ok(()),
                Err(e) => last = e.to_string(),
            }
        }
        Err(format!("SetFeature 0x{id:02X}: {last}"))
    }

    fn get_feature(&self, id: u8, length: usize) -> Option<Vec<u8>> {
        for len in [length + 1, 65] {
            let mut buf = vec![0u8; len];
            buf[0] = id;
            if let Ok(n) = self.hid.get_feature_report(&mut buf) {
                buf.truncate(n.max(1));
                return Some(buf);
            }
        }
        None
    }

    fn write(&self, id: u8, length: usize, data: &[u8]) -> Result<(), String> {
        self.set_feature(id, &encode(data, length))?;
        sleep(Duration::from_millis(30));
        Ok(())
    }

    /// Запрос чтения и ответ: данные уже без инверсии (после [id, команда]).
    /// Через донгл ответ мыши приходит по радио с задержкой: пока его нет, донгл отдаёт
    /// «пустой» ответ (все данные = 0). Такой ответ считаем неготовым и ждём дальше,
    /// при необходимости повторяем запрос. RIGCONTROL_DEBUG=1 печатает каждый ответ.
    fn read(&self, id: u8, length: usize, command: u8, args: &[u8]) -> Option<Vec<u8>> {
        let debug = std::env::var("RIGCONTROL_DEBUG").map(|v| v == "1").unwrap_or(false);
        let mut data = vec![command];
        data.extend_from_slice(args);
        let mut empty: Option<Vec<u8>> = None;
        for round in 0..3 {
            self.set_feature(id, &encode(&data, length)).ok()?;
            for attempt in 0..12 {
                sleep(Duration::from_millis(if attempt == 0 { 20 } else { 40 }));
                let Some(reply) = self.get_feature(id, length) else { continue };
                if debug {
                    eprintln!("[0x{id:02X} cmd 0x{command:02X} попытка {round}.{attempt}] {}", hex(&reply, reply.len()));
                }
                if reply.len() < 3 || reply[0] != id || !reply[1] != command {
                    continue;
                }
                let decoded: Vec<u8> = reply[2..].iter().map(|b| !b).collect();
                if decoded.iter().all(|&b| b == 0) {
                    empty = Some(decoded);
                    continue;
                }
                return Some(decoded);
            }
        }
        empty
    }

    pub fn read_status(&self) -> Option<MouseStatus> {
        parse_status(&self.read(SHORT_REPORT, SHORT_LEN, 0x06, &[])?)
    }

    /// v22: свежее состояние (заряд). Через донгл первый непустой ответ бывает копией,
    /// которую донгл запомнил раньше (отсюда «зависшие» 57 % при реальных 60 %), поэтому
    /// спрашиваем дважды и берём последний ответ, в котором видна мышь. Всё пишется в battery.log.
    pub fn read_status_fresh(&self) -> Option<MouseStatus> {
        let first = self.read_status();
        let second = if self.via_dongle() {
            sleep(Duration::from_millis(300));
            self.read_status()
        } else {
            None
        };
        let result = match &second {
            Some(s) if s.has_mouse() => second.clone(),
            _ => first.clone(),
        };
        log_mouse_battery(first.as_ref(), second.as_ref(), result.as_ref());
        result
    }

    /// Версия прошивки мыши или приёмника (0x11 0x04 00 / AA).
    pub fn read_version(&self, receiver: bool) -> Option<String> {
        let d = self.read(SHORT_REPORT, SHORT_LEN, 0x04, &[if receiver { 0xAA } else { 0x00 }])?;
        let n = *d.first()? as usize;
        if n == 0 || n > 16 {
            return None;
        }
        let chars: Vec<u8> = d.iter().skip(1).take(n).copied().take_while(|&b| b != 0).collect();
        if chars.is_empty() || chars.iter().any(|&c| !(0x20..=0x7E).contains(&c)) {
            return None;
        }
        let text = String::from_utf8_lossy(&chars).trim().to_string();
        Some(if text.starts_with(|c: char| c.is_ascii_digit()) { format!("V{text}") } else { text })
    }

    pub fn read_config(&self) -> Option<MouseConfig> {
        parse_config(&self.read(LONG_REPORT, LONG_LEN, 0x67, &[])?)
    }

    pub fn write_dpi(&self, cfg: &MouseConfig, dpi: &[u16; 6], dpi_y: &[u16; 6], count: usize, selected: usize) -> Result<(), String> {
        self.write(LONG_REPORT, LONG_LEN, &dpi_command(cfg.reserved, dpi, dpi_y, count, selected))
    }

    /// Частота опроса (индекс в списке). По радио — второй байт, по кабелю — первый.
    pub fn write_rate(&self, index: u8, wireless: bool) -> Result<(), String> {
        self.write(SHORT_REPORT, SHORT_LEN, &[0x41, if wireless { 0 } else { index }, if wireless { index } else { 0 }])
    }

    /// 0x11 0x42: высота отрыва, переключатели (1 — вкл, 2 — выкл, 0 — не трогать),
    /// режим (1 — мощность, 2 — игровой, 3 — супер, 0 — не трогать), поворот сенсора.
    pub fn write_sensor(&self, lod: u8, ripple: u8, line: u8, motion_sync: u8, mode: u8, rotate: Option<i8>) -> Result<(), String> {
        let r = rotate.map(|v| v.clamp(-30, 30)).unwrap_or(0);
        self.write(SHORT_REPORT, SHORT_LEN, &[0x42, lod, ripple, line, motion_sync, 0, 0, mode, rotate.is_some() as u8, r as u8])
    }

    pub fn write_debounce(&self, ms: u8) -> Result<(), String> {
        self.write(SHORT_REPORT, SHORT_LEN, &[0x43, ms.min(20)])
    }

    pub fn write_sleep(&self, minutes: u8) -> Result<(), String> {
        self.write(SHORT_REPORT, SHORT_LEN, &[0x0A, (minutes > 0) as u8, minutes])
    }

    pub fn switch_profile(&self, index: u8) -> Result<(), String> {
        self.write(SHORT_REPORT, SHORT_LEN, &[0x58, index.min(2)])
    }

    pub fn reset_keys(&self) -> Result<(), String> {
        self.write(SHORT_REPORT, SHORT_LEN, &[0x0B, 0x01, 0x01])
    }

    /// Назначить кнопке функцию — «0x12 0x52» [кнопка, 0, тип, значение]. Тип 0 — вернуть родную.
    pub fn write_key(&self, button: u8, key: MouseKey) -> Result<(), String> {
        self.write(LONG_REPORT, LONG_LEN, &key_command(button, key))
    }

    /// Заводские настройки текущего профиля («0x11 0x0B», 0xAA00).
    pub fn factory_reset(&self) -> Result<(), String> {
        self.write(SHORT_REPORT, SHORT_LEN, &[0x0B, 0xAA, 0x00])
    }
}

/// Тело команды назначения кнопки (WriteKey в C#).
/// %AppData%\RigControl\battery.log — заряд мыши и сырые ответы 0x11 0x06 (только изменения, до 100 КБ).
fn log_mouse_battery(first: Option<&MouseStatus>, second: Option<&MouseStatus>, result: Option<&MouseStatus>) {
    static LAST: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
    let raw = |s: Option<&MouseStatus>| s.map(|s| format!("{}%/{}", s.battery, s.raw)).unwrap_or_else(|| "—".into());
    let text = match result {
        Some(s) if s.has_mouse() => format!(
            "мышь {} %{} (1: {} | 2: {})",
            s.battery,
            if s.charging() { " заряжается" } else { "" },
            raw(first),
            raw(second)
        ),
        _ => format!("мышь не на связи (1: {} | 2: {})", raw(first), raw(second)),
    };
    let Ok(mut last) = LAST.lock() else { return };
    if last.as_deref() == Some(text.as_str()) {
        return;
    }
    *last = Some(text.clone());
    crate::logfile::append_ex(
        "battery.log",
        &format!("{} {text}\n", chrono::Local::now().format("%d.%m %H:%M:%S")),
        100_000,
        false,
    );
}

pub fn key_command(button: u8, key: MouseKey) -> Vec<u8> {
    vec![0x52, button, 0, key.kind, (key.value >> 16 & 0xFF) as u8, (key.value >> 8 & 0xFF) as u8, (key.value & 0xFF) as u8]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_command_layout() {
        let k = MouseKey { kind: 2, value: 0x011D00 };
        assert_eq!(key_command(3, k), vec![0x52, 3, 0, 2, 0x01, 0x1D, 0x00]);
    }

    #[test]
    fn encode_inverts_and_pads() {
        assert_eq!(encode(&[0x41, 0x02], 4), vec![0xBE, 0xFD, 0xFF, 0xFF]);
    }

    #[test]
    fn dpi_command_layout() {
        let d = dpi_command(7, &[400, 800, 1600, 3200, 6400, 26000], &[400, 800, 1600, 3200, 6400, 26000], 4, 9);
        assert_eq!(&d[..4], &[0x40, 3, 3, 7]);
        assert_eq!(&d[4..6], &[0x90, 0x01]);
        assert_eq!(d[16], 4);
        assert_eq!(d[17], 0xFF);
        assert_eq!(d.len(), 4 + 12 + 2 + 12);
    }

    #[test]
    fn config_roundtrip_fields() {
        let mut d = vec![0u8; 63];
        d[1] = 0x52; // кабель: частота 5, уровень 2
        d[2] = 0x21;
        d[4] = 0x90;
        d[5] = 0x01; // 400
        d[16] = 1;
        d[17] = 0b1101_0101;
        d[49] = 0xF6; // -10
        let c = parse_config(&d).unwrap();
        assert!(c.valid);
        assert_eq!((c.usb_rate, c.usb_dpi, c.g_rate, c.g_dpi), (5, 2, 2, 1));
        assert_eq!(c.dpi[0], 400);
        assert_eq!(c.dpi_y[0], 400);
        assert_eq!((c.lod(), c.ripple(), c.motion_sync(), c.mode()), (1, true, true, 2));
        assert_eq!(c.rotate, -10);
    }

    #[test]
    fn status_parse() {
        let d = [0x53, 0x52, 0x20, 0x10, 1, 2, 3, 4, 0x09, 80, 1, 0];
        let s = parse_status(&d).unwrap();
        assert_eq!((s.vid, s.pid, s.connect_mode, s.online, s.battery), (0x5253, 0x1020, 1, true, 80));
        assert!(s.has_mouse() && s.charging());
    }
}
