//! Редактор раскладки экрана помпы (порт редактора сцены из MainWindow.Cooler.cs).
//! Предпросмотр рисуется тем же lcdscene::render, что и на помпе, поэтому что видно — то и будет.
//! Мышь: клик — выделить, тянуть — переместить, колесо над объектом — размер.
//! Клавиши: стрелки — сдвиг на 1 px (Shift — на 10), Delete — удалить,
//! Ctrl+Z — отменить, Ctrl+Y (Ctrl+Shift+Z) — вернуть, Ctrl+A — выделить всё.
//! Тянуть по пустому месту — выделение рамкой; выделенную группу можно двигать и удалять целиком.
//! Раскладка сохраняется в settings.json как CoolerLayout (формат C#).

use crate::cooler;
use crate::lcdscene::{self, Background, Fonts, LcdItem, LcdKind, KIND_ORDER};
use crate::settings::Settings;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, TextureHandle, TextureOptions, Vec2};
use image::RgbImage;
use std::path::Path;

const FONTS: &[&str] = &[lcdscene::SEGOE, lcdscene::MONO, lcdscene::KAWAII, "Arial", "Verdana", "Georgia", "Impact"];

pub struct PumpEditor {
    pub items: Vec<LcdItem>,
    sel: Option<usize>,
    drag_off: Option<(f64, f64)>,
    fonts: Fonts,
    tex: Option<TextureHandle>,
    sig: String,
    add_kind: usize,
    /// Фон (своя картинка или картинка темы): путь, размер, кадры с задержками, общая длина гифки.
    bg: Option<(String, (u32, u32), Vec<(RgbImage, u64)>, u64)>,
    started: std::time::Instant,
    /// Размер сцены: экран помпы с учётом поворота. Раскладка хранится в этих координатах (как в C#).
    base: (u32, u32),
    /// Экран помпы: выбранный вручную, сообщённый помпой или 320×240.
    panel: (u32, u32),
    zoom: f32,
    /// Раскладки до изменений (отмена) и отменённые (вернуть).
    undo: Vec<Vec<LcdItem>>,
    redo: Vec<Vec<LcdItem>>,
    /// Когда раскладка менялась последний раз: серия правок подряд (перетаскивание, колесо) — один шаг отмены.
    last_change: Option<std::time::Instant>,
    /// Выделенные рамкой (если их больше одного).
    group: Vec<usize>,
    /// Рамка выделения: откуда и докуда тянули (в точках сцены).
    rubber: Option<((f64, f64), (f64, f64))>,
    /// Перетаскивание группы: точка старта и исходные места.
    group_drag: Option<((f64, f64), Vec<(f64, f64)>)>,
    /// Поле «цвет объекта»: (объект, его цвет на момент синхронизации, что набрано).
    /// Набранное живёт отдельно от цвета объекта, иначе недописанный «#FF0» стирался бы на следующем кадре.
    color_draft: Option<(usize, String, String)>,
}

fn theme_of(s: &Settings) -> lcdscene::LcdTheme {
    s.cooler_theme.as_deref().and_then(lcdscene::find_theme).unwrap_or_else(lcdscene::default_theme)
}

fn hex(c: (u8, u8, u8)) -> String {
    format!("#{:02X}{:02X}{:02X}", c.0, c.1, c.2)
}

impl PumpEditor {
    pub fn new(s: &Settings) -> Self {
        // Пока помпа не ответила, её экран — выбранный вручную или 320×240 (см. set_panel).
        let panel = crate::service::panel_size(s, None);
        let base = lcdscene::scene_size(panel, s.cooler_rotation);
        let items = lcdscene::saved_layout(s.cooler_layout.as_ref())
            .unwrap_or_else(|| lcdscene::layout_for(&theme_of(s), base.0, base.1));
        Self {
            items, sel: None, drag_off: None, fonts: Fonts::default(), tex: None, sig: String::new(),
            add_kind: LcdKind::Text.index(), bg: None, started: std::time::Instant::now(), base, panel, zoom: 1.5,
            undo: Vec::new(), redo: Vec::new(), last_change: None, group: Vec::new(), rubber: None, group_drag: None,
            color_draft: None,
        }
    }

