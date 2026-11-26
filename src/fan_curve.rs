//! FanCurveEditor.cs + кривые из MainWindow.Fans.cs → egui.
//! Кривая «температура CPU → скорость вентилятора»: точки тянутся мышью, между ними — прямые.
//! Скорость вдоль кривой только растёт: точка, поднятая выше соседней, тянет её за собой.

#[cfg(feature = "gui")]
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, Vec2};

pub const MIN_T: f64 = 20.0;
pub const MAX_T: f64 = 100.0;
pub const MIN_P: f64 = 0.0;
pub const MAX_P: f64 = 100.0;
const MIN_GAP_T: f64 = 3.0;
/// FanMinPercent: ниже вентилятор не опускается.
pub const FAN_MIN_PERCENT: i32 = 20;
pub const FAN_MODE_FIXED: i32 = 5;
pub const FAN_MODE_CURVE: i32 = 6;

/// DefaultFanCurve: с чего начинается «Своя кривая».
pub const DEFAULT_CURVE: [(f64, f64); 5] = [(35.0, 30.0), (50.0, 40.0), (65.0, 55.0), (75.0, 75.0), (85.0, 100.0)];

/// FanCurves: кривые режимов 1–3 (Тихий, Обычный, Производительный).
pub const PRESETS: [&[(f64, f64)]; 4] = [
    &[],
    &[(40.0, 25.0), (60.0, 35.0), (75.0, 60.0), (85.0, 100.0)],
    &[(40.0, 35.0), (60.0, 50.0), (75.0, 75.0), (85.0, 100.0)],
    &[(40.0, 50.0), (60.0, 70.0), (75.0, 90.0), (80.0, 100.0)],
];

#[cfg(feature = "gui")]
const LINE: Color32 = Color32::from_rgb(0x26, 0x26, 0x26);
#[cfg(feature = "gui")]
const MUTED: Color32 = Color32::from_rgb(0x9A, 0x9A, 0x9A);
#[cfg(feature = "gui")]
const TEXT: Color32 = Color32::from_rgb(0xF2, 0xF2, 0xF2);
#[cfg(feature = "gui")]
const MINT: Color32 = Color32::from_rgb(0x5E, 0xE6, 0xB0);

/// FanCurveEditor.Evaluate: скорость по кривой при температуре temp.
pub fn evaluate(points: &[(f64, f64)], temp: f64) -> f64 {
    let Some(first) = points.first() else { return 100.0 };
    if temp <= first.0 {
        return first.1;
    }
    for i in 1..points.len() {
        if temp <= points[i].0 {
            let k = (temp - points[i - 1].0) / (points[i].0 - points[i - 1].0).max(0.001);
            return points[i - 1].1 + (points[i].1 - points[i - 1].1) * k;
        }
    }
    points[points.len() - 1].1
}

/// FanCurveOf: плоский массив [T0, P0, T1, P1, …] из settings.json → точки (или кривая по умолчанию).
pub fn from_flat(flat: Option<&Vec<i32>>) -> Vec<(f64, f64)> {
    match flat {
        Some(f) if f.len() >= 4 && f.len() % 2 == 0 => {
            let mut pts: Vec<(f64, f64)> =
                f.chunks(2).map(|c| (c[0].clamp(0, 110) as f64, c[1].clamp(FAN_MIN_PERCENT, 100) as f64)).collect();
            pts.sort_by(|a, b| a.0.total_cmp(&b.0));
            pts
        }
        _ => DEFAULT_CURVE.to_vec(),
    }
}

/// SaveFanCurve: точки → плоский массив (Math.Round — банковское округление).
pub fn to_flat(points: &[(f64, f64)]) -> Vec<i32> {
    points.iter().flat_map(|p| [p.0.round_ties_even() as i32, p.1.round_ties_even() as i32]).collect()
}

