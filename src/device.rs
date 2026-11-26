//! Порт Ak820Device.cs + нужной части HidInterface.cs: подключение к клавиатуре
//! через два вендорских HID-интерфейса (0xFF13 — команды, 0xFF68 — данные) и сами команды.
//! Вместо P/Invoke к hid.dll используется крейт hidapi.

use crate::keypaint::USER_LIGHT_MODE;
use crate::protocol::{self, DateTimeParts, CHUNK, COMMAND_DELAY_MS, CONTROL_USAGE_PAGE, DATA_USAGE_PAGE, LIVE_FRAME_SIZE, PRODUCT_IDS, VENDOR_ID};
use hidapi::{HidApi, HidDevice};
use std::thread::sleep;
use std::time::{Duration, Instant};

/// Итоги заливки экрана: сколько порций, сколько ACK и куда ушло время (UploadStats в C#).
#[derive(Debug, Clone, Copy, Default)]
pub struct UploadStats {
    pub chunks: usize,
    pub acks: usize,
    pub write_seconds: f64,
    pub ack_seconds: f64,
    pub total_seconds: f64,
}

pub struct Ak820Device {
    control: HidDevice,
    data: HidDevice,
    pub description: String,
    /// Статус последнего заголовка живого кадра: байт 3 ответа (1 — принято), −1 — ответа не было.
    pub last_live_status: i32,
}

fn delay(ms: u64) {
    sleep(Duration::from_millis(ms));
}

/// Список вендорских HID-интерфейсов (usage page ≥ 0xFF00) — для сообщения об ошибке.
pub fn describe_vendor_interfaces(api: &HidApi) -> Vec<String> {
    api.device_list()
        .filter(|d| d.usage_page() >= 0xFF00)
        .map(|d| format!("{:04X}:{:04X} 0x{:04X}", d.vendor_id(), d.product_id(), d.usage_page()))
        .collect()
}

impl Ak820Device {
    /// Ищет клавиатуру: по кабелю или через донгл. Нужны оба интерфейса Sonix (VID 0C45): 0xFF13 и 0xFF68.
    pub fn open(api: &HidApi) -> Result<Self, String> {
        let interfaces: Vec<_> = api.device_list().filter(|d| d.vendor_id() == VENDOR_ID).collect();
        let control = interfaces.iter().find(|d| d.usage_page() == CONTROL_USAGE_PAGE);
        let data = interfaces.iter().find(|d| d.usage_page() == DATA_USAGE_PAGE);

        let (Some(control), Some(data)) = (control, data) else {
            let seen = describe_vendor_interfaces(api);
            let list = if seen.is_empty() { "вендорских HID-интерфейсов в системе нет".to_string() } else { seen.join("; ") };
            let dongle = !interfaces.is_empty() && interfaces.iter().all(|i| !PRODUCT_IDS.contains(&i.product_id()));
            let mut error = if interfaces.is_empty() {
                "Клавиатура не найдена: подключите кабель USB-C или включите режим 2,4 ГГц с донглом.".to_string()
            } else if dongle {
                "Найден донгл 2,4 ГГц, но он не отдаёт интерфейсы команд (0xFF13/0xFF68), по радио подсветкой и экраном управлять нельзя — подключите кабель USB-C. Настройки хранятся в клавиатуре и останутся после возврата на 2,4 ГГц.".to_string()
            } else {
                "Найдена клавиатура, но интерфейсы команд (0xFF13/0xFF68) не видны — проверьте проводный режим.".to_string()
            };
            error += &format!(" Вендорские интерфейсы: {list}.");
            return Err(error);
        };

        let description = format!("AK820 Pro  {:04X}:{:04X}", control.vendor_id(), control.product_id());
        let control = api.open_path(control.path()).map_err(|e| format!("Не открыть интерфейс 0xFF13: {e}"))?;
        let data = api.open_path(data.path()).map_err(|e| format!("Не открыть интерфейс 0xFF68: {e}"))?;
        Ok(Self { control, data, description, last_live_status: -1 })
    }

    // ── HID (из HidInterface.cs) ─────────────────────────────────────────

