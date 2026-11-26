//! Порт ImageFrames.cs (+ GifFrames.cs): картинка или гифка → кадры 128×128 RGB565
//! для экранчика AK820 Pro. Сборку кадров гифки (смещения, disposal) делает крейт image.

use crate::cooler;
use crate::protocol::{ScreenFrame, MAX_FRAMES, SCREEN_SIZE};
use image::{AnimationDecoder, DynamicImage, RgbImage};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

const SIZE: u32 = SCREEN_SIZE as u32;

/// RGB → RGB565 little-endian (ToRgb565 в C#).
pub fn to_rgb565(image: &RgbImage) -> Vec<u8> {
    let mut out = Vec::with_capacity(image.width() as usize * image.height() as usize * 2);
    for p in image.pixels() {
        let [r, g, b] = p.0;
        let v: u16 = (((r as u16) & 0xF8) << 8) | (((g as u16) & 0xFC) << 3) | ((b as u16) >> 3);
        out.push((v & 0xFF) as u8);
        out.push((v >> 8) as u8);
    }
    out
}

/// Оставляет не больше max кадров равномерно по времени: каждый оставшийся держится
/// столько, сколько вся группа, которую он заменил (Decimate в C#).
pub fn decimate(frames: Vec<ScreenFrame>, max: usize) -> Vec<ScreenFrame> {
    if max == 0 || frames.len() <= max {
        return frames;
    }
    let n = frames.len();
    (0..max)
        .map(|i| {
            let start = i * n / max;
            let end = (i + 1) * n / max;
            let delay = frames[start..end].iter().map(|f| f.delay_ms).sum();
            ScreenFrame { rgb565: frames[start].rgb565.clone(), delay_ms: delay }
        })
        .collect()
}

/// Загрузка картинки/гифки. crop — заполнить экран с обрезкой, иначе вписать с чёрными полями.
/// max_frames — 0 = все (но не больше 140, ограничение прошивки).
pub fn load(path: &Path, crop: bool, max_frames: usize) -> Result<Vec<ScreenFrame>, String> {
    let is_gif = path.extension().map(|e| e.eq_ignore_ascii_case("gif")).unwrap_or(false);
    let mut frames = Vec::new();
    if is_gif {
        let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let decoder = image::codecs::gif::GifDecoder::new(BufReader::new(file)).map_err(|e| format!("GIF: {e}"))?;
        for f in decoder.into_frames().take(MAX_FRAMES) {
            let f = f.map_err(|e| format!("GIF: {e}"))?;
            let delay = cooler::gif_delay_ms(f.delay()) as i32;
            let img = DynamicImage::ImageRgba8(f.into_buffer());
            frames.push(ScreenFrame { rgb565: to_rgb565(&cooler::compose(&img, SIZE, SIZE, crop)), delay_ms: delay.max(20) });
        }
    } else {
        let img = image::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        frames.push(ScreenFrame { rgb565: to_rgb565(&cooler::compose(&img, SIZE, SIZE, crop)), delay_ms: 1000 });
    }
    if frames.is_empty() {
        return Err("В файле нет ни одного кадра".into());
    }
    let max = if max_frames == 0 { MAX_FRAMES } else { max_frames.min(MAX_FRAMES) };
    Ok(decimate(frames, max))
}

/// Однотонный кадр.
pub fn solid(color: crate::Rgb) -> ScreenFrame {
    let img = RgbImage::from_pixel(SIZE, SIZE, image::Rgb([color.0, color.1, color.2]));
    ScreenFrame { rgb565: to_rgb565(&img), delay_ms: 1000 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb565_matches_csharp() {
        let img = RgbImage::from_pixel(1, 1, image::Rgb([0xFF, 0x80, 0x10]));
        // r&F8<<8 = F800, g&FC<<3 = 0400, b>>3 = 02 → 0xFC02
        assert_eq!(to_rgb565(&img), vec![0x02, 0xFC]);
    }

    #[test]
    fn decimate_keeps_total_time() {
        let frames: Vec<_> = (0..10).map(|i| ScreenFrame { rgb565: vec![i as u8], delay_ms: 10 }).collect();
        let out = decimate(frames, 3);
        assert_eq!(out.len(), 3);
        assert_eq!(out.iter().map(|f| f.delay_ms).sum::<i32>(), 100);
        assert_eq!(out[1].rgb565, vec![3]);
    }

    #[test]
    fn solid_size() {
        assert_eq!(solid((1, 2, 3)).rgb565.len(), SCREEN_SIZE * SCREEN_SIZE * 2);
    }
}
