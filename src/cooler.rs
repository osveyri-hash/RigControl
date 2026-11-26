//! Порт Cooler/CoolerLcd.cs (+ нужная часть LcdRender.cs): экран на помпе Thermalright
//! (Frozen Warframe 360 и родственные), HID «Type 2», USB 0416:5302.
//! Протокол: рукопожатие с магией DA DB DC DD, затем кадры — 20-байтный заголовок + JPEG
//! (или сырой RGB565 у PM 58), добитые нулями до размера отчёта.
//! Версии с USB-ID 0402:3922 (SCSI) не поддерживаются — как и в C#.

use hidapi::{HidApi, HidDevice};
use image::imageops::{self, FilterType};
use image::{AnimationDecoder, DynamicImage, RgbImage, RgbaImage};
use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

pub const VENDOR_ID: u16 = 0x0416;
pub const PRODUCT_ID: u16 = 0x5302;
pub const MAGIC: [u8; 4] = [0xDA, 0xDB, 0xDC, 0xDD];
/// Размер порции по умолчанию (C#: OutputReportLength − 1, если устройство не объявило > 64 — 512).
pub const DEFAULT_CHUNK: usize = 512;
/// Сколько кадров гифки максимум берём (как прореживание в C#).
pub const MAX_FRAMES: usize = 300;

/// Один кадр для экрана помпы: готовая картинка и задержка показа.
pub struct LcdFrame {
    pub image: RgbImage,
    pub delay_ms: u32,
}

pub struct CoolerLcd {
    hid: HidDevice,
    chunk: usize,
    pub pm: u8,
    pub sub: u8,
    pub serial: String,
    pub handshake_ok: bool,
    /// Разрешение панели по ответу рукопожатия (по умолчанию 320×320).
    pub resolution: (u32, u32),
    pub frames_sent: u64,
    pub handshakes: u32,
    pub errors: u32,
    pub last_error: String,
    interface_info: String,
    handshake_info: String,
}

/// Кадр Type 2: заголовок 20 байт + данные, добитые нулями до кратного chunk.
/// mode: 0 — JPEG, 1 — RGB565.
pub fn frame_packet(payload: &[u8], width: u16, height: u16, mode: u8, chunk: usize) -> Vec<u8> {
    let mut header = [0u8; 20];
    header[..4].copy_from_slice(&MAGIC);
    header[4] = 0x02; // команда «картинка»
    header[6] = mode;
    header[8..10].copy_from_slice(&width.to_le_bytes());
    header[10..12].copy_from_slice(&height.to_le_bytes());
    header[12] = 0x02;
    header[16..20].copy_from_slice(&(payload.len() as u32).to_le_bytes());

    let total = header.len() + payload.len();
    let mut padded = vec![0u8; total.div_ceil(chunk) * chunk];
    padded[..20].copy_from_slice(&header);
    padded[20..total].copy_from_slice(payload);
    padded
}

/// Картинка → сырой RGB565, младший байт первым.
pub fn to_rgb565(image: &RgbImage) -> Vec<u8> {
    let mut data = Vec::with_capacity(image.width() as usize * image.height() as usize * 2);
    for p in image.pixels() {
        let [r, g, b] = p.0;
        let value: u16 = ((r as u16 >> 3) << 11) | ((g as u16 >> 2) << 5) | (b as u16 >> 3);
        data.push((value & 0xFF) as u8);
        data.push((value >> 8) as u8);
    }
    data
}