/// Перетаскивание точки i: температура — между соседями (зазор 3 °C), скорость — от пола до 100 %.
/// Поднятая точка тянет вверх следующие, опущенная — вниз предыдущие.
pub fn drag_point(points: &mut [(f64, f64)], i: usize, t: f64, p: f64, floor: f64) {
    if i >= points.len() {
        return;
    }
    let lo = if i == 0 { MIN_T } else { points[i - 1].0 + MIN_GAP_T };
    let hi = if i + 1 == points.len() { MAX_T } else { points[i + 1].0 - MIN_GAP_T };
    let t = t.clamp(lo, hi.max(lo));
    let p = p.clamp(floor.clamp(MIN_P, MAX_P), MAX_P);
    points[i] = (t, p);
    for pt in points.iter_mut().skip(i + 1) {
        if pt.1 < p {
            pt.1 = p;
        }
    }
    for pt in points.iter_mut().take(i) {
        if pt.1 > p {
            pt.1 = p;
        }
    }
}

#[cfg(feature = "gui")]
/// Редактор кривой высотой 210 px. temp — текущая температура CPU (вертикальная линия).
/// Возвращает true, когда точку отпустили (событие Changed в C#): кривую пора сохранить.
pub fn editor(ui: &mut egui::Ui, id_salt: &str, points: &mut Vec<(f64, f64)>, temp: Option<f64>, floor: f64) -> bool {
    let size = Vec2::new(ui.available_width().max(200.0), 210.0);
    let (resp, painter) = ui.allocate_painter(size, Sense::click_and_drag());
    let rect = resp.rect;
    let plot = Rect::from_min_max(rect.min + Vec2::new(38.0, 12.0), rect.max - Vec2::new(12.0, 24.0));
    let to_screen = move |t: f64, p: f64| {
        Pos2::new(
            plot.left() + ((t - MIN_T) / (MAX_T - MIN_T)) as f32 * plot.width(),
            plot.bottom() - ((p - MIN_P) / (MAX_P - MIN_P)) as f32 * plot.height(),
        )
    };
    let from_screen = move |s: Pos2| {
        (
            MIN_T + ((s.x - plot.left()) / plot.width()) as f64 * (MAX_T - MIN_T),
            MIN_P + ((plot.bottom() - s.y) / plot.height()) as f64 * (MAX_P - MIN_P),
        )
    };
    let nearest = |pts: &[(f64, f64)], pos: Pos2| {
        pts.iter()
            .enumerate()
            .map(|(i, &(t, p))| (i, to_screen(t, p).distance(pos)))
            .filter(|(_, d)| *d < 12.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    };

    let id = ui.make_persistent_id(id_salt);
    let mut drag: Option<usize> = ui.data(|d| d.get_temp::<Option<usize>>(id)).flatten();
    if resp.drag_started() {
        drag = resp.interact_pointer_pos().and_then(|pos| nearest(points.as_slice(), pos));
    }
    if let (Some(i), Some(pos)) = (drag, resp.interact_pointer_pos()) {
        if resp.dragged() {
            let (t, p) = from_screen(pos);
            drag_point(points.as_mut_slice(), i, t, p, floor);
            ui.ctx().request_repaint();
        }
    }
    let mut released = false;
    if resp.drag_stopped() {
        released = drag.is_some();
        drag = None;
    }
    ui.data_mut(|d| d.insert_temp(id, drag));
    let hover = drag.or_else(|| resp.hover_pos().and_then(|pos| nearest(points.as_slice(), pos)));
    if hover.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }

    // Сетка: каждые 10 °C и 20 %.
    let grid = Stroke::new(1.0_f32, LINE);
    let font = FontId::proportional(11.0);
    let mut t = MIN_T;
    while t <= MAX_T + 0.1 {
        let a = to_screen(t, MIN_P);
        painter.line_segment([a, to_screen(t, MAX_P)], grid);
        if (t as i32) % 20 == 0 {
            painter.text(Pos2::new(a.x, plot.bottom() + 4.0), Align2::CENTER_TOP, format!("{}°", t as i32), font.clone(), MUTED);
        }
        t += 10.0;
    }
    let mut p = MIN_P;
    while p <= MAX_P + 0.1 {
        let a = to_screen(MIN_T, p);
        painter.line_segment([a, to_screen(MAX_T, p)], grid);
        painter.text(Pos2::new(plot.left() - 6.0, a.y), Align2::RIGHT_CENTER, format!("{}%", p as i32), font.clone(), MUTED);
        p += 20.0;
    }
    // Ниже пола вентилятор не опустится.
    if floor > MIN_P {
        let fl = Rect::from_min_max(to_screen(MIN_T, floor), to_screen(MAX_T, MIN_P));
        painter.rect_filled(fl, 0.0, Color32::from_rgba_unmultiplied(0x9A, 0x9A, 0x9A, 18));
    }
    if let (Some(first), Some(last)) = (points.first(), points.last()) {
        let mut line = vec![to_screen(MIN_T, first.1)];
        line.extend(points.iter().map(|&(t, p)| to_screen(t, p)));
        line.push(to_screen(MAX_T, last.1));
        painter.add(Shape::line(line, Stroke::new(2.0_f32, TEXT)));
    }
    if let Some(t) = temp {
        let t = t.clamp(MIN_T, MAX_T);
        painter.extend(Shape::dashed_line(&[to_screen(t, MIN_P), to_screen(t, MAX_P)], Stroke::new(1.0_f32, MINT), 3.0, 3.0));
        let p = evaluate(points.as_slice(), t);
        painter.circle_filled(to_screen(t, p), 4.0, MINT);
        painter.text(to_screen(t, MAX_P) + Vec2::new(4.0, 0.0), Align2::LEFT_TOP, format!("{t:.0} °C → {p:.0} %"), font.clone(), MINT);
    }
    for (i, &(t, p)) in points.iter().enumerate() {
        let r = if hover == Some(i) { 6.5 } else { 5.0 };
        painter.circle(to_screen(t, p), r, Color32::BLACK, Stroke::new(2.0_f32, TEXT));
    }
    if let Some(&(t, p)) = hover.and_then(|i| points.get(i)) {
        painter.text(to_screen(t, p) + Vec2::new(8.0, -8.0), Align2::LEFT_BOTTOM, format!("{t:.0} °C → {p:.0} %"), font, TEXT);
    }
    released
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluate_like_csharp() {
        let pts = DEFAULT_CURVE;
        assert_eq!(evaluate(&pts, 10.0), 30.0);
        assert_eq!(evaluate(&pts, 90.0), 100.0);
        assert!((evaluate(&pts, 57.5) - 47.5).abs() < 1e-9);
        assert_eq!(evaluate(&[], 50.0), 100.0);
    }

    #[test]
    fn drag_keeps_curve_rising() {
        let mut pts = DEFAULT_CURVE.to_vec();
        drag_point(&mut pts, 1, 80.0, 90.0, 20.0);
        assert_eq!(pts[1].0, 62.0); // упёрлись в соседа справа (65 − 3)
        assert!(pts.windows(2).all(|w| w[0].1 <= w[1].1));
        drag_point(&mut pts, 3, 75.0, 5.0, 20.0);
        assert_eq!(pts[3].1, 20.0);
        assert!(pts.windows(2).all(|w| w[0].1 <= w[1].1));
    }

    #[test]
    fn flat_roundtrip() {
        let flat = vec![60, 50, 40, 10, 85, 100, 75, 70];
        let pts = from_flat(Some(&flat));
        assert_eq!(pts[0], (40.0, 20.0));
        assert_eq!(to_flat(&pts), vec![40, 20, 60, 50, 75, 70, 85, 100]);
        assert_eq!(from_flat(Some(&vec![1, 2, 3])), DEFAULT_CURVE.to_vec());
    }
}
