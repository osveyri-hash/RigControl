//! Порт Ak820Dongle.cs: клавиатура AK820 Pro по радио через донгл 2,4 ГГц (USB 0C45:FDFD).
//! Донгл отдаёт вендорский HID-интерфейс 0xFF60 с 32-байтными отчётами:
//! [команда, …, контрольная сумма], контрольная сумма — сумма первых 31 байта по модулю 256.
//! По радио работают часы, подсветка, своя раскраска и заряд батареи; экран — только по кабелю.
//!
//! Отличие от C#: hidapi сам убирает нулевой Report ID из входящих отчётов, поэтому
//! команда в ответе лежит в байте 0, а не в байте 1. На всякий случай проверяем оба варианта.

use crate::keypaint::USER_LIGHT_MODE;
use crate::protocol::{DateTimeParts, VENDOR_ID};
use hidapi::{HidApi, HidDevice};
use std::thread::sleep;
use std::time::{Duration, Instant};

pub const PRODUCT_ID: u16 = 0xFDFD;
pub const USAGE_PAGE: u16 = 0xFF60;
pub const PACKET_SIZE: usize = 32;
/// Своя раскраска уходит 21 порцией по 28 байт (576 байт таблицы + 12 нулей).
pub const PART: usize = 28;
pub const PARTS: usize = 21;

fn delay(ms: u64) {
    sleep(Duration::from_millis(ms));
}

/// Ставит контрольную сумму в последний байт.
pub fn finish_checksum(p: &mut [u8; PACKET_SIZE]) {
    let sum: u32 = p[..PACKET_SIZE - 1].iter().map(|&b| b as u32).sum();
    p[PACKET_SIZE - 1] = (sum & 0xFF) as u8;
}

/// Пакет из заголовка + нули + контрольная сумма (Packet в C#).
pub fn packet(head: &[u8]) -> [u8; PACKET_SIZE] {
    let mut p = [0u8; PACKET_SIZE];
    let n = head.len().min(PACKET_SIZE - 1);
    p[..n].copy_from_slice(&head[..n]);
    finish_checksum(&mut p);
    p
}

/// Пакет времени 0x0C (SyncTimeCore в C#).
pub fn time_packet(now: DateTimeParts) -> [u8; PACKET_SIZE] {
    let mut p = [0u8; PACKET_SIZE];
    p[0] = 0x0C;
    p[1] = 0x10;
    p[5] = 0x5A;
    p[6] = (now.year.rem_euclid(2000)) as u8;
    p[7] = now.month;
    p[8] = now.day;
    p[9] = now.hour;
    p[10] = now.minute;
    p[11] = now.second;
    p[13] = now.weekday % 7; // воскресенье = 0
    p[17] = 0xAA;
    p[18] = 0x55;
    finish_checksum(&mut p);
    p
}

/// Пакет подсветки по радио: [05 10 00] + 16 байт проводного пакета + нули + сумма.
#[allow(clippy::too_many_arguments)]
pub fn light_packet(mode: i32, r: u8, g: u8, b: u8, rainbow: bool, brightness: i32, speed: i32, direction: i32) -> [u8; PACKET_SIZE] {
    let mut p = [0u8; PACKET_SIZE];
    p[0] = 0x05;
    p[1] = 0x10;
    p[2] = 0x00;
    p[3] = mode as u8;
    if mode != 0 {
        p[4] = r;
        p[5] = g;
        p[6] = b;
        p[11] = rainbow as u8;
        p[12] = brightness.clamp(0, 5) as u8;
        p[13] = speed.clamp(0, 5) as u8;
        p[14] = direction.clamp(0, 3) as u8;
    }
    p[17] = 0xAA;
    p[18] = 0x55;
    finish_checksum(&mut p);
    p
}