/// Разрешение панели по PM.
pub fn resolution_for_pm(pm: u8) -> (u32, u32) {
    match pm {
        5 | 50 | 58 => (320, 240),
        7 => (640, 480),
        _ => (320, 320),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

impl CoolerLcd {
    /// Ищет помпу и делает рукопожатие. chunk — размер порции (None — 512).
    pub fn open(api: &HidApi, chunk: Option<usize>) -> Result<Self, String> {
        let list: Vec<_> = api.device_list().filter(|d| d.vendor_id() == VENDOR_ID && d.product_id() == PRODUCT_ID).collect();
        if list.is_empty() {
            return Err("Экран помпы (USB 0416:5302) не найден. Проверьте внутренний USB-кабель помпы и закройте \
                        программу производителя (TRCC) — она занимает устройство. Если у вас модель с ID 0402:3922 \
                        (SCSI), эта версия её не поддерживает."
                .into());
        }
        let mut info = String::new();
        for d in &list {
            info += &format!("HID-интерфейс: usage page 0x{:04X}, interface {}\n", d.usage_page(), d.interface_number());
        }
        // C# берёт интерфейс с самыми большими отчётами; hidapi их размер не отдаёт —
        // берём вендорский (usage page ≥ 0xFF00), иначе первый.
        let main = list.iter().copied().find(|d| d.usage_page() >= 0xFF00).unwrap_or(list[0]);
        let hid = api.open_path(main.path()).map_err(|e| format!("Не открыть экран помпы: {e}"))?;

        let mut lcd = Self {
            hid,
            chunk: chunk.unwrap_or(DEFAULT_CHUNK).max(64),
            pm: 0,
            sub: 0,
            serial: String::new(),
            handshake_ok: false,
            resolution: (320, 320),
            frames_sent: 0,
            handshakes: 0,
            errors: 0,
            last_error: String::new(),
            interface_info: info,
            handshake_info: String::new(),
        };
        lcd.handshake(1500);
        Ok(lcd)
    }

    pub fn use_rgb565(&self) -> bool {
        self.pm == 58
    }

    pub fn description(&self) -> String {
        let mut s = format!("USB {VENDOR_ID:04X}:{PRODUCT_ID:04X} · PM {} · SUB {}", self.pm, self.sub);
        if !self.serial.is_empty() {
            s += &format!(" · {}", self.serial);
        }
        s
    }

    pub fn diagnostics(&self) -> String {
        let mut text = format!("{}{}Кадров отправлено: {}, рукопожатий: {}\n", self.interface_info, self.handshake_info, self.frames_sent, self.handshakes);
        if self.errors > 0 {
            text += &format!("Ошибок передачи: {}, последняя: {}\n", self.errors, self.last_error);
        }
        text
    }

    /// Output-отчёт: Report ID 0 + данные.
    fn write_output(&self, data: &[u8]) -> Result<(), String> {
        let mut buf = Vec::with_capacity(data.len() + 1);
        buf.push(0);
        buf.extend_from_slice(data);
        self.hid.write(&buf).map(|_| ()).map_err(|e| format!("запись в помпу: {e}"))
    }

    /// Рукопожатие. true — помпа ответила по протоколу Type 2.
    pub fn handshake(&mut self, timeout_ms: i32) -> bool {
        self.handshakes += 1;
        self.handshake_ok = false;
        self.handshake_info.clear();

        let mut init = vec![0u8; self.chunk];
        init[..4].copy_from_slice(&MAGIC);
        init[12] = 0x01;
        if let Err(e) = self.write_output(&init) {
            self.handshake_info = format!("Рукопожатие: ошибка записи — {e}\n");
            return false;
        }

        let mut response = vec![0u8; self.chunk + 1];
        let n = match self.hid.read_timeout(&mut response, timeout_ms) {
            Ok(n) if n > 0 => n,
            Ok(_) => {
                self.handshake_info = "Рукопожатие: ответа нет (таймаут 1,5 с)\n".into();
                return false;
            }
            Err(e) => {
                self.handshake_info = format!("Рукопожатие: ошибка чтения — {e}\n");
                return false;
            }
        };
        response.truncate(n);
        self.handshake_info = format!("Рукопожатие: ответ {} байт: {}\n", n, hex(&response[..n.min(48)]));

        // Первым байтом может прийти Report ID — магию ищем со сдвигом 0 или 1.
        let offset = (0..=1usize).find(|&i| i + 6 <= response.len() && response[i..i + 4] == MAGIC);
        let Some(o) = offset else {
            self.handshake_info += "Магия DA DB DC DD в ответе не найдена — это не протокол Type 2.\n";
            return false;
        };

        self.handshake_ok = true;
        self.sub = response[o + 4];
        self.pm = response[o + 5];
        if response.len() >= o + 36 && response[o + 16] == 0x10 {
            self.serial = hex(&response[o + 20..o + 36]);
        }
        self.resolution = resolution_for_pm(self.pm);
        true
    }

    /// Шлёт кадр порциями. При сбое один раз повторяет рукопожатие и кадр целиком.
    fn write_frame(&mut self, padded: &[u8]) -> Result<(), String> {
        for attempt in 0..2 {
            let mut result = Ok(());
            for (index, part) in padded.chunks(self.chunk).enumerate() {
                if let Err(e) = self.write_output(part) {
                    result = Err(e);
                    break;
                }
                if (index + 1) % 128 == 0 {
                    sleep(Duration::from_millis(1));
                }
            }
            match result {
                Ok(()) => return Ok(()),
                Err(e) if attempt == 0 => {
                    self.errors += 1;
                    self.last_error = e;
                    self.handshake(1500);
                }
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    /// Готовый JPEG на экран.
    pub fn send_jpeg(&mut self, jpeg: &[u8], width: u32, height: u32) -> Result<(), String> {
        let padded = frame_packet(jpeg, width as u16, height as u16, 0x00, self.chunk);
        self.write_frame(&padded)?;
        self.frames_sent += 1;
        Ok(())
    }

    /// Картинка на экран: RGB565 для PM 58, иначе JPEG (качество 85, как в C#).
    pub fn send_image(&mut self, image: &RgbImage) -> Result<(), String> {
        if self.use_rgb565() {
            // PM58 ждёт 320×240, повёрнутую на 90° по часовой; в заголовке 240×320; пауза 30 мс.
            let normalized = if image.dimensions() == (320, 240) {
                image.clone()
            } else {
                DynamicImage::ImageRgb8(image.clone()).resize_to_fill(320, 240, FilterType::Triangle).to_rgb8()
            };
            let data = to_rgb565(&imageops::rotate90(&normalized));
            let padded = frame_packet(&data, 240, 320, 0x01, self.chunk);
            self.write_frame(&padded)?;
            sleep(Duration::from_millis(30));
            self.frames_sent += 1;
            return Ok(());
        }
        let jpeg = encode_jpeg(image, 85)?;
        self.send_jpeg(&jpeg, image.width(), image.height())
    }
}

pub fn encode_jpeg(image: &RgbImage, quality: u8) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
        .encode_image(image)
        .map_err(|e| format!("JPEG: {e}"))?;
    Ok(out)
}

/// RGBA на чёрном фоне → RGB (как рисование поверх чёрного прямоугольника в C#).
fn flatten(rgba: &RgbaImage) -> RgbImage {
    RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let [r, g, b, a] = rgba.get_pixel(x, y).0;
        let k = a as u32;
        image::Rgb([(r as u32 * k / 255) as u8, (g as u32 * k / 255) as u8, (b as u32 * k / 255) as u8])
    })
}

/// Вписать кадр в width×height: crop — заполнить с обрезкой, иначе целиком с чёрными полями.
pub(crate) fn compose(frame: &DynamicImage, width: u32, height: u32, crop: bool) -> RgbImage {
    if crop {
        return flatten(&frame.resize_to_fill(width, height, FilterType::Triangle).to_rgba8());
    }
    let fitted = flatten(&frame.resize(width, height, FilterType::Triangle).to_rgba8());
    let mut canvas = RgbImage::new(width, height);
    let x = (width as i64 - fitted.width() as i64) / 2;
    let y = (height as i64 - fitted.height() as i64) / 2;
    imageops::overlay(&mut canvas, &fitted, x, y);
    canvas
}

fn rotate(image: RgbImage, rotation: u32) -> RgbImage {
    match rotation {
        90 => imageops::rotate90(&image),
        180 => imageops::rotate180(&image),
        270 => imageops::rotate270(&image),
        _ => image,
    }
}

/// Картинка или гифка → кадры под экран (с поворотом). Порт LcdRender.LoadImage.
pub fn load_image(path: &Path, width: u32, height: u32, rotation: u32, crop: bool) -> Result<Vec<LcdFrame>, String> {
    // Встроенная картинка темы (pack://…/Assets/Имя) — из программы, а не с диска.
    if let Some(name) = crate::pump_art::resource_name(&path.to_string_lossy()) {
        let bytes = crate::pump_art::bytes(name).ok_or_else(|| format!("нет встроенной картинки {name}"))?;
        let gif = name.to_ascii_lowercase().ends_with(".gif");
        return load_bytes(bytes, gif, width, height, rotation, crop);
    }
    let gif = path.extension().map(|e| e.eq_ignore_ascii_case("gif")).unwrap_or(false);
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    load_bytes(&bytes, gif, width, height, rotation, crop).map_err(|e| format!("{}: {e}", path.display()))
}

/// Задержка кадра гифки, как GifFrames.Delay в C# (и как браузеры): 0 или 10 мс в файле — это
/// «не задано», такие кадры идут по 100 мс. Иначе гифки без задержек (все встроенные аниме-темы)
/// крутились бы на 50 кадр/с — быстрее, чем помпа успевает показывать.
pub fn gif_delay_ms(delay: image::Delay) -> u32 {
    let (n, d) = delay.numer_denom_ms();
    let ms = if d == 0 { 0 } else { n / d };
    if ms <= 10 { 100 } else { ms }
}

/// Картинка или гифка из памяти → кадры под экран width×height.
pub fn load_bytes(bytes: &[u8], gif: bool, width: u32, height: u32, rotation: u32, crop: bool) -> Result<Vec<LcdFrame>, String> {
    let swap = matches!(rotation, 90 | 270);
    let (draw_w, draw_h) = if swap { (height, width) } else { (width, height) };
    if gif {
        let decoder = image::codecs::gif::GifDecoder::new(std::io::Cursor::new(bytes)).map_err(|e| format!("GIF: {e}"))?;
        let frames = decoder.into_frames().collect_frames().map_err(|e| format!("GIF: {e}"))?;
        if frames.is_empty() {
            return Err("В файле нет ни одного кадра".into());
        }
        return Ok(frames
            .into_iter()
            .take(MAX_FRAMES)
            .map(|f| {
                let delay = gif_delay_ms(f.delay());
                let img = DynamicImage::ImageRgba8(f.into_buffer());
                LcdFrame { image: rotate(compose(&img, draw_w, draw_h, crop), rotation), delay_ms: delay.max(20) }
            })
            .collect());
    }
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    Ok(vec![LcdFrame { image: rotate(compose(&img, draw_w, draw_h, crop), rotation), delay_ms: 1000 }])
}

/// Однотонный кадр (для проверки экрана).
pub fn solid(width: u32, height: u32, color: crate::Rgb) -> RgbImage {
    RgbImage::from_pixel(width, height, image::Rgb([color.0, color.1, color.2]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_header_and_padding() {
        let p = frame_packet(&[1, 2, 3], 320, 240, 0, 512);
        assert_eq!(p.len(), 512);
        assert_eq!(&p[..4], &MAGIC);
        assert_eq!((p[4], p[6], p[12]), (2, 0, 2));
        assert_eq!(u16::from_le_bytes([p[8], p[9]]), 320);
        assert_eq!(u16::from_le_bytes([p[10], p[11]]), 240);
        assert_eq!(u32::from_le_bytes([p[16], p[17], p[18], p[19]]), 3);
        assert_eq!(&p[20..23], &[1, 2, 3]);
        assert_eq!(frame_packet(&vec![0; 492], 1, 1, 0, 512).len(), 512);
        assert_eq!(frame_packet(&vec![0; 493], 1, 1, 0, 512).len(), 1024);
    }

    #[test]
    fn rgb565_little_endian() {
        let img = RgbImage::from_vec(2, 1, vec![255, 0, 0, 255, 255, 255]).unwrap();
        assert_eq!(to_rgb565(&img), vec![0x00, 0xF8, 0xFF, 0xFF]);
    }

    #[test]
    fn pm_resolution() {
        assert_eq!(resolution_for_pm(58), (320, 240));
        assert_eq!(resolution_for_pm(7), (640, 480));
        assert_eq!(resolution_for_pm(1), (320, 320));
    }
}
