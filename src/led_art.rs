//! Картинки устройств с «живыми» светодиодами и крутящейся крыльчаткой.
//!
//! *_dark.png — картинка с погашенными светодиодами (родная подсветка снята), *_glow.png — где и насколько
//! ярко светятся светодиоды (белый с прозрачностью, уже с мягким ореолом). Свечение рисуется сеткой
//! треугольников с этой текстурой: цвет вершины — цвет светодиода из кадра argb::Renderer в этом месте,
//! прозрачность 0 — свет складывается с картинкой. Подсветка идёт слева направо по всей СЖО, как на
//! настоящей: первый светодиод — слева, последний — справа, между ними цвета плавно перетекают.

use std::collections::HashMap;

use eframe::egui::{self, epaint::Mesh, epaint::Vertex, Color32, Pos2, Rect, Shape, TextureHandle, TextureOptions, Vec2};

/// Какие картинки есть.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Art {
    /// СЖО целиком: радиатор с тремя вентиляторами и помпа (argb.png, 600×367).
    Cooler,
    /// Один вентилятор (fans.png, 404×400).
    Fan,
}

/// Крыльчатка на fans.png: центр и радиус круга, который крутится (в пикселях картинки 404×400).
pub const FAN_ROTOR: (f32, f32, f32) = (207.0, 199.0, 160.0);

/// Вдоль чего бежит подсветка СЖО: от левого края радиатора к правому (он на картинке наклонён).
/// Точка отсчёта, направление и где на этой оси первый и последний светодиоды (в пикселях argb.png).
const COOLER_AXIS: ((f32, f32), (f32, f32), f32, f32) = ((160.0, 123.0), (0.979_790_5, 0.200_026_4), -89.5, 380.3);
/// У вентилятора — просто слева направо (пиксели fans.png).
const FAN_X: (f32, f32) = (15.0, 392.0);

fn bytes(name: &str) -> Option<&'static [u8]> {
    Some(match name {
        "cooler_dark" => include_bytes!("../assets/leds/argb_dark.png"),
        "cooler_glow" => include_bytes!("../assets/leds/argb_glow.png"),
        "fan_dark" => include_bytes!("../assets/leds/fan_dark.png"),
        "fan_glow" => include_bytes!("../assets/leds/fan_glow.png"),
        "fan_glow_small" => include_bytes!("../assets/leds/fan_glow_small.png"),
        "fan" => include_bytes!("../assets/devices/fans.png"),
        // fans.png без крыльчатки (круг r=160 залит тёмным): на неё кладётся повёрнутая крыльчатка.
        // Иначе сквозь прозрачные щели между лопастями видны неподвижные лопасти — «два слоя».
        "fan_static" => include_bytes!("../assets/leds/fan_static.png"),
        _ => return None,
    })
}

/// Текстуры — загружаются один раз.
#[derive(Default)]
pub struct LedArt {
    tex: HashMap<&'static str, Option<TextureHandle>>,
}

/// Цвет светодиода в точке t (0 — первый, 1 — последний) с плавным переходом между соседними.
fn led_at(frame: &[u8], leds: usize, t: f32, k: f32) -> Color32 {
    let pos = t.clamp(0.0, 1.0) * (leds.saturating_sub(1)) as f32;
    let i0 = (pos.floor() as usize).min(leds - 1);
    let i1 = (i0 + 1).min(leds - 1);
    let f = pos - i0 as f32;
    let ch = |c: usize| {
        let a = f32::from(frame[i0 * 3 + c]);
        let b = f32::from(frame[i1 * 3 + c]);
        ((a + (b - a) * f) * k).round().clamp(0.0, 255.0) as u8
    };
    // Прозрачность 0 при цвете — сложение света с картинкой под ним.
    Color32::from_rgba_premultiplied(ch(0), ch(1), ch(2), 0)
}

impl LedArt {
    pub fn texture(&mut self, ctx: &egui::Context, name: &'static str) -> Option<TextureHandle> {
        self.tex
            .entry(name)
            .or_insert_with(|| {
                let img = image::load_from_memory(bytes(name)?).ok()?.to_rgba8();
                let c = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
                Some(ctx.load_texture(format!("ledart-{name}"), c, TextureOptions::LINEAR))
            })
            .clone()
    }

