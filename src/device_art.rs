//! DeviceArt.cs: картинки устройств для плиток главной. Сначала своя картинка рядом с программой
//! (папка Devices: mouse.png, keyboard.png, pump.png, argb.png, fans.png или .jpg), затем вшитая
//! (assets/devices — те же картинки, что Assets/Devices в C#), иначе — значок в круге.

use std::collections::HashMap;
use std::path::PathBuf;

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};

pub const MOUSE: &str = "mouse";
pub const KEYBOARD: &str = "keyboard";
pub const PUMP: &str = "pump";
pub const ARGB: &str = "argb";
pub const FANS: &str = "fans";

fn embedded(key: &str) -> Option<&'static [u8]> {
    Some(match key {
        MOUSE => include_bytes!("../assets/devices/mouse.png"),
        KEYBOARD => include_bytes!("../assets/devices/keyboard.png"),
        PUMP => include_bytes!("../assets/devices/pump.png"),
        ARGB => include_bytes!("../assets/devices/argb.png"),
        FANS => include_bytes!("../assets/devices/fans.png"),
        _ => return None,
    })
}

fn external(key: &str) -> Option<Vec<u8>> {
    let dir: PathBuf = std::env::current_exe().ok()?.parent()?.join("Devices");
    ["png", "jpg", "jpeg"].iter().find_map(|ext| std::fs::read(dir.join(format!("{key}.{ext}"))).ok())
}

/// Декодировать и уменьшить до height пикселей по высоте (своя картинка может быть огромной).
fn decode(bytes: &[u8], height: u32) -> Option<ColorImage> {
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let img = if img.height() > height {
        let w = (u64::from(img.width()) * u64::from(height) / u64::from(img.height().max(1))).max(1) as u32;
        image::imageops::resize(&img, w, height, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let size = [img.width() as usize, img.height() as usize];
    Some(ColorImage::from_rgba_unmultiplied(size, img.as_raw()))
}

/// Кэш текстур: каждая картинка декодируется один раз.
#[derive(Default)]
pub struct DeviceArt {
    cache: HashMap<&'static str, Option<TextureHandle>>,
}

impl DeviceArt {
    pub fn texture(&mut self, ctx: &egui::Context, key: &'static str) -> Option<TextureHandle> {
        self.cache
            .entry(key)
            .or_insert_with(|| {
                let img = external(key)
                    .and_then(|b| decode(&b, 330))
                    .or_else(|| embedded(key).and_then(|b| decode(b, 330)))?;
                Some(ctx.load_texture(format!("device-{key}"), img, TextureOptions::LINEAR))
            })
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_pictures_decode() {
        for key in [MOUSE, KEYBOARD, PUMP, ARGB, FANS] {
            let img = decode(embedded(key).unwrap(), 330).unwrap();
            assert!(img.size[1] <= 330 && img.size[0] > 0, "{key}");
        }
    }
}