/// Порция таблицы своей раскраски: [14 1C номер, 28 байт, сумма].
pub fn part_packet(frame: &[u8], index: usize) -> [u8; PACKET_SIZE] {
    let mut p = [0u8; PACKET_SIZE];
    p[0] = 0x14;
    p[1] = PART as u8;
    p[2] = index as u8;
    let from = index * PART;
    for i in 0..PART {
        p[3 + i] = frame.get(from + i).copied().unwrap_or(0);
    }
    finish_checksum(&mut p);
    p
}

/// Тело ответа без Report ID: если hidapi его не срезал (байт 0 = 0 и отчёт 33 байта) — срезаем сами.
fn body(report: &[u8]) -> &[u8] {
    if report.len() > PACKET_SIZE && report[0] == 0 { &report[1..] } else { report }
}

/// Варианты тела ответа: как пришло и без первого байта (если донгл шлёт ненулевой Report ID).
fn bodies(report: &[u8]) -> [&[u8]; 2] {
    let r = body(report);
    [r, if r.is_empty() { r } else { &r[1..] }]
}

/// Ответ на команду `cmd` (с учётом обоих смещений).
pub fn is_reply(report: &[u8], cmd: u8) -> bool {
    bodies(report).iter().any(|r| r.first() == Some(&cmd))
}

/// Заряд в ответе [20 01 00 заряд …], %; None — заряда нет.
pub fn level_of(report: &[u8]) -> Option<u8> {
    bodies(report).iter().find_map(|r| (r.len() > 3 && r[0] == 0x20 && r[1] == 0x01 && r[3] > 0).then(|| r[3].min(100)))
}

/// %AppData%\RigControl\battery.log — что ответила клавиатура (пишем только изменения, до 100 КБ).
fn log_battery(text: &str) {
    static LAST: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
    let Ok(mut last) = LAST.lock() else { return };
    if last.as_deref() == Some(text) {
        return;
    }
    *last = Some(text.to_string());
    crate::logfile::append_ex(
        "battery.log",
        &format!("{} {text}\n", chrono::Local::now().format("%d.%m %H:%M:%S")),
        100_000,
        false,
    );
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")
}

/// Разобранная часть ответа на 0x02. По железу: 02 00 00 40 30 00 00 45 0C 09 80 14 01 FF FF …
/// Байты 7–8 и 9–10 — VID/PID сопряжённой клавиатуры (0C45:8009, как по кабелю).
/// Смысл байтов 3–4 и 11–12 пока не известен (версия прошивки? заряд?).
pub struct DongleStatus {
    pub vendor_id: u16,
    pub product_id: u16,
    pub raw: Vec<u8>,
}

pub fn parse_status(report: &[u8]) -> Option<DongleStatus> {
    let r = body(report);
    if r.len() < 13 || r[0] != 0x02 {
        return None;
    }
    Some(DongleStatus {
        vendor_id: u16::from_le_bytes([r[7], r[8]]),
        product_id: u16::from_le_bytes([r[9], r[10]]),
        raw: r.to_vec(),
    })
}

pub struct Ak820Dongle {
    hid: HidDevice,
    pub description: String,
    /// Донгл хоть раз подтвердил подсветку — значит, без ответа пакет потерян.
    light_ack_seen: bool,
    /// Заряд, пойманный среди ответов на другие команды.
    stray_battery: Option<u8>,
    /// Печатать все пакеты в stderr (--debug).
    pub debug: bool,
}

impl Ak820Dongle {
    /// Есть ли донгл в системе (без открытия).
    pub fn present(api: &HidApi) -> bool {
        api.device_list().any(|d| d.vendor_id() == VENDOR_ID && d.product_id() == PRODUCT_ID && d.usage_page() == USAGE_PAGE)
    }

    /// Номера USB-интерфейсов с вендорским каналом 0xFF60 (у AK820 их два: #3 и #4).
    pub fn interfaces(api: &HidApi) -> Vec<i32> {
        let mut v: Vec<i32> = api
            .device_list()
            .filter(|d| d.vendor_id() == VENDOR_ID && d.product_id() == PRODUCT_ID && d.usage_page() == USAGE_PAGE)
            .map(|d| d.interface_number())
            .collect();
        v.sort();
        v.dedup();
        v
    }

