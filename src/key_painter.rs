//! Схема AK820 Pro для «Своей раскраски» (MainWindow.KeyPaint.cs): колпачки как на клавиатуре
//! (рисунок с фото — assets/keyart.png = Assets/KeyArt.png, у Esc и F-ряда — цвет колпачка с подписью),
//! раскраска — светящимся ободком вокруг колпачка, как свет из-под кейкапа.
//! ЛКМ — красить (можно вести), Shift+ЛКМ — стереть, ПКМ — пипетка.
//! С «Подсветкой в реальном времени» схема показывает то, что горит сейчас; во время рисования — раскраску.
//! Раскраска хранится в settings.json (KeyPaint/PaintColor) как в C#-версии.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use eframe::egui::{self, vec2, Align2, Color32, ColorImage, FontId, Pos2, Rect, Sense, Stroke, TextureHandle, TextureOptions};

use crate::keypaint::{self, KeyCap, Paint, HEIGHT, KEYS, ORIGIN_X, ORIGIN_Y, WIDTH};
use crate::protocol::parse_color;
use crate::settings::Settings;

/// Сколько после правки схема показывает раскраску, а не то, что горит.
const EDIT_HOLD: Duration = Duration::from_millis(1500);

pub struct KeyPainter {
    pub paint: BTreeMap<String, String>,
    pub brush: [u8; 3],
    last: Option<u8>,
    stroke_saved: bool,
    erasing: bool,
    history: Vec<BTreeMap<String, String>>,
    edit_at: Option<Instant>,
    art: Option<TextureHandle>,
    art_failed: bool,
}