    /// Нарисовать картинку `art`, вписанную в `area`, с подсветкой из кадра `frame` (RGB на светодиод,
    /// `leds` штук) и яркостью 0…1. frame = None — подсветка погашена. Возвращает, куда легла картинка.
    #[allow(clippy::too_many_arguments)]
    pub fn paint(
        &mut self, ui: &egui::Ui, area: Rect, art: Art, frame: Option<&[u8]>, leds: usize, _fans: usize,
        brightness: f32, rotor_angle: Option<f32>,
    ) -> Rect {
        let ctx = ui.ctx().clone();
        let (dark, glow) = match art {
            Art::Cooler => ("cooler_dark", "cooler_glow"),
            Art::Fan => ("fan_dark", if area.width() < 160.0 { "fan_glow_small" } else { "fan_glow" }),
        };
        let Some(tex) = self.texture(&ctx, dark) else { return area };
        let size = tex.size_vec2();
        let k = (area.width() / size.x).min(area.height() / size.y);
        let rect = Rect::from_center_size(area.center(), size * k);
        let painter = ui.painter_at(area.expand(4.0));
        painter.image(tex.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        if art == Art::Fan {
            if let (Some(angle), Some(t)) = (rotor_angle, self.texture(&ctx, "fan")) {
                rotor(&painter, &t, rect, angle);
            }
        }
        let Some(frame) = frame else { return rect };
        let leds = leds.max(1).min(frame.len() / 3);
        if leds == 0 {
            return rect;
        }
        let Some(glow_tex) = self.texture(&ctx, glow) else { return rect };
        let bri = brightness.clamp(0.0, 1.0);
        // Где на «линии подсветки» точка картинки (u, v — доли ширины и высоты), 0…1.
        let along = |u: f32, v: f32| -> f32 {
            match art {
                Art::Cooler => {
                    let ((ox, oy), (dx, dy), t0, t1) = COOLER_AXIS;
                    let (x, y) = (u * 600.0, v * 367.0);
                    ((x - ox) * dx + (y - oy) * dy - t0) / (t1 - t0)
                }
                Art::Fan => (u * 404.0 - FAN_X.0) / (FAN_X.1 - FAN_X.0),
            }
        };
        // Сетка: цвет между вершинами смешивается линейно, поэтому вдоль подсветки — мелкий шаг.
        let (nx, ny) = match art {
            Art::Cooler => (120usize, 12usize),
            Art::Fan => (48, 2),
        };
        let mut mesh = Mesh::with_texture(glow_tex.id());
        for j in 0..=ny {
            let v = j as f32 / ny as f32;
            for i in 0..=nx {
                let u = i as f32 / nx as f32;
                mesh.vertices.push(Vertex {
                    pos: rect.min + Vec2::new(u * rect.width(), v * rect.height()),
                    uv: Pos2::new(u, v),
                    color: led_at(frame, leds, along(u, v), bri),
                });
            }
        }
        let row = (nx + 1) as u32;
        for j in 0..ny as u32 {
            for i in 0..nx as u32 {
                let a = j * row + i;
                mesh.add_triangle(a, a + 1, a + row);
                mesh.add_triangle(a + 1, a + row + 1, a + row);
            }
        }
        painter.add(Shape::mesh(mesh));
        rect
    }

    /// Вентилятор как на картинке (с её цветными уголками), вписанный в area; tint — приглушить.
    /// rotor_angle — повернуть крыльчатку (None — как на картинке).
    pub fn fan_plain(&mut self, ui: &egui::Ui, area: Rect, rotor_angle: Option<f32>, tint: Color32) -> Rect {
        let ctx = ui.ctx().clone();
        let Some(tex) = self.texture(&ctx, "fan") else { return area };
        let base = if rotor_angle.is_some() { self.texture(&ctx, "fan_static").unwrap_or_else(|| tex.clone()) } else { tex.clone() };
        let size = tex.size_vec2();
        let k = (area.width() / size.x).min(area.height() / size.y);
        let rect = Rect::from_center_size(area.center(), size * k);
        let painter = ui.painter_at(area.expand(4.0));
        painter.image(base.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), tint);
        if let Some(a) = rotor_angle {
            rotor_tinted(&painter, &tex, rect, a, tint);
        }
        rect
    }
}

/// Крыльчатка fans.png, повёрнутая на angle (радианы), поверх картинки в rect.
pub fn rotor(painter: &egui::Painter, tex: &TextureHandle, rect: Rect, angle: f32) {
    rotor_tinted(painter, tex, rect, angle, Color32::WHITE);
}

/// То же с оттенком (приглушённый вентилятор).
pub fn rotor_tinted(painter: &egui::Painter, tex: &TextureHandle, rect: Rect, angle: f32, tint: Color32) {
    let size = tex.size_vec2();
    let (cx, cy, r) = FAN_ROTOR;
    let k = rect.width() / size.x;
    let center = rect.min + Vec2::new(cx, cy) * k;
    let uv_c = Pos2::new(cx / size.x, cy / size.y);
    let mut mesh = Mesh::with_texture(tex.id());
    mesh.vertices.push(Vertex { pos: center, uv: uv_c, color: tint });
    let n = 72u32;
    let (s, c) = angle.sin_cos();
    for i in 0..n {
        let a = std::f32::consts::TAU * i as f32 / n as f32;
        let (dx, dy) = (a.cos(), a.sin());
        // Картинку берём из места, повёрнутого назад, — на экране она повёрнута вперёд.
        let (ux, uy) = (dx * c + dy * s, -dx * s + dy * c);
        mesh.vertices.push(Vertex {
            pos: center + Vec2::new(dx, dy) * r * k,
            uv: Pos2::new((cx + ux * r) / size.x, (cy + uy * r) / size.y),
            color: tint,
        });
    }
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    painter.add(Shape::mesh(mesh));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pictures_decode() {
        for name in ["cooler_dark", "cooler_glow", "fan_dark", "fan_glow", "fan_glow_small", "fan", "fan_static"] {
            let b = bytes(name).unwrap_or_else(|| panic!("{name}"));
            assert!(image::load_from_memory(b).is_ok(), "{name}");
        }
    }

    #[test]
    fn colors_flow_left_to_right() {
        let frame = [255, 0, 0, 0, 0, 255];
        assert_eq!(led_at(&frame, 2, 0.0, 1.0), Color32::from_rgba_premultiplied(255, 0, 0, 0));
        assert_eq!(led_at(&frame, 2, 1.0, 1.0), Color32::from_rgba_premultiplied(0, 0, 255, 0));
        assert_eq!(led_at(&frame, 2, 0.5, 1.0), Color32::from_rgba_premultiplied(128, 0, 128, 0));
        assert_eq!(led_at(&frame, 2, 0.5, 0.0), Color32::from_rgba_premultiplied(0, 0, 0, 0));
    }
}