    pub fn open(api: &HidApi) -> Result<Self, String> {
        Self::open_interface(api, None)
    }

    /// Открыть канал 0xFF60 на интерфейсе `number` (None — первый по порядку номеров).
    pub fn open_interface(api: &HidApi, number: Option<i32>) -> Result<Self, String> {
        let number = number.or_else(|| Self::interfaces(api).first().copied());
        let info = api
            .device_list()
            .find(|d| d.vendor_id() == VENDOR_ID && d.product_id() == PRODUCT_ID && d.usage_page() == USAGE_PAGE
                && Some(d.interface_number()) == number)
            .ok_or("Донгл 2,4 ГГц (0C45:FDFD) не найден: вставьте донгл и переключите клавиатуру в режим 2,4 ГГц.")?;
        let description = format!("Донгл 2,4 ГГц {:04X}:{:04X} (интерфейс #{})", info.vendor_id(), info.product_id(), info.interface_number());
        let hid = api.open_path(info.path()).map_err(|e| format!("Не открыть донгл: {e}"))?;
        Ok(Self { hid, description, light_ack_seen: false, stray_battery: None, debug: false })
    }

    /// Output-отчёт: Report ID 0 + 32 байта.
    fn write(&self, p: &[u8; PACKET_SIZE]) -> Result<(), String> {
        let mut buf = [0u8; PACKET_SIZE + 1];
        buf[1..].copy_from_slice(p);
        if self.debug {
            eprintln!("  → {}", hex(p));
        }
        self.hid.write(&buf).map(|_| ()).map_err(|e| format!("Запись в донгл: {e}"))
    }

    fn read(&mut self, timeout_ms: i32) -> Option<Vec<u8>> {
        let mut buf = [0u8; 64];
        let n = self.hid.read_timeout(&mut buf, timeout_ms.max(1)).ok()?;
        if n == 0 {
            return None;
        }
        let report = buf[..n].to_vec();
        if self.debug {
            eprintln!("  ← [{n}] {}", hex(&report));
        }
        if let Some(level) = level_of(&report) {
            self.stray_battery = Some(level);
        }
        Some(report)
    }

    /// Выкидывает накопившиеся отчёты, чтобы ответы не читались «со сдвигом».
    fn drain(&mut self) -> Vec<Vec<u8>> {
        let mut kept = Vec::new();
        for _ in 0..8 {
            match self.read(1) {
                Some(r) => kept.push(r),
                None => break,
            }
        }
        kept
    }