    /// Feature-отчёт: 64 байта данных; Report ID = 0 идёт первым байтом (всего 65).
    fn set_feature(&self, data: &[u8]) -> Result<(), String> {
        let mut buf = [0u8; 65];
        let n = data.len().min(64);
        buf[1..1 + n].copy_from_slice(&data[..n]);
        self.control.send_feature_report(&buf).map_err(|e| format!("HidD_SetFeature: {e}"))
    }

    /// Чтение feature-отчёта (короткий ACK). Возвращает 64 байта без Report ID.
    fn get_feature(&self) -> Result<[u8; 64], String> {
        let mut buf = [0u8; 65];
        self.control.get_feature_report(&mut buf).map_err(|e| format!("HidD_GetFeature: {e}"))?;
        let mut out = [0u8; 64];
        out.copy_from_slice(&buf[1..]);
        Ok(out)
    }

    fn try_read_feature(&self) {
        let _ = self.get_feature(); // ответ необязателен
    }

    // ── Команды ──────────────────────────────────────────────────────────

    /// Команда с паузой 35 мс до и после.
    fn command(&self, op: u8, count: i32, extra0: u8) -> Result<(), String> {
        delay(COMMAND_DELAY_MS);
        self.set_feature(&protocol::command(op, count, extra0))?;
        delay(COMMAND_DELAY_MS);
        self.try_read_feature();
        Ok(())
    }

    /// Финальный 04 F0: официальный конфигуратор шлёт его после смены подсветки.
    fn finish(&self) {
        delay(COMMAND_DELAY_MS);
        let _ = self.set_feature(&protocol::command(0xF0, 0, 0));
    }

    /// Встроенный эффект подсветки (сохраняется в клавиатуре). rainbow=true — цвет не используется.
    pub fn set_effect(&self, mode: i32, color: &str, rainbow: bool, brightness: i32, speed: i32, direction: i32) -> Result<(), String> {
        let (r, g, b) = protocol::parse_color(Some(color), (255, 255, 255));
        self.command(0x18, 0, 0)?;
        self.command(0x13, 1, 0)?;
        delay(COMMAND_DELAY_MS);
        self.set_feature(&protocol::light_packet(mode, r, g, b, rainbow, brightness, speed, direction))?;
        self.command(0x02, 0, 0)?;
        self.finish();
        Ok(())
    }

    /// Быстрая смена цвета/эффекта без сессии и без записи во флеш (SetEffectFast в C#):
    /// для подсветки в такт гифке — несколько новых цветов в секунду, сохранять их в память нельзя.
    #[allow(clippy::too_many_arguments)]
    pub fn set_effect_fast(&self, mode: i32, r: u8, g: u8, b: u8, rainbow: bool, brightness: i32, speed: i32, direction: i32) -> Result<(), String> {
        self.set_feature(&protocol::light_packet(mode, r, g, b, rainbow, brightness, speed, direction))?;
        self.try_read_feature();
        crate::light_mirror::effect(mode, r, g, b, rainbow, brightness, speed, direction, false);
        Ok(())
    }

    /// Свои цвета по клавишам: команда 0x23 с count=9, затем 9 пакетов по 64 байта.
    pub fn set_key_colors(&self, frame: &[u8], commit: bool, start: bool) -> Result<(), String> {
        if frame.len() < 9 * 64 {
            return Err("Таблица цветов короче 576 байт".into());
        }
        if start {
            self.command(0x18, 0, 0)?;
        }
        self.command(0x23, 9, 0)?;
        for i in 0..9 {
            self.set_feature(&frame[i * 64..i * 64 + 64])?;
            self.try_read_feature();
        }
        if commit {
            self.command(0x02, 0, 0)?;
        }
        Ok(())
    }

    /// Своя раскраска: режим 0x80, затем таблица цветов. Прошивка хранит её сама.
    pub fn set_user_light(&self, frame: &[u8]) -> Result<(), String> {
        self.set_effect(USER_LIGHT_MODE, "#000000", false, 5, 0, 0)?;
        self.set_key_colors(frame, true, true)?;
        self.finish();
        Ok(())
    }

