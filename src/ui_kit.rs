//! Общие элементы окна, как в Styles.xaml / Palette.xaml C#-версии: шрифты (Segoe UI, Segoe UI Semibold,
//! значки Segoe MDL2 Assets), палитра, заголовки, подписи, карточки, точка состояния и значок батареи.
//!
//! Шрифты берутся из C:\Windows\Fonts. Нет файла — остаются шрифты egui, а значки MDL2 заменяются эмодзи
//! ([`icon`]), так что окно не ломается и на чужой системе.

use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui::{self, epaint::Mesh, Color32, FontData, FontDefinitions, FontFamily, FontId, Pos2, Rect, RichText, Rounding, Shape, Stroke, Vec2};

pub const WINDOW: Color32 = Color32::from_rgb(0x00, 0x00, 0x00);
pub const SIDEBAR: Color32 = Color32::from_rgb(0x05, 0x05, 0x05);
pub const SURFACE: Color32 = Color32::from_rgb(0x0A, 0x0A, 0x0A);
pub const CARD: Color32 = Color32::from_rgb(0x0E, 0x0E, 0x0E);
pub const SURFACE2: Color32 = Color32::from_rgb(0x18, 0x18, 0x18);
pub const HOVER: Color32 = Color32::from_rgb(0x22, 0x22, 0x22);
pub const LINE: Color32 = Color32::from_rgb(0x26, 0x26, 0x26);
pub const LINE_HOVER: Color32 = Color32::from_rgb(0x3C, 0x3C, 0x3C);
pub const TEXT: Color32 = Color32::from_rgb(0xF2, 0xF2, 0xF2);
pub const MUTED: Color32 = Color32::from_rgb(0x9A, 0x9A, 0x9A);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(0x1E, 0x1E, 0x1E);
pub const GOOD: Color32 = Color32::from_rgb(0x7C, 0xD6, 0xA6);
pub const BAD: Color32 = Color32::from_rgb(0xFF, 0x8A, 0x96);
/// «Жидкое стекло» выбранного пункта меню (LiquidGlassBrush / LiquidGlassBorderBrush).
pub const GLASS: Color32 = Color32::from_rgba_premultiplied(0x24, 0x24, 0x24, 0x24);
pub const GLASS_BORDER: Color32 = Color32::from_rgba_premultiplied(0x70, 0x70, 0x70, 0x70);

/// Глифы Segoe MDL2 Assets, которые использует C#-версия.
pub mod glyph {
    pub const DEVICES: char = '\u{E80F}';
    pub const KEYBOARD: char = '\u{E765}';
    pub const MOUSE: char = '\u{E962}';
    pub const PUMP: char = '\u{E7F4}';
    pub const LIGHT: char = '\u{E706}';
    pub const FANS: char = '\u{E9CA}';
    pub const SYSTEM: char = '\u{E770}';
    pub const SETTINGS: char = '\u{E713}';
    pub const CHEVRON: char = '\u{E76C}';
    pub const CHARGING: char = '\u{E945}';
    pub const USB: char = '\u{E88E}';
    pub const WIRELESS: char = '\u{E701}';
    pub const ADD: char = '\u{E710}';
}

static ICONS: AtomicBool = AtomicBool::new(false);

/// Значки MDL2 загружены (есть segmdl2.ttf или SegoeIcons.ttf).
pub fn icons_available() -> bool {
    ICONS.load(Ordering::Relaxed)
}

fn font_file(names: &[&str]) -> Option<Vec<u8>> {
    let dir = std::env::var_os("WINDIR").map(std::path::PathBuf::from).unwrap_or_else(|| "C:\\Windows".into()).join("Fonts");
    names.iter().find_map(|n| std::fs::read(dir.join(n)).ok())
}