    /// Ждёт ответ, для которого `matches` вернёт true.
    fn wait(&mut self, timeout_ms: u64, matches: impl Fn(&[u8]) -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            let Some(r) = self.read(left.as_millis() as i32) else { return false };
            if matches(&r) {
                return true;
            }
        }
        false
    }

    /// Отправить и дождаться ответа с той же командой (Exchange в C#).
    fn exchange(&mut self, p: &[u8; PACKET_SIZE]) -> Result<(), String> {
        self.drain();
        self.write(p)?;
        let cmd = p[0];
        if !self.wait(500, |r| is_reply(r, cmd)) && self.debug {
            eprintln!("  (нет ответа на {cmd:02X})");
        }
        Ok(())
    }

    /// Заряд батареи, %. None — клавиатура не ответила.
    ///
    /// На железе видно: донгл сразу отвечает пустым эхо «20 01 00 00», а заряд спрашивает у клавиатуры
    /// по радио и отдаёт позже — отдельным отчётом или в ответ на следующий запрос (официальная программа
    /// спрашивает раз в 15 с). Поэтому запрашиваем несколько раз и ждём до `total_ms`.
    pub fn read_battery_for(&mut self, total_ms: u64) -> Result<Option<u8>, String> {
        // v21: раньше случайно пойманный старый заряд возвращался сразу, без нового запроса,
        // а из нескольких ответов брался первый (запомненный донглом). Из-за этого цифра
        // «зависала» (57 % при реальных 60 %). Теперь всегда спрашиваем заново и берём
        // ПОСЛЕДНИЙ пришедший заряд; старый — только если клавиатура не ответила.
        let mut seen: Vec<String> = Vec::new();
        let see = |seen: &mut Vec<String>, r: &[u8]| {
            if is_reply(r, 0x20) {
                let h = hex(&r[..r.len().min(12)]);
                if !seen.contains(&h) {
                    seen.push(h);
                }
            }
        };
        let mut stale = self.stray_battery.take();
        for r in self.drain() {
            see(&mut seen, &r);
            if let Some(l) = level_of(&r) {
                stale = Some(l);
            }
        }
        let mut fresh: Option<u8> = None;
        let deadline = Instant::now() + Duration::from_millis(total_ms);
        while fresh.is_none() && Instant::now() < deadline {
            self.write(&packet(&[0x20, 0x01]))?;
            // слушаем до 1,5 с после запроса; после первого заряда ждём ещё 400 мс — вдруг придёт свежее
            let mut round = (Instant::now() + Duration::from_millis(1500)).min(deadline);
            loop {
                let Some(left) = round.checked_duration_since(Instant::now()) else { break };
                let Some(r) = self.read(left.as_millis() as i32) else { break };
                see(&mut seen, &r);
                if let Some(l) = level_of(&r) {
                    if fresh.is_none() {
                        round = round.min(Instant::now() + Duration::from_millis(400));
                    }
                    fresh = Some(l);
                }
            }
        }
        self.stray_battery = None;
        let result = fresh.or(stale);
        let how = match (fresh, stale) {
            (Some(_), _) => "свежий",
            (None, Some(_)) => "из очереди",
            _ => "нет",
        };
        let answers = if seen.is_empty() { "ответов нет".to_string() } else { seen.join(" | ") };
        log_battery(&match result {
            Some(l) => format!("{l} % ({how}; {answers})"),
            None => format!("нет заряда в ответе: {answers}"),
        });
        Ok(result)
    }

    pub fn read_battery(&mut self) -> Result<Option<u8>, String> {
        self.read_battery_for(6000)
    }

    /// Состояние донгла (ответ на 0x02 целиком) — для диагностики.
    pub fn status(&mut self) -> Result<Option<Vec<u8>>, String> {
        self.drain();
        self.write(&packet(&[0x02]))?;
        let deadline = Instant::now() + Duration::from_millis(500);
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            let Some(r) = self.read(left.as_millis() as i32) else { break };
            if is_reply(&r, 0x02) {
                return Ok(Some(r));
            }
        }
        Ok(None)
    }

    /// Слушать донгл `seconds` секунд и печатать всё, что придёт (диагностика).
    pub fn listen(&mut self, seconds: u64) -> usize {
        let was = self.debug;
        self.debug = true;
        let deadline = Instant::now() + Duration::from_secs(seconds);
        let mut count = 0;
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            if self.read(left.as_millis().min(500) as i32).is_some() {
                count += 1;
            }
        }
        self.debug = was;
        count
    }

    /// Часы клавиатуры: два запроса состояния и пакет времени, как в официальной программе.
    pub fn sync_time(&mut self, now: DateTimeParts) -> Result<(), String> {
        self.exchange(&packet(&[0x02]))?;
        delay(20);
        self.exchange(&packet(&[0x20, 0x01]))?;
        delay(20);
        self.exchange(&time_packet(now))
    }

    /// Встроенный эффект по радио. При потере пакета повторяем, если донгл умеет подтверждать.
    #[allow(clippy::too_many_arguments)]
    pub fn send_light(&mut self, mode: i32, r: u8, g: u8, b: u8, rainbow: bool, brightness: i32, speed: i32, direction: i32) -> Result<(), String> {
        let p = light_packet(mode, r, g, b, rainbow, brightness, speed, direction);
        for _ in 0..3 {
            self.drain();
            self.write(&p)?;
            let timeout = if self.light_ack_seen { 250 } else { 150 };
            if self.wait(timeout, |r| is_reply(r, 0x05)) {
                self.light_ack_seen = true;
                return Ok(());
            }
            if !self.light_ack_seen {
                return Ok(()); // донгл не подтверждает — шлём один раз
            }
            delay(40);
        }
        Ok(())
    }

    /// Своя раскраска: режим 0x80, затем таблица цветов 21 порцией с подтверждением каждой.
    pub fn set_user_light(&mut self, frame: &[u8]) -> Result<(), String> {
        self.send_light(USER_LIGHT_MODE, 0, 0, 0, false, 5, 0, 0)?;
        delay(150);
        for i in 0..PARTS {
            let p = part_packet(frame, i);
            let mut ok = false;
            for _ in 0..3 {
                self.drain();
                self.write(&p)?;
                if self.wait(300, |r| bodies(r).iter().any(|b| b.len() > 2 && b[0] == 0x14 && b[2] == i as u8)) {
                    ok = true;
                    break;
                }
            }
            if !ok {
                return Err(format!("Донгл не подтвердил порцию раскраски {} из {PARTS}.", i + 1));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_is_sum_of_first_31() {
        let p = packet(&[0x20, 0x01]);
        assert_eq!(p[31], 0x21);
        let p = packet(&[0xFF, 0xFF]);
        assert_eq!(p[31], 0xFE);
    }

    #[test]
    fn time_layout() {
        let t = DateTimeParts { year: 2026, month: 11, day: 21, hour: 22, minute: 54, second: 7, weekday: 6 };
        let p = time_packet(t);
        assert_eq!(&p[..2], &[0x0C, 0x10]);
        assert_eq!(p[5], 0x5A);
        assert_eq!(&p[6..12], &[26, 11, 21, 22, 54, 7]);
        assert_eq!(p[13], 6);
        assert_eq!(&p[17..19], &[0xAA, 0x55]);
    }

    #[test]
    fn light_off_has_no_color() {
        let p = light_packet(0, 255, 0, 0, true, 5, 3, 0);
        assert_eq!(&p[3..15], &[0u8; 12]);
        let p = light_packet(4, 1, 2, 3, true, 9, 3, 1);
        assert_eq!(&p[3..7], &[4, 1, 2, 3]);
        assert_eq!(&p[11..15], &[1, 5, 3, 1]);
    }

    #[test]
    fn parts_cover_frame() {
        let frame: Vec<u8> = (0..576).map(|i| (i % 251) as u8).collect();
        let last = part_packet(&frame, 20);
        assert_eq!(last[2], 20);
        assert_eq!(last[3], frame[560]);
        assert_eq!(last[3 + 15], frame[575]);
        assert_eq!(last[3 + 16], 0); // хвост нулей
    }

    #[test]
    fn battery_with_and_without_report_id() {
        assert_eq!(level_of(&[0x20, 0x01, 0x00, 77]), Some(77));
        let mut with_id = vec![0u8; 33];
        with_id[1..5].copy_from_slice(&[0x20, 0x01, 0x00, 150]);
        assert_eq!(level_of(&with_id), Some(100));
        assert_eq!(level_of(&[0x20, 0x01, 0x00, 0]), None);
        assert_eq!(level_of(&[0x06, 0x20, 0x01, 0x00, 55]), Some(55));
        assert!(is_reply(&[0x06, 0x0C, 0x10], 0x0C));
    }

    #[test]
    fn status_from_hardware() {
        let r = [0x02, 0, 0, 0x40, 0x30, 0, 0, 0x45, 0x0C, 0x09, 0x80, 0x14, 0x01, 0xFF, 0xFF];
        let st = parse_status(&r).unwrap();
        assert_eq!((st.vendor_id, st.product_id), (0x0C45, 0x8009));
    }
}