fn hex(c: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

fn rgb(s: &str) -> [u8; 3] {
    let (r, g, b) = parse_color(Some(s), (0, 0, 0));
    [r, g, b]
}

fn load_art(ctx: &egui::Context) -> Option<TextureHandle> {
    let img = image::load_from_memory(include_bytes!("../assets/keyart.png")).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let ci = ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    Some(ctx.load_texture("keyart", ci, TextureOptions::LINEAR))
}

impl KeyPainter {
    pub fn new(s: &Settings) -> Self {
        Self {
            paint: s.key_paint.clone().unwrap_or_default(),
            brush: s.paint_color.as_deref().map(rgb).unwrap_or([0xFF, 0x7E, 0xB6]),
            last: None,
            stroke_saved: false,
            erasing: false,
            history: Vec::new(),
            edit_at: None,
            art: None,
            art_failed: false,
        }
    }

    /// Ушли со страницы клавиатуры: отдать картинку клавиш (загрузится снова при возврате).
    pub fn release(&mut self) {
        self.art = None;
        self.art_failed = false;
    }

    /// Кадр для прошивки (режим 0x80) с учётом яркости 0–5.
    pub fn frame(&self, brightness: i32) -> Vec<u8> {
        let p: Paint = self.paint.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        keypaint::build_frame(Some(&p), brightness)
    }

    pub fn count(&self) -> usize {
        self.paint.len()
    }

    pub fn can_undo(&self) -> bool {
        !self.history.is_empty()
    }

    fn snapshot(&mut self) {
        self.history.push(self.paint.clone());
        if self.history.len() > 50 {
            self.history.remove(0);
        }
    }

    fn store(&mut self, s: &mut Settings) {
        s.key_paint = if self.paint.is_empty() { None } else { Some(self.paint.clone()) };
        self.edit_at = Some(Instant::now());
    }

    pub fn set_brush(&mut self, c: [u8; 3], s: &mut Settings) {
        self.brush = c;
        s.paint_color = Some(hex(c));
    }

    pub fn fill_all(&mut self, s: &mut Settings) {
        self.snapshot();
        let h = hex(self.brush);
        for k in KEYS {
            self.paint.insert(k.code.to_string(), h.clone());
        }
        self.store(s);
    }

    pub fn clear(&mut self, s: &mut Settings) {
        self.snapshot();
        self.paint.clear();
        self.store(s);
    }

    pub fn preset(&mut self, i: usize, s: &mut Settings) {
        let Some((_, color_of)) = keypaint::PRESETS.get(i) else { return };
        self.snapshot();
        for k in KEYS {
            self.paint.insert(k.code.to_string(), color_of(k));
        }
        self.store(s);
    }

    pub fn undo(&mut self, s: &mut Settings) {
        if let Some(p) = self.history.pop() {
            self.paint = p;
            self.store(s);
        }
    }

    /// Схема. mirror — «Подсветка в реальном времени». Возвращает true, если раскраска изменилась.
    pub fn canvas(&mut self, ui: &mut egui::Ui, s: &mut Settings, mirror: bool) -> bool {
        if self.art.is_none() && !self.art_failed {
            self.art = load_art(ui.ctx());
            self.art_failed = self.art.is_none();
        }
        let scale = (ui.available_width() / WIDTH as f32).min(330.0 / HEIGHT as f32);
        let size = vec2(WIDTH as f32 * scale, HEIGHT as f32 * scale);
        let (resp, painter) = ui.allocate_painter(size, Sense::click_and_drag());
        let resp = resp.on_hover_cursor(egui::CursorIcon::Crosshair);
        let o = resp.rect.min;
        painter.rect(resp.rect, 10.0 * scale, Color32::from_rgb(0x1A, 0x1A, 0x1A), Stroke::new(1.0_f32, Color32::from_rgb(0x53, 0x5B, 0x68)));

        let key_rect = |k: &KeyCap| {
            Rect::from_min_size(
                Pos2::new(o.x + (k.x - ORIGIN_X) as f32 * scale, o.y + (k.y - ORIGIN_Y) as f32 * scale),
                vec2(k.w as f32 * scale, k.h as f32 * scale),
            )
        };
        let pointer = resp.interact_pointer_pos().or(resp.hover_pos());
        let hovered = pointer.and_then(|p| KEYS.iter().find(|k| key_rect(k).expand(3.0 * scale).contains(p)).copied());

        // Рисование: ЛКМ — красить, Shift — стереть.
        let mut changed = false;
        let (pressed, down, shift) = ui.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_down(), i.modifiers.shift));
        if pressed && resp.hovered() {
            self.stroke_saved = false;
            self.last = None;
            self.erasing = shift;
        }
        let drawing = down && resp.is_pointer_button_down_on();
        if drawing {
            if let Some(k) = hovered {
                if self.last != Some(k.code) {
                    self.last = Some(k.code);
                    let code = k.code.to_string();
                    let want = if self.erasing { None } else { Some(hex(self.brush)) };
                    if self.paint.get(&code) != want.as_ref() {
                        if !self.stroke_saved {
                            self.snapshot();
                            self.stroke_saved = true;
                        }
                        match want {
                            Some(h) => {
                                self.paint.insert(code, h);
                            }
                            None => {
                                self.paint.remove(&code);
                            }
                        }
                        changed = true;
                    }
                }
            }
        } else {
            self.last = None;
        }
        // ПКМ — пипетка.
        if resp.secondary_clicked() {
            if let Some(k) = hovered {
                if let Some(h) = self.paint.get(&k.code.to_string()).cloned() {
                    self.set_brush(rgb(&h), s);
                }
            }
        }
        if changed {
            self.store(s);
        }

        // Что светится: то, что горит сейчас, или раскраска (пока рисуют и ещё чуть-чуть после).
        let editing = drawing || self.edit_at.is_some_and(|t| t.elapsed() < EDIT_HOLD);
        let mirror_shown = mirror && !editing;
        let shown = if mirror_shown { crate::light_mirror::display() } else { None };
        if mirror {
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }

        for k in KEYS {
            let r = key_rect(k).shrink(1.5 * scale);
            let glow = if mirror_shown {
                shown.as_ref().and_then(|(bank, _)| crate::light_mirror::bank_color(bank, k.code)).map(|(r, g, b)| [r, g, b])
            } else {
                self.paint.get(&k.code.to_string()).map(|h| rgb(h))
            };
            let lit = glow.filter(|c| u32::from(c[0]) + u32::from(c[1]) + u32::from(c[2]) > 24);
            let rounding = 3.0 * scale;
            if let Some(c) = lit {
                let col = Color32::from_rgb(c[0], c[1], c[2]);
                painter.rect_filled(r.expand(3.0 * scale), rounding + 2.0, col.gamma_multiply(0.35));
            }
            // Колпачок: рисунок с фото (подписи уже на нём) или цвет колпачка с подписью.
            match (&self.art, keypaint::has_art(k.code)) {
                (Some(tex), true) => {
                    let uv = Rect::from_min_size(
                        Pos2::new(((k.x - ORIGIN_X - 2.0) / WIDTH) as f32, ((k.y - ORIGIN_Y - 5.5) / HEIGHT) as f32),
                        vec2(((k.w + 4.0) / WIDTH) as f32, ((k.h + 4.0) / HEIGHT) as f32),
                    );
                    painter.image(tex.id(), r, uv, Color32::WHITE);
                }
                _ => {
                    let [cr, cg, cb] = rgb(keypaint::cap_color(k.code));
                    painter.rect_filled(r, rounding, Color32::from_rgb(cr, cg, cb));
                    if k.code != 102 && !k.label.is_empty() {
                        let light = 0.299 * f32::from(cr) + 0.587 * f32::from(cg) + 0.114 * f32::from(cb) > 150.0;
                        let tc = if light { Color32::from_rgb(0x22, 0x22, 0x22) } else { Color32::WHITE };
                        painter.text(r.center(), Align2::CENTER_CENTER, k.label, FontId::proportional(9.0 * scale), tc);
                    }
                }
            }
            let stroke = match lit {
                Some(c) => Stroke::new(1.5 * scale, Color32::from_rgb(c[0], c[1], c[2])),
                None => Stroke::new(1.0_f32, Color32::from_rgb(0x47, 0x4C, 0x55)),
            };
            painter.rect_stroke(r, rounding, stroke);
            if hovered.is_some_and(|h| h.code == k.code) {
                painter.rect_stroke(r.expand(1.0), rounding, Stroke::new(1.5_f32, Color32::WHITE));
            }
        }
        if let Some(k) = hovered {
            let name = if k.label.is_empty() { "Пробел" } else { k.label };
            let cur = self.paint.get(&k.code.to_string()).cloned().unwrap_or_else(|| "не закрашена".into());
            resp.clone().on_hover_text_at_pointer(format!("{name} — {cur}"));
        }
        changed
    }

    /// Подсказка под схемой в режиме «в реальном времени» (PaintMirrorHint).
    pub fn mirror_hint() -> Option<&'static str> {
        match crate::light_mirror::display() {
            None => Some("Программа ещё ничего не отправляла на клавиатуру — схема покажет подсветку после первого эффекта."),
            Some((_, true)) => Some("Эффект прошивки рисует сама клавиатура — на схеме похожая картинка, не точная копия."),
            Some((_, false)) => None,
        }
    }
}