/// Шрифты как в C#: Segoe UI (KawaiiFont = «Segoe UI Variable Text, Segoe UI»), Segoe UI Semibold
/// для заголовков и Segoe MDL2 Assets для значков. Шрифты egui остаются запасными (эмодзи, редкие символы).
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let fallback = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();

    if let Some(data) = font_file(&["segoeui.ttf"]) {
        fonts.font_data.insert("segoe".into(), FontData::from_owned(data));
        if let Some(list) = fonts.families.get_mut(&FontFamily::Proportional) {
            list.insert(0, "segoe".into());
        }
    }
    let mut semibold = fallback.clone();
    if let Some(data) = font_file(&["seguisb.ttf", "segoeuib.ttf"]) {
        fonts.font_data.insert("segoe-semibold".into(), FontData::from_owned(data));
        semibold.insert(0, "segoe-semibold".into());
    } else if fonts.font_data.contains_key("segoe") {
        semibold.insert(0, "segoe".into());
    }
    fonts.families.insert(FontFamily::Name("semibold".into()), semibold);

    let mut icons = fallback;
    if let Some(data) = font_file(&["segmdl2.ttf", "SegoeIcons.ttf"]) {
        fonts.font_data.insert("mdl2".into(), FontData::from_owned(data));
        icons.insert(0, "mdl2".into());
        ICONS.store(true, Ordering::Relaxed);
    }
    fonts.families.insert(FontFamily::Name("icons".into()), icons);
    ctx.set_fonts(fonts);
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("semibold".into()))
}

pub fn icon_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("icons".into()))
}

/// Значок MDL2 или, если шрифта нет, похожий эмодзи.
pub fn icon(g: char) -> String {
    if icons_available() {
        return g.to_string();
    }
    match g {
        glyph::DEVICES => "🖥",
        glyph::KEYBOARD => "⌨",
        glyph::MOUSE => "🖱",
        glyph::PUMP => "💧",
        glyph::LIGHT => "💡",
        glyph::FANS => "🌀",
        glyph::SYSTEM => "📊",
        glyph::SETTINGS => "⚙",
        glyph::CHEVRON => "›",
        glyph::CHARGING => "⚡",
        glyph::USB => "🔌",
        glyph::WIRELESS => "📶",
        glyph::ADD => "+",
        _ => "●",
    }
    .to_string()
}

/// Текст значком: MDL2 — своим шрифтом, эмодзи — обычным.
pub fn icon_text(g: char, size: f32, color: Color32) -> RichText {
    let t = RichText::new(icon(g)).color(color);
    if icons_available() { t.font(icon_font(size)) } else { t.size(size) }
}

/// Заголовок страницы (H1): 26, полужирный.
pub fn h1(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).font(semibold(26.0)).color(TEXT));
}

/// Заголовок раздела (H2): 17, полужирный.
pub fn h2(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).font(semibold(17.0)).color(TEXT));
}

/// Подпись (Cap): 12.5, приглушённая, с переносом.
pub fn cap(ui: &mut egui::Ui, text: impl Into<String>) -> egui::Response {
    ui.add(egui::Label::new(RichText::new(text.into()).size(12.5).color(MUTED)).wrap())
}

/// Подпись разделов меню (NavCaption): 10.5, заглавными, приглушённая.
pub fn nav_caption(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).font(semibold(10.5)).color(MUTED));
}

/// Карточка (Card): #0E0E0E, рамка #262626, скругление 12, отступы 20/16.
pub fn card() -> egui::Frame {
    egui::Frame::none()
        .fill(CARD)
        .stroke(Stroke::new(1.0_f32, LINE))
        .rounding(Rounding::same(12.0))
        .inner_margin(egui::Margin::symmetric(20.0, 16.0))
}

/// Точка состояния: зелёная — на связи, красная — нет.
pub fn status_dot(painter: &egui::Painter, center: egui::Pos2, radius: f32, ok: bool) {
    painter.circle_filled(center, radius, if ok { GOOD } else { BAD });
}

/// Вертикальная батарея плитки (12×20): рамка и заливка снизу по заряду.
pub fn battery_vertical(painter: &egui::Painter, rect: Rect, percent: u8, low: u8) {
    painter.rect_stroke(rect, Rounding::same(2.5), Stroke::new(1.5_f32, TEXT));
    let inner = rect.shrink(3.0);
    let h = (inner.height() * f32::from(percent.min(100)) / 100.0).max(1.5);
    let fill = Rect::from_min_max(egui::pos2(inner.left(), inner.bottom() - h), inner.max);
    painter.rect_filled(fill, Rounding::same(1.0), if percent <= low { BAD } else { TEXT });
}

