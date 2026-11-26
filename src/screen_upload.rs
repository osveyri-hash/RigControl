//! Заливка картинки/гифки на экранчик AK820 Pro в фоне (для окна): окно не зависает на несколько
//! секунд и показывает прогресс по порциям, как UploadScreen в C#.
//! Экран заливается только по кабелю (интерфейс данных 0xFF68): донгл 2,4 ГГц его не отдаёт.
//!
//! Заливку выполняет поток клавиатуры ([`crate::kb_light::KbLight::upload`]) — тот же, что ведёт
//! подсветку: сессия заливки (04 18 … 04 02) не должна перемешиваться с пакетами подсветки,
//! иначе чужой 04 02 обрывает её и на экране остаётся мусор. В C# перед заливкой останавливается
//! StartLightSync, а устройство сериализует сессии блокировкой.

use crate::device::Ak820Device;
use crate::protocol::{self, ScreenFrame};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Default, PartialEq)]
pub enum UploadState {
    #[default]
    Idle,
    Running { done: usize, total: usize },
    Done(String),
    Failed(String),
}

/// Ход заливки: окно читает, поток клавиатуры пишет.
#[derive(Clone)]
pub struct ScreenUpload {
    state: Arc<Mutex<UploadState>>,
}

impl ScreenUpload {
    /// Заливка поставлена в очередь потока клавиатуры.
    pub(crate) fn queued() -> Self {
        Self { state: Arc::new(Mutex::new(UploadState::Running { done: 0, total: 0 })) }
    }

    pub(crate) fn set(&self, v: UploadState) {
        *self.state.lock().unwrap_or_else(|e| e.into_inner()) = v;
    }

    pub fn state(&self) -> UploadState {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn running(&self) -> bool {
        matches!(self.state(), UploadState::Running { .. })
    }
}

/// Залить кадры в открытую по кабелю клавиатуру. Ok — строка итога для окна.
pub(crate) fn send(dev: &Ak820Device, frames: &[ScreenFrame], progress: &ScreenUpload) -> Result<String, String> {
    let count = frames.len().min(protocol::MAX_FRAMES);
    let blob = protocol::screen_blob(frames).map_err(|e| e.to_string())?;
    let stats = dev.upload_screen(&blob, |done, total| progress.set(UploadState::Running { done, total }))?;
    let mut msg = format!(
        "Готово: кадров {count}, {} КиБ за {:.1} с (ACK {}/{}).",
        blob.len() / 1024, stats.total_seconds, stats.acks, stats.chunks
    );
    if stats.acks == 0 {
        msg.push_str(" Клавиатура не прислала ни одного ACK — если картинка не сменилась, пришлите это сообщение.");
    }
    Ok(msg)
}

/// RGB565 little-endian → RGB888 (для предпросмотра в окне).
pub fn rgb565_to_rgb(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() / 2 * 3);
    for px in data.chunks_exact(2) {
        let v = u16::from_le_bytes([px[0], px[1]]);
        let r = ((v >> 11) & 0x1F) as u8;
        let g = ((v >> 5) & 0x3F) as u8;
        let b = (v & 0x1F) as u8;
        out.push((r << 3) | (r >> 2));
        out.push((g << 2) | (g >> 4));
        out.push((b << 3) | (b >> 2));
    }
    out
}

/// Какой кадр показывать в момент t_ms при проигрывании по кругу.
pub fn frame_at(frames: &[ScreenFrame], t_ms: u64) -> usize {
    let total: u64 = frames.iter().map(|f| f.delay_ms.max(20) as u64).sum();
    if frames.len() <= 1 || total == 0 {
        return 0;
    }
    let mut t = t_ms % total;
    for (i, f) in frames.iter().enumerate() {
        let d = f.delay_ms.max(20) as u64;
        if t < d {
            return i;
        }
        t -= d;
    }
    frames.len() - 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb565_back_to_rgb() {
        let img = image::RgbImage::from_pixel(1, 1, image::Rgb([0xFF, 0x00, 0xFF]));
        assert_eq!(rgb565_to_rgb(&crate::screen::to_rgb565(&img)), vec![0xFF, 0x00, 0xFF]);
    }

    #[test]
    fn frame_timing_loops() {
        let f = |d| ScreenFrame { rgb565: vec![], delay_ms: d };
        let frames = vec![f(100), f(200), f(100)];
        assert_eq!(frame_at(&frames, 0), 0);
        assert_eq!(frame_at(&frames, 150), 1);
        assert_eq!(frame_at(&frames, 350), 2);
        assert_eq!(frame_at(&frames, 400), 0);
    }
}