    /// «Как в теме»: раскладка темы вместо своей (фон не трогаем).
    pub fn reset_to_theme(&mut self, s: &mut Settings) {
        self.items = lcdscene::layout_for(&theme_of(s), self.base.0, self.base.1);
        self.sel = None;
        self.group.clear();
        s.cooler_layout = None;
    }

    /// Размер экрана помпы стал известен или его сменили в настройках (detected — ответ помпы).
    /// Своя раскладка уже в координатах экрана и не меняется; раскладка темы пересчитывается под новый размер.
    pub fn set_panel(&mut self, s: &Settings, detected: Option<(u32, u32)>) {
        let panel = crate::service::panel_size(s, detected);
        if panel == self.panel {
            return;
        }
        self.panel = panel;
        self.base = lcdscene::scene_size(panel, s.cooler_rotation);
        if s.cooler_layout.is_none() {
            self.items = lcdscene::layout_for(&theme_of(s), self.base.0, self.base.1);
        }
        // Номера и история относились к прежней сцене.
        self.sel = None;
        self.group.clear();
        self.group_drag = None;
        self.drag_off = None;
        self.undo.clear();
        self.redo.clear();
        self.sig.clear();
    }

    /// ApplyTheme из C#: тема меняет раскладку и фон целиком (цвета, картинку, яркость).
    pub fn apply_theme(&mut self, s: &mut Settings, theme: &lcdscene::LcdTheme) {
        lcdscene::apply_theme_background(theme, s);
        self.reset_to_theme(s);
    }

    /// Фон на момент t: кадр гифки по времени или картинка. None — градиент.
    fn background(&mut self, s: &Settings) -> Option<(usize, &RgbImage)> {
        let Some(path) = s.cooler_background.clone().filter(|p| !p.trim().is_empty()) else {
            // Фон убрали: кадры прежней гифки не нужны (и не должны дёргать перерисовку 25 раз в секунду).
            self.bg = None;
            return None;
        };
        let fresh = matches!(&self.bg, Some((p, sz, _, _)) if *p == path && *sz == self.base);
        if !fresh {
            // Неудачу тоже запоминаем (пустым списком кадров): иначе битый или пропавший файл
            // читался бы с диска на каждом кадре окна.
            let frames: Vec<(RgbImage, u64)> = cooler::load_image(Path::new(&path), self.base.0, self.base.1, 0, true)
                .unwrap_or_default()
                .into_iter()
                .map(|f| (f.image, u64::from(f.delay_ms.max(20))))
                .collect();
            let total = frames.iter().map(|f| f.1).sum();
            self.bg = Some((path, self.base, frames, total));
        }
        let (_, _, frames, total) = self.bg.as_ref()?;
        let mut idx = 0;
        if frames.len() > 1 && *total > 0 {
            let mut t = self.started.elapsed().as_millis() as u64 % total;
            for (n, f) in frames.iter().enumerate() {
                if t < f.1 {
                    idx = n;
                    break;
                }
                t -= f.1;
            }
        }
        frames.get(idx).map(|f| (idx, &f.0))
    }

    /// Ушли со страницы помпы: отдать картинку предпросмотра, кадры фона и шрифты (раскладка и история остаются).
    pub fn release(&mut self) {
        self.tex = None;
        self.bg = None;
        self.fonts = Fonts::default();
        self.sig.clear();
    }

    /// Фон анимирован (гифка) — предпросмотру нужна частая перерисовка.
    pub fn animated(&self) -> bool {
        matches!(&self.bg, Some((_, _, f, _)) if f.len() > 1)
    }

