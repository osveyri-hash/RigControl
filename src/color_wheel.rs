//! ColorWheel.cs -> egui: оттенок по кругу, насыщенность от центра (белый) к краю,
//! под кругом ползунок яркости. Колечко показывает выбранный цвет.
use eframe::egui::{self, Color32, ColorImage, Pos2, Sense, Stroke, TextureHandle, TextureOptions, Vec2};

use crate::color::{from_hsv, to_hsv};

fn wheel_texture(ctx: &egui::Context, size: usize) -> TextureHandle {
    let id = egui::Id::new(("rc_color_wheel_tex", size));
    if let Some(t) = ctx.data(|d| d.get_temp::<TextureHandle>(id)) {
        return t;
    }
    let mut px = vec![Color32::TRANSPARENT; size * size];
    let r = size as f64 / 2.0;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f64 + 0.5 - r;
            let dy = y as f64 + 0.5 - r;
            let dist = (dx * dx + dy * dy).sqrt();
            let alpha = (r - dist).clamp(0.0, 1.0); // сглаженный край
            if alpha <= 0.0 {
                continue;
            }
            let hue = ((-dy).atan2(dx).to_degrees() + 360.0) % 360.0;
            let (cr, cg, cb) = from_hsv(hue, (dist / r).min(1.0), 1.0);
            let a = (255.0 * alpha) as u8;
            px[y * size + x] = Color32::from_rgba_unmultiplied(cr, cg, cb, a);
        }
    }
    let img = ColorImage { size: [size, size], pixels: px };
    let t = ctx.load_texture("rc_color_wheel", img, TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, t.clone()));
    t
}

/// Круг + ползунок яркости. Возвращает true, если цвет изменился.
pub fn color_wheel(ui: &mut egui::Ui, id_salt: impl std::hash::Hash, color: &mut [u8; 3], diameter: f32) -> bool {
    let id = ui.make_persistent_id(("rc_color_wheel", id_salt));
    let (h0, s0, v0) = to_hsv((color[0], color[1], color[2]));
    // У серого и чёрного оттенка нет — держим прошлый, чтобы колечко не прыгало.
    let (mut h, mut s) = ui.data(|d| d.get_temp::<(f64, f64)>(id)).unwrap_or((h0, s0));
    if s0 > 0.001 && v0 > 0.001 {
        h = h0;
    }
    if v0 > 0.001 {
        s = s0;
    }
    let mut v = v0;
    let mut changed = false;

    let tex = wheel_texture(ui.ctx(), (diameter * 2.0).round() as usize);
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(diameter), Sense::click_and_drag());
    let resp = resp.on_hover_cursor(egui::CursorIcon::Crosshair);
    ui.painter().image(tex.id(), rect, egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);

    let r = diameter as f64 / 2.0;
    if resp.is_pointer_button_down_on() || resp.dragged() || resp.clicked() {
        if let Some(p) = resp.interact_pointer_pos() {
            let dx = (p.x - rect.center().x) as f64;
            let dy = (p.y - rect.center().y) as f64;
            h = ((-dy).atan2(dx).to_degrees() + 360.0) % 360.0;
            s = ((dx * dx + dy * dy).sqrt() / r).clamp(0.0, 1.0);
            if v < 0.05 {
                v = 1.0; // при «чёрной» яркости выбор по кругу ничего бы не показал
            }
            changed = true;
        }
    }

    let a = h.to_radians();
    let hx = rect.center().x + (a.cos() * s * (r - 1.0)) as f32;
    let hy = rect.center().y - (a.sin() * s * (r - 1.0)) as f32;
    let ring = if v > 0.55 && s < 0.35 { Color32::DARK_GRAY } else { Color32::WHITE };
    ui.painter().circle_stroke(Pos2::new(hx, hy), 8.0, Stroke::new(3.0_f32, Color32::from_black_alpha(140)));
    ui.painter().circle_stroke(Pos2::new(hx, hy), 7.0, Stroke::new(2.2_f32, ring));

    ui.add_space(10.0);
    let mut pct = (v * 100.0).round();
    ui.spacing_mut().slider_width = diameter;
    if ui.add(egui::Slider::new(&mut pct, 0.0..=100.0).show_value(false)).changed() {
        v = pct / 100.0;
        changed = true;
    }

    ui.data_mut(|d| d.insert_temp(id, (h, s)));
    if changed {
        let (cr, cg, cb) = from_hsv(h, s, v);
        *color = [cr, cg, cb];
    }
    changed
}

/// Кнопка-образец цвета: по щелчку всплывает круг (как ColorWheel.ShowPopup), цвет меняется сразу.
pub fn wheel_button(ui: &mut egui::Ui, id_salt: impl std::hash::Hash + Copy, color: &mut [u8; 3]) -> bool {
    let fill = Color32::from_rgb(color[0], color[1], color[2]);
    let btn = ui.add(egui::Button::new("").fill(fill).min_size(Vec2::new(40.0, 22.0)))
        .on_hover_text(crate::color::hex((color[0], color[1], color[2])));
    let popup_id = ui.make_persistent_id(("rc_wheel_popup", id_salt));
    if btn.clicked() {
        ui.memory_mut(|m| m.toggle_popup(popup_id));
    }
    let mut changed = false;
    egui::popup::popup_below_widget(ui, popup_id, &btn, egui::popup::PopupCloseBehavior::CloseOnClickOutside, |ui| {
        ui.set_min_width(170.0);
        changed = color_wheel(ui, id_salt, color, 170.0);
        ui.label(crate::color::hex((color[0], color[1], color[2])));
    });
    changed
}