    /// Синхронизация часов на экране клавиатуры.
    pub fn sync_time(&self, now: DateTimeParts) -> Result<(), String> {
        self.command(0x18, 0, 0)?;
        self.command(0x28, 1, 0)?;
        delay(COMMAND_DELAY_MS);
        self.set_feature(&protocol::time_packet(now, 1))?;
        self.try_read_feature();
        self.command(0x02, 0, 0)?;
        Ok(())
    }

    /// Живой кадр (как страница «Музыка» официальной программы): 04 20, 8 пакетов, 04 02.
    /// Без записи во флеш — можно слать 20 раз в секунду. false — клавиатура дважды ответила «занята».
    pub fn send_live_frame(&mut self, live: &[u8]) -> Result<bool, String> {
        if live.len() < LIVE_FRAME_SIZE {
            return Err("Живой кадр короче 512 байт".into());
        }
        if self.live_command(&protocol::command(0x20, (LIVE_FRAME_SIZE / 64) as i32, 0))? == 0xFF {
            return Ok(false);
        }
        for i in 0..LIVE_FRAME_SIZE / 64 {
            self.set_feature(&live[i * 64..i * 64 + 64])?;
            delay(2);
        }
        self.live_command(&protocol::command(0x02, 0, 0))?;
        crate::light_mirror::live(&live[..LIVE_FRAME_SIZE]);
        Ok(true)
    }

    /// Заголовок живого кадра: 5 мс, запись, 5 мс, ответ. 0xFF — «занята», повторяем один раз.
    fn live_command(&mut self, packet: &[u8]) -> Result<i32, String> {
        let mut status = -1;
        for _ in 0..2 {
            delay(5);
            self.set_feature(packet)?;
            delay(5);
            status = self.get_feature().map(|b| b[3] as i32).unwrap_or(-1);
            if status != 0xFF {
                break;
            }
        }
        self.last_live_status = status;
        Ok(status)
    }

    /// Заливка картинки/гифки на экран (UploadScreen в C#): 04 18, 04 72 (count = число порций, extra0 = 1),
    /// затем порции по 4 КиБ в интерфейс данных 0xFF68. После каждой порции читаем ACK —
    /// иначе прошивка не дописывает данные во флеш. Сессию закрываем 04 02 даже при ошибке.
    pub fn upload_screen(&self, blob: &[u8], mut progress: impl FnMut(usize, usize)) -> Result<UploadStats, String> {
        if blob.is_empty() || blob.len() % CHUNK != 0 {
            return Err(format!("Полотно экрана должно быть кратно {CHUNK} байт, а оно {} байт", blob.len()));
        }
        let clock = Instant::now();
        let total = blob.len() / CHUNK;
        let mut stats = UploadStats { chunks: total, ..Default::default() };
        let mut ack_timeout_ms: i32 = 300;

        self.command(0x18, 0, 0)?;
        self.command(0x72, total as i32, 0x01)?;

        let mut report = vec![0u8; CHUNK + 1]; // Report ID 0 + 4096 байт
        let mut ack = vec![0u8; 4096];
        let mut result: Result<(), String> = Ok(());
        for i in 0..total {
            let t0 = Instant::now();
            report[0] = 0;
            report[1..].copy_from_slice(&blob[i * CHUNK..(i + 1) * CHUNK]);
            match self.data.write(&report) {
                Ok(0) => {
                    result = Err(format!("Порция {}: записано 0 байт", i + 1));
                    break;
                }
                Ok(_) => {}
                Err(e) => {
                    result = Err(format!("Порция {} из {total}: {e}", i + 1));
                    break;
                }
            }
            let t1 = Instant::now();
            // ACK бывает пустым — не повод рвать заливку. Нет ответа — дальше ждём 20 мс, а не 300.
            let got = self.data.read_timeout(&mut ack, ack_timeout_ms).unwrap_or(0);
            ack_timeout_ms = if got > 0 { 300 } else { 20 };
            if got > 0 {
                stats.acks += 1;
            }
            stats.write_seconds += (t1 - t0).as_secs_f64();
            stats.ack_seconds += t1.elapsed().as_secs_f64();
            progress(i + 1, total);
        }

        // Закрываем сессию в любом случае, чтобы клавиатура не осталась в режиме приёма.
        let _ = self.command(0x02, 0, 0);
        result?;
        stats.total_seconds = clock.elapsed().as_secs_f64();
        Ok(stats)
    }

}