    /// Нарисовать редактор. Возвращает true, если раскладка изменилась (её надо сохранить).
    pub fn show(&mut self, ui: &mut egui::Ui, s: &mut Settings) -> bool {
        let before = format!("{:?}", self.items);
        let before_items = self.items.clone();
        let mut from_history = false;

        // Поворот 90/270 — портретная сцена: растягиваем раскладку под новый размер.
        let base = lcdscene::scene_size(self.panel, s.cooler_rotation);
        if base != self.base {
            self.items = lcdscene::stretch(&self.items, (self.base.0 as f64, self.base.1 as f64), (base.0 as f64, base.1 as f64));
            self.base = base;
        }
        let (w, h) = self.base;

        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("pump_add_kind")
                .selected_text(KIND_ORDER[self.add_kind].title())
                .show_ui(ui, |ui| {
                    for (i, k) in KIND_ORDER.iter().enumerate() {
                        ui.selectable_value(&mut self.add_kind, i, k.title());
                    }
                });
            if ui.button("Добавить").clicked() {
                let mut it = LcdItem::create(KIND_ORDER[self.add_kind]);
                it.x = 16.0;
                it.y = (h as f64 / 2.0 - 12.0).max(0.0);
                self.items.push(it);
                self.select_only(self.items.len() - 1);
            }
            if ui.add_enabled(self.sel.is_some(), egui::Button::new("Удалить")).clicked() {
                self.delete_selected();
            }
            if ui.add_enabled(self.sel.is_some(), egui::Button::new("Копия")).clicked() {
                if let Some(i) = self.sel {
                    let mut it = self.items[i].clone();
                    it.x += 10.0;
                    it.y += 10.0;
                    self.items.push(it);
                    self.select_only(self.items.len() - 1);
                }
            }
            if ui.button("Как в теме").on_hover_text("Сбросить свою раскладку на раскладку выбранной темы").clicked() {
                self.reset_to_theme(s);
            }
        });
        ui.horizontal(|ui| {
            if ui.add_enabled(!self.undo.is_empty(), egui::Button::new("Отменить")).on_hover_text("Ctrl+Z").clicked() {
                from_history |= self.step_back();
            }
            if ui.add_enabled(!self.redo.is_empty(), egui::Button::new("Вернуть")).on_hover_text("Ctrl+Y").clicked() {
                from_history |= self.step_forward();
            }
            if !self.group.is_empty() {
                ui.label(format!("Выделено: {}", self.group.len()));
            }
        });

        // ── Предпросмотр ──
        let theme = theme_of(s);
        let snap = crate::telemetry::live().unwrap_or_else(lcdscene::sample_snapshot);
        let now = lcdscene::now();
        let top = s.cooler_bg_top.as_deref().and_then(lcdscene::parse_color)
            .or_else(|| lcdscene::parse_color(theme.top)).unwrap_or((0x10, 0x12, 0x1A));
        let bottom = s.cooler_bg_bottom.as_deref().and_then(lcdscene::parse_color)
            .or_else(|| lcdscene::parse_color(theme.bottom)).unwrap_or((5, 6, 10));
        let k = s.cooler_brightness.clamp(0, 100) as f64 / 100.0;
        let frame_no = self.background(s).map(|b| b.0);
        let values = lcdscene::signature(&self.items, &snap, &now);
        let sig = format!("{before}|{values}|{:?}|{frame_no:?}|{top:?}{bottom:?}{k}|{}|{w}x{h}", s.cooler_background, now.format("%H%M%S"));
        if sig != self.sig || self.tex.is_none() {
            let bg = self.background(s).map(|b| b.1.clone());
            let background = match &bg { Some(img) => Background::Image(img), None => Background::Gradient(top, bottom) };
            let img = lcdscene::render(w, h, &self.items, &snap, &now, background, k, &mut self.fonts);
            let color = egui::ColorImage::from_rgb([w as usize, h as usize], img.as_raw());
            match &mut self.tex {
                Some(t) => t.set(color, TextureOptions::LINEAR),
                None => self.tex = Some(ui.ctx().load_texture("pump_scene", color, TextureOptions::LINEAR)),
            }
            self.sig = sig;
        }
        // Часы на предпросмотре идут, фон-гифка проигрывается.
        let repaint = if self.animated() { 40 } else { 500 };
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(repaint));

        // Сцена — на всю ширину карточки (не больше 3×).
        // Сцена — на всю ширину карточки: маленький экран увеличиваем (до 3×), большой (640×480) — ужимаем.
        self.zoom = (ui.available_width() / w as f32).clamp(0.25, 3.0);
        let z = self.zoom;
        let (resp, painter) = ui.allocate_painter(Vec2::new(w as f32 * z, h as f32 * z), Sense::click_and_drag());
        let origin = resp.rect.min;
        if let Some(t) = &self.tex {
            painter.image(t.id(), resp.rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }
        painter.rect_stroke(resp.rect, 0.0, Stroke::new(1.0_f32, Color32::from_gray(90)));
        let to_scene = |p: Pos2| (((p.x - origin.x) / z) as f64, ((p.y - origin.y) / z) as f64);

        if resp.drag_started() {
            if let Some(p) = resp.interact_pointer_pos() {
                let (x, y) = to_scene(p);
                let hit = lcdscene::hit(&self.items, x, y, &snap, &now, &mut self.fonts);
                match hit {
                    // Тянем один из выделенных рамкой — едет вся группа.
                    Some(i) if self.group.len() > 1 && self.group.contains(&i) => {
                        let start = self.group.iter().map(|&g| (self.items[g].x, self.items[g].y)).collect();
                        self.group_drag = Some(((x, y), start));
                        self.sel = Some(i);
                        self.drag_off = None;
                    }
                    Some(i) => {
                        self.group.clear();
                        self.sel = Some(i);
                        self.drag_off = Some((x - self.items[i].x, y - self.items[i].y));
                    }
                    // По пустому месту — рамка.
                    None => {
                        self.group.clear();
                        self.sel = None;
                        self.drag_off = None;
                        self.rubber = Some(((x, y), (x, y)));
                    }
                }
            }
        }
        if resp.dragged() {
            if let Some(p) = resp.interact_pointer_pos() {
                let (x, y) = to_scene(p);
                if let Some(((sx, sy), start)) = &self.group_drag {
                    let (dx, dy) = ((x - sx).round(), (y - sy).round());
                    for (n, &g) in self.group.iter().enumerate() {
                        if let (Some(it), Some(&(ox, oy))) = (self.items.get_mut(g), start.get(n)) {
                            it.x = (ox + dx).clamp(-(w as f64), 2.0 * w as f64);
                            it.y = (oy + dy).clamp(-(h as f64), 2.0 * h as f64);
                        }
                    }
                } else if let (Some(i), Some((dx, dy))) = (self.sel, self.drag_off) {
                    let (_, _, bw, bh) = lcdscene::bounds(&self.items[i], &snap, &now, &mut self.fonts);
                    let it = &mut self.items[i];
                    it.x = (x - dx).round().clamp(-bw + 8.0, w as f64 - 8.0);
                    it.y = (y - dy).round().clamp(-bh + 8.0, h as f64 - 8.0);
                } else if let Some((a, _)) = self.rubber {
                    self.rubber = Some((a, (x, y)));
                }
            }
        }
        if resp.drag_stopped() {
            self.drag_off = None;
            self.group_drag = None;
            if let Some(((x0, y0), (x1, y1))) = self.rubber.take() {
                let (l, r) = (x0.min(x1), x0.max(x1));
                let (t, b) = (y0.min(y1), y0.max(y1));
                let mut picked = Vec::new();
                for i in 0..self.items.len() {
                    let (bx, by, bw, bh) = lcdscene::bounds(&self.items[i], &snap, &now, &mut self.fonts);
                    if bx < r && bx + bw > l && by < b && by + bh > t {
                        picked.push(i);
                    }
                }
                self.sel = picked.last().copied();
                self.group = if picked.len() > 1 { picked } else { Vec::new() };
            }
        }
        if resp.clicked() {
            if let Some(p) = resp.interact_pointer_pos() {
                let (x, y) = to_scene(p);
                self.sel = lcdscene::hit(&self.items, x, y, &snap, &now, &mut self.fonts);
                self.group.clear();
            }
        }
        // Колесо над сценой — размер выделенного.
        if resp.hovered() {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll != 0.0 {
                if let Some(i) = self.sel.filter(|i| *i < self.items.len()) {
                    // Колесо меняет размер всех выделенных (OnSceneWheel в C#).
                    let k = if scroll > 0.0 { 1.08 } else { 1.0 / 1.08 };
                    for j in self.selection(i) {
                        let it = &mut self.items[j];
                        it.scale = (it.scale * k).clamp(0.3, 5.0);
                    }
                }
            }
        }
        // Клавиши, пока не печатаем в текстовом поле.
        if ui.ctx().memory(|m| m.focused().is_none()) {
            let (undo, redo, all) = ui.input(|inp| {
                let c = inp.modifiers.command;
                (
                    c && !inp.modifiers.shift && inp.key_pressed(egui::Key::Z),
                    c && (inp.key_pressed(egui::Key::Y) || (inp.modifiers.shift && inp.key_pressed(egui::Key::Z))),
                    c && inp.key_pressed(egui::Key::A),
                )
            });
            if undo {
                from_history |= self.step_back();
            }
            if redo {
                from_history |= self.step_forward();
            }
            if all && !self.items.is_empty() {
                self.group = (0..self.items.len()).collect();
                self.sel = self.items.len().checked_sub(1);
                if self.group.len() < 2 {
                    self.group.clear();
                }
            }
        }
        if ui.ctx().memory(|m| m.focused().is_none()) {
            if let Some(i) = self.sel.filter(|i| *i < self.items.len()) {
                let (dx, dy, del) = ui.input(|inp| {
                    let step = if inp.modifiers.shift { 10.0 } else { 1.0 };
                    let mut d = (0.0, 0.0, false);
                    if inp.key_pressed(egui::Key::ArrowLeft) { d.0 -= step; }
                    if inp.key_pressed(egui::Key::ArrowRight) { d.0 += step; }
                    if inp.key_pressed(egui::Key::ArrowUp) { d.1 -= step; }
                    if inp.key_pressed(egui::Key::ArrowDown) { d.1 += step; }
                    d.2 = inp.key_pressed(egui::Key::Delete);
                    d
                });
                if self.group.len() > 1 {
                    for &g in &self.group {
                        if let Some(it) = self.items.get_mut(g) {
                            it.x += dx;
                            it.y += dy;
                        }
                    }
                } else {
                    self.items[i].x += dx;
                    self.items[i].y += dy;
                }
                if del {
                    self.delete_selected();
                }
            }
        }
        self.group.retain(|g| *g < self.items.len());
        for &g in &self.group {
            if Some(g) == self.sel {
                continue;
            }
            let (bx, by, bw, bh) = lcdscene::bounds(&self.items[g], &snap, &now, &mut self.fonts);
            let r = Rect::from_min_size(
                Pos2::new(origin.x + bx as f32 * z, origin.y + by as f32 * z),
                Vec2::new(bw as f32 * z, bh as f32 * z),
            ).expand(2.0);
            painter.rect_stroke(r, 2.0, Stroke::new(1.0_f32, Color32::from_rgb(0x4D, 0xA3, 0xFF)));
        }
        if let Some(((x0, y0), (x1, y1))) = self.rubber {
            let r = Rect::from_two_pos(
                Pos2::new(origin.x + x0 as f32 * z, origin.y + y0 as f32 * z),
                Pos2::new(origin.x + x1 as f32 * z, origin.y + y1 as f32 * z),
            );
            painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(0x4D, 0xA3, 0xFF, 40));
            painter.rect_stroke(r, 0.0, Stroke::new(1.0_f32, Color32::from_rgb(0x4D, 0xA3, 0xFF)));
        }
        if let Some(i) = self.sel.filter(|i| *i < self.items.len()) {
            let (bx, by, bw, bh) = lcdscene::bounds(&self.items[i], &snap, &now, &mut self.fonts);
            let r = Rect::from_min_size(
                Pos2::new(origin.x + bx as f32 * z, origin.y + by as f32 * z),
                Vec2::new(bw as f32 * z, bh as f32 * z),
            ).expand(2.0);
            painter.rect_stroke(r, 2.0, Stroke::new(1.5_f32, Color32::from_rgb(0x4D, 0xA3, 0xFF)));
        }
        ui.small("Клик — выделить · тянуть — двигать · колесо — размер · стрелки (Shift ×10) · Delete — удалить · тянуть по пустому — рамка · Ctrl+Z / Ctrl+Y — отмена / возврат · Ctrl+A — всё. Цифры на предпросмотре — живые, как на помпе.");

        // ── Свойства выделенного ──
        ui.separator();
        match self.sel.filter(|i| *i < self.items.len()) {
            None => { ui.label("Выделите объект на сцене, чтобы изменить его."); }
            Some(i) => self.props(ui, i, w, h),
        }

        let changed = format!("{:?}", self.items) != before;
        if changed && !from_history {
            // Правки подряд (каждый кадр перетаскивания, щелчки колеса) сливаем в один шаг.
            let series = self.last_change.map(|t| t.elapsed() < std::time::Duration::from_millis(500)).unwrap_or(false);
            if !series {
                self.undo.push(before_items);
                if self.undo.len() > 100 {
                    self.undo.remove(0);
                }
            }
            self.redo.clear();
            self.last_change = Some(std::time::Instant::now());
        }
        if changed {
            s.cooler_layout = Some(lcdscene::items_to_json(&self.items));
        }
        changed
    }

    /// Объект переставили с места from на место to («На передний / задний план»). Группа хранит номера
    /// объектов, поэтому их надо пересчитать: иначе они указывали бы на другие объекты и «Удалить»
    /// стёрло бы не то. Группа остаётся выделенной (в C# выделение хранит сами объекты).
    fn moved(&mut self, from: usize, to: usize) {
        for g in &mut self.group {
            *g = moved_index(*g, from, to);
        }
        self.sel = Some(to);
        self.group_drag = None;
        self.drag_off = None;
    }

    /// Выделить один объект (после «Добавить» и «Копия»): прежнюю группу снимаем.
    fn select_only(&mut self, index: usize) {
        self.sel = Some(index);
        self.group.clear();
        self.group_drag = None;
        self.drag_off = None;
    }

    /// Кого касается правка свойства: вся группа, если главный объект в ней, иначе только он.
    fn selection(&self, primary: usize) -> Vec<usize> {
        if self.group.len() > 1 && self.group.contains(&primary) {
            self.group.iter().copied().filter(|g| *g < self.items.len()).collect()
        } else {
            vec![primary]
        }
    }

    fn delete_selected(&mut self) {
        let mut all: Vec<usize> = std::mem::take(&mut self.group);
        if let Some(i) = self.sel.take() {
            all.push(i);
        }
        all.sort_unstable();
        all.dedup();
        for i in all.into_iter().rev() {
            if i < self.items.len() {
                self.items.remove(i);
            }
        }
    }

    /// Отменить последний шаг. true — раскладка взята из истории.
    fn step_back(&mut self) -> bool {
        let Some(prev) = self.undo.pop() else { return false };
        self.redo.push(std::mem::replace(&mut self.items, prev));
        self.after_history();
        true
    }

    /// Вернуть отменённый шаг.
    fn step_forward(&mut self) -> bool {
        let Some(next) = self.redo.pop() else { return false };
        self.undo.push(std::mem::replace(&mut self.items, next));
        self.after_history();
        true
    }

    fn after_history(&mut self) {
        self.last_change = None;
        self.drag_off = None;
        self.group_drag = None;
        self.rubber = None;
        // Номера группы относятся к раскладке до шага истории: после отмены перестановки или удаления
        // они указывали бы на другие объекты (и «Удалить» стёрло бы невыделенное) — группу снимаем.
        self.group.clear();
        if self.sel.map(|i| i >= self.items.len()).unwrap_or(false) {
            self.sel = None;
        }
    }

    fn props(&mut self, ui: &mut egui::Ui, i: usize, w: u32, h: u32) {
        let others: Vec<usize> = self.selection(i).into_iter().filter(|j| *j != i).collect();
        let was = self.items[i].clone();
        // Поле цвета: набранное сбрасываем, только когда выделили другой объект или цвет сменили не здесь.
        let mut draft = match self.color_draft.take() {
            Some((j, synced, text)) if j == i && synced == was.color => text,
            _ => was.color.clone(),
        };
        let it = &mut self.items[i];
        ui.strong(it.kind.title());
        egui::Grid::new("pump_props").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("Положение");
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut it.x).range(-200.0..=w as f64).prefix("X "));
                ui.add(egui::DragValue::new(&mut it.y).range(-200.0..=h as f64).prefix("Y "));
            });
            ui.end_row();

            ui.label("Размер");
            ui.add(egui::Slider::new(&mut it.scale, 0.3..=5.0).fixed_decimals(2));
            ui.end_row();

            ui.label("Цвет");
            ui.horizontal(|ui| {
                let mut auto = it.color.trim().is_empty();
                let temp = matches!(it.kind, LcdKind::CpuTemp | LcdKind::GpuTemp);
                if ui.checkbox(&mut auto, if temp { "Авто (по нагреву)" } else { "Авто" }).changed() {
                    it.color = if auto {
                        String::new()
                    } else {
                        let c = LcdItem::create(it.kind).color;
                        if c.is_empty() { "#FFFFFF".to_string() } else { c }
                    };
                }
                if !auto {
                    let c = lcdscene::parse_color(&it.color).unwrap_or((255, 255, 255));
                    let mut rgb = [c.0, c.1, c.2];
                    if crate::color_wheel::wheel_button(ui, ui.next_auto_id(), &mut rgb) {
                        it.color = hex((rgb[0], rgb[1], rgb[2]));
                    }
                    if ui.add(egui::TextEdit::singleline(&mut draft).desired_width(70.0)).changed()
                        && lcdscene::parse_color(&draft).is_some()
                    {
                        it.color = draft.trim().to_uppercase();
                    }
                }
            });
            ui.end_row();

            ui.label("Шрифт");
            egui::ComboBox::from_id_salt("pump_font").selected_text(it.font.clone()).show_ui(ui, |ui| {
                for f in FONTS {
                    ui.selectable_value(&mut it.font, f.to_string(), *f);
                }
            });
            ui.end_row();

            ui.label("");
            ui.horizontal(|ui| {
                ui.checkbox(&mut it.bold, "Жирный");
                if it.has_label() {
                    let caption = format!("Подпись «{}»", it.label_text());
                    ui.checkbox(&mut it.label, caption);
                }
                if it.kind == LcdKind::Time {
                    ui.checkbox(&mut it.seconds, "Секунды");
                }
            });
            ui.end_row();

            if it.kind == LcdKind::Text {
                ui.label("Текст");
                ui.text_edit_singleline(&mut it.text);
                ui.end_row();
            }
        });
        // Цвет сменили кругом или флажком «Авто» — показываем его; иначе оставляем набранное.
        let now = self.items[i].clone();
        if now.color != was.color && lcdscene::parse_color(&draft).map(hex) != lcdscene::parse_color(&now.color).map(hex) {
            draft = now.color.clone();
        }
        self.color_draft = Some((i, now.color.clone(), draft));
        // Общие свойства — всем выделенным (ApplyProp в C#); текст и место у каждого свои.
        for j in others {
            let o = &mut self.items[j];
            if now.scale != was.scale && was.scale > 0.0 {
                // У группы размер меняется во столько же раз, во сколько у главного: пропорции сохраняются.
                o.scale = (o.scale * now.scale / was.scale).clamp(0.3, 5.0);
            }
            if now.color != was.color { o.color = now.color.clone(); }
            if now.font != was.font { o.font = now.font.clone(); }
            if now.bold != was.bold { o.bold = now.bold; }
            if now.label != was.label && o.has_label() { o.label = now.label; }
            if now.seconds != was.seconds && o.kind == LcdKind::Time { o.seconds = now.seconds; }
        }
        ui.horizontal(|ui| {
            if ui.button("На передний план").clicked() && i + 1 < self.items.len() {
                let it = self.items.remove(i);
                self.items.push(it);
                self.moved(i, self.items.len() - 1);
            }
            if ui.button("На задний план").clicked() && i > 0 {
                let it = self.items.remove(i);
                self.items.insert(0, it);
                self.moved(i, 0);
            }
            if ui.button("По центру").clicked() {
                let snap = crate::telemetry::live().unwrap_or_else(lcdscene::sample_snapshot);
                let now = lcdscene::now();
                let (_, _, bw, _) = lcdscene::bounds(&self.items[self.sel.unwrap_or(i)], &snap, &now, &mut self.fonts);
                let j = self.sel.unwrap_or(i);
                self.items[j].x = ((w as f64 - bw) / 2.0).round();
            }
        });
    }
}