/// Горизонтальная батарея боковой панели (24×12 и «носик» 2×5), левый верхний угол — min.
pub fn battery_horizontal(painter: &egui::Painter, min: egui::Pos2, percent: u8, low: u8) {
    let body = Rect::from_min_size(min, Vec2::new(24.0, 12.0));
    painter.rect_stroke(body, Rounding::same(3.0), Stroke::new(1.5_f32, MUTED));
    let nose = Rect::from_min_size(egui::pos2(body.right() + 0.5, body.center().y - 2.5), Vec2::new(2.0, 5.0));
    painter.rect_filled(nose, Rounding::same(1.0), MUTED);
    let inner = body.shrink(3.0);
    let w = (inner.width() * f32::from(percent.min(100)) / 100.0).max(1.5);
    let fill = Rect::from_min_size(inner.min, Vec2::new(w, inner.height()));
    painter.rect_filled(fill, Rounding::same(1.5), if percent <= low { BAD } else { GOOD });
}

// ── Скруглённые градиенты (плитки тем помпы) ────────────────────────────────

/// Точки контура скруглённого прямоугольника (по часовой, от верхнего левого скругления).
pub fn rounded_points(rect: Rect, radius: f32, per_corner: usize) -> Vec<Pos2> {
    let r = radius.min(rect.width() / 2.0).min(rect.height() / 2.0).max(0.0);
    let corners = [
        (Pos2::new(rect.left() + r, rect.top() + r), std::f32::consts::PI),
        (Pos2::new(rect.right() - r, rect.top() + r), 1.5 * std::f32::consts::PI),
        (Pos2::new(rect.right() - r, rect.bottom() - r), 0.0),
        (Pos2::new(rect.left() + r, rect.bottom() - r), 0.5 * std::f32::consts::PI),
    ];
    let mut pts = Vec::with_capacity(4 * (per_corner + 1));
    for (c, start) in corners {
        for i in 0..=per_corner {
            let a = start + std::f32::consts::FRAC_PI_2 * i as f32 / per_corner as f32;
            pts.push(Pos2::new(c.x + r * a.cos(), c.y + r * a.sin()));
        }
    }
    pts
}

/// Скруглённый прямоугольник с вертикальным градиентом (цвет сверху → снизу).
pub fn gradient_rounded(painter: &egui::Painter, rect: Rect, radius: f32, top: Color32, bottom: Color32) {
    let pts = rounded_points(rect, radius, 8);
    let at = |y: f32| {
        let t = ((y - rect.top()) / rect.height().max(1.0)).clamp(0.0, 1.0);
        lerp_color(top, bottom, t)
    };
    let mut mesh = Mesh::default();
    let c = rect.center();
    mesh.colored_vertex(c, at(c.y));
    for p in &pts {
        mesh.colored_vertex(*p, at(p.y));
    }
    let n = pts.len() as u32;
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    painter.add(Shape::mesh(mesh));
}

pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgba_premultiplied(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), l(a.a(), b.a()))
}

/// Склонение: 1 канал, 2 канала, 5 каналов.
pub fn plural<'a>(n: usize, one: &'a str, few: &'a str, many: &'a str) -> &'a str {
    if (11..=14).contains(&(n % 100)) {
        return many;
    }
    match n % 10 {
        1 => one,
        2..=4 => few,
        _ => many,
    }
}

/// Строка, обрезанная до max символов с «…».
pub fn ellipsize(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut s: String = text.chars().take(max.saturating_sub(1)).collect();
    s.push('…');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plural_ru() {
        assert_eq!(plural(1, "канал", "канала", "каналов"), "канал");
        assert_eq!(plural(3, "канал", "канала", "каналов"), "канала");
        assert_eq!(plural(7, "канал", "канала", "каналов"), "каналов");
        assert_eq!(plural(11, "канал", "канала", "каналов"), "каналов");
        assert_eq!(plural(22, "канал", "канала", "каналов"), "канала");
    }

    #[test]
    fn ellipsis() {
        assert_eq!(ellipsize("абв", 5), "абв");
        assert_eq!(ellipsize("абвгдеж", 4), "абв…");
    }
}