/// Новый номер объекта index после того, как объект с места from переставили на место to
/// (остальные сдвинулись на одно место навстречу).
fn moved_index(index: usize, from: usize, to: usize) -> usize {
    if index == from {
        to
    } else if from < to && index > from && index <= to {
        index - 1
    } else if to < from && index >= to && index < from {
        index + 1
    } else {
        index
    }
}

#[cfg(test)]
mod tests {
    use super::moved_index;

    /// Переставить, как это делает редактор, и проверить, что номера указывают на те же объекты.
    fn check(from: usize, to: usize) {
        let before = vec!['A', 'B', 'C', 'D', 'E'];
        let mut after = before.clone();
        let it = after.remove(from);
        after.insert(to, it);
        for (i, item) in before.iter().enumerate() {
            assert_eq!(after[moved_index(i, from, to)], *item, "{from}->{to}, объект {item}");
        }
    }

    #[test]
    fn editor_works_in_panel_coordinates() {
        use super::PumpEditor;
        use crate::settings::Settings;
        // Экран 640×480 выбран вручную: сцена — 640×480, тема подогнана под неё.
        let mut s = Settings { cooler_size: 4, ..Settings::default() };
        let small = PumpEditor::new(&Settings::default());
        let big = PumpEditor::new(&s);
        assert_eq!((small.base, big.base), ((320, 240), (640, 480)));
        assert_eq!(big.items[0].scale, (small.items[0].scale * 2.0 * 100.0).round_ties_even() / 100.0);
        // Своя раскладка (в том числе из C#) уже в координатах экрана — берётся как есть.
        s.cooler_layout = Some(serde_json::json!([{"Kind": 9, "X": 376, "Y": 40, "Scale": 2, "Text": "A"}]));
        let saved = PumpEditor::new(&s);
        assert_eq!((saved.items[0].x, saved.items[0].y, saved.items[0].scale), (376.0, 40.0, 2.0));
        // Помпа сообщила размер позже: тема пересчитывается, своя раскладка — нет.
        let auto = Settings::default();
        let mut e = PumpEditor::new(&auto);
        e.set_panel(&auto, Some((640, 480)));
        assert_eq!(e.base, (640, 480));
        assert_eq!(e.items[0].scale, big.items[0].scale);
        let mut own = Settings { cooler_layout: s.cooler_layout.clone(), ..Settings::default() };
        own.cooler_rotation = 90;
        let mut e = PumpEditor::new(&own);
        assert_eq!(e.base, (240, 320));
        e.set_panel(&own, Some((640, 480)));
        assert_eq!(e.base, (480, 640));
        assert_eq!((e.items[0].x, e.items[0].scale), (376.0, 2.0));
    }

    #[test]
    fn group_indices_follow_objects_after_reorder() {
        for from in 0..5 {
            check(from, 4); // на передний план
            check(from, 0); // на задний план
        }
        // Пример из разбора ошибки: [A,B,C,D], выделены A и B, B — на передний план → [A,C,D,B].
        assert_eq!([0usize, 1].map(|g| moved_index(g, 1, 3)), [0, 3]);
    }
}
