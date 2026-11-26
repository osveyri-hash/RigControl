//! Порт GifLight.cs: подсветка по гифке с экрана клавиатуры — живыми кадрами по клавишам (только по кабелю).
//! - «Картинка» — кадр растянут на всю клавиатуру: каждая клавиша светит средним цветом своего участка;
//! - «Волна от экрана» — главный цвет кадра выходит из экранчика (справа, над «→») и расходится кругами:
//!   дальние клавиши показывают то, что экран показывал чуть раньше.
//! Всё тяжёлое считается один раз при запуске, на каждый кадр подсветки остаётся смешать два кадра гифки.
//! Округления — банковские (Math.Round в .NET), как в C#.

use crate::keypaint::{self, KEYS};
use crate::protocol::{self, ScreenFrame};
use crate::Rgb;

pub const PICTURE: i32 = 0;
pub const RIPPLE: i32 = 1;
pub const SINGLE: i32 = 2;

/// Каналы RGB565 → 0…1: как на экране (с гаммой) и в линейной яркости (для честного усреднения).
struct Tables {
    g5: [f64; 32],
    g6: [f64; 64],
    l5: [f64; 32],
    l6: [f64; 64],
}

impl Tables {
    fn new() -> Self {
        let mut t = Tables { g5: [0.0; 32], g6: [0.0; 64], l5: [0.0; 32], l6: [0.0; 64] };
        for v in 0..32 {
            t.g5[v] = v as f64 / 31.0;
            t.l5[v] = (v as f64 / 31.0).powf(2.2);
        }
        for v in 0..64 {
            t.g6[v] = v as f64 / 63.0;
            t.l6[v] = (v as f64 / 63.0).powf(2.2);
        }
        t
    }
}

fn to_byte(v: f64) -> u8 {
    (v * 255.0).round_ties_even().clamp(0.0, 255.0) as u8
}

fn hue(r: f64, g: f64, b: f64, max: f64, delta: f64) -> f64 {
    if delta <= 0.0 {
        return 0.0;
    }
    let h = if max == r {
        60.0 * (((g - b) / delta) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };
    if h < 0.0 {
        h + 360.0
    } else {
        h
    }
}

fn to_hsv(c: Rgb) -> (f64, f64, f64) {
    let (r, g, b) = (f64::from(c.0) / 255.0, f64::from(c.1) / 255.0, f64::from(c.2) / 255.0);
    let max = r.max(g.max(b));
    let delta = max - r.min(g.min(b));
    (hue(r, g, b, max, delta), if max <= 0.0 { 0.0 } else { delta / max }, max)
}

fn from_hsv(h: f64, s: f64, v: f64) -> Rgb {
    let h = (h % 360.0 + 360.0) % 360.0;
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    (to_byte(r + m), to_byte(g + m), to_byte(b + m))
}

/// Пастель на светодиодах выглядит белой — поднимаем насыщенность (бледные цвета сильнее). Серое остаётся серым.
fn vivid(c: Rgb, amount: f64, min_v: f64) -> Rgb {
    let (h, mut s, v) = to_hsv(c);
    if s > 0.06 {
        s = (s + (1.0 - s) * (1.0 - s) * amount).min(0.95);
    }
    from_hsv(h, s, v.max(min_v))
}

fn with_value(c: Rgb, v: f64) -> Rgb {
    let (h, s, _) = to_hsv(c);
    from_hsv(h, s, v)
}

/// Цвет экрана → яркость светодиода: экран светит с гаммой, светодиод — линейно.
fn to_led(c: Rgb) -> Rgb {
    let f = |x: u8| to_byte((f64::from(x) / 255.0).powf(2.2));
    (f(c.0), f(c.1), f(c.2))
}

fn rgb565_at(px: &[u8], i: usize) -> u16 {
    u16::from(px[i]) | (u16::from(px[i + 1]) << 8)
}

/// Главный цвет кадра RGB565: оттенок, который занимает больше всего места с учётом насыщенности и яркости
/// (а не средний цвет — тот у пёстрых гифок выходит серо-бурым). Почти бесцветная картинка — её средний цвет.
pub fn dominant(rgb565: &[u8]) -> Rgb {
    const BINS: usize = 24;
    let t = Tables::new();
    let mut weight = [0.0f64; BINS];
    let mut sum_r = [0.0f64; BINS];
    let mut sum_g = [0.0f64; BINS];
    let mut sum_b = [0.0f64; BINS];
    let (mut pr, mut pg, mut pb) = (0.0, 0.0, 0.0);
    let mut plain = 0usize;
    let mut samples = 0usize;
    let size = protocol::SCREEN_SIZE;

    // Каждый второй пиксель по обеим осям: 4096 точек на кадр — с запасом.
    for y in (0..size).step_by(2) {
        for x in (0..size).step_by(2) {
            let i = (y * size + x) * 2;
            if i + 1 >= rgb565.len() {
                continue;
            }
            let value = rgb565_at(rgb565, i);
            let r = t.g5[usize::from((value >> 11) & 0x1F)];
            let g = t.g6[usize::from((value >> 5) & 0x3F)];
            let b = t.g5[usize::from(value & 0x1F)];
            samples += 1;
            let max = r.max(g.max(b));
            if max < 0.08 {
                continue; // фон
            }
            pr += r;
            pg += g;
            pb += b;
            plain += 1;
            let delta = max - r.min(g.min(b));
            let s = delta / max;
            if s < 0.12 {
                continue; // почти серый — в оттенки не идёт
            }
            let bin = (hue(r, g, b, max, delta) / (360.0 / BINS as f64)) as usize % BINS;
            let w = s * s * max;
            weight[bin] += w;
            sum_r[bin] += r * w;
            sum_g[bin] += g * w;
            sum_b[bin] += b * w;
        }
    }

    let mut best: Option<usize> = None;
    let (mut best_score, mut total) = (0.0, 0.0);
    for i in 0..BINS {
        total += weight[i];
        let score = weight[i] + 0.5 * (weight[(i + BINS - 1) % BINS] + weight[(i + 1) % BINS]);
        if score > best_score {
            best_score = score;
            best = Some(i);
        }
    }
    let best = match best {
        Some(b) if total >= 0.015 * samples.max(1) as f64 => b,
        _ => {
            if plain == 0 {
                return (0, 0, 0);
            }
            let n = plain as f64;
            return (to_byte(pr / n), to_byte(pg / n), to_byte(pb / n));
        }
    };
    let (mut wr, mut wg, mut wb, mut ww) = (0.0, 0.0, 0.0, 0.0);
    for d in [BINS - 1, 0, 1] {
        let i = (best + d) % BINS;
        wr += sum_r[i];
        wg += sum_g[i];
        wb += sum_b[i];
        ww += weight[i];
    }
    (to_byte(wr / ww), to_byte(wg / ww), to_byte(wb / ww))
}

/// Участок кадра 128×128 под каждой клавишей: схема клавиатуры растянута на весь кадр,
/// у клавиши берём и половину промежутков вокруг неё, чтобы в подсветку попала вся картинка.
fn sample_rects() -> Vec<(usize, usize, usize, usize)> {
    let min_x = KEYS.iter().map(|k| k.x).fold(f64::INFINITY, f64::min);
    let max_x = KEYS.iter().map(|k| k.x + k.w).fold(f64::NEG_INFINITY, f64::max);
    let min_y = KEYS.iter().map(|k| k.y).fold(f64::INFINITY, f64::min);
    let max_y = KEYS.iter().map(|k| k.y + k.h).fold(f64::NEG_INFINITY, f64::max);
    let size = protocol::SCREEN_SIZE as i64;
    const PAD: f64 = 5.0;
    let map = |v: f64, min: f64, max: f64| (((v - min) / (max - min)).clamp(0.0, 1.0) * size as f64).round_ties_even() as i64;
    KEYS.iter()
        .map(|k| {
            let x0 = map(k.x - PAD, min_x, max_x);
            let x1 = (x0 + 1).max(map(k.x + k.w + PAD, min_x, max_x));
            let y0 = map(k.y - PAD, min_y, max_y);
            let y1 = (y0 + 1).max(map(k.y + k.h + PAD, min_y, max_y));
            (x0.min(size - 1) as usize, y0.min(size - 1) as usize, x1.min(size) as usize, y1.min(size) as usize)
        })
        .collect()
}

/// Расстояние каждой клавиши от экранчика, 0…1.
fn distances() -> Vec<f64> {
    let cx = keypaint::SCREEN_X + keypaint::SCREEN_SIZE / 2.0;
    let cy = keypaint::SCREEN_Y + keypaint::SCREEN_SIZE / 2.0;
    let d: Vec<f64> = KEYS
        .iter()
        .map(|k| {
            let dx = k.x + k.w / 2.0 - cx;
            let dy = k.y + k.h / 2.0 - cy;
            (dx * dx + dy * dy).sqrt()
        })
        .collect();
    let max = d.iter().copied().fold(1.0f64, f64::max);
    d.into_iter().map(|v| v / max).collect()
}

/// Средний цвет участка: складываем в линейной яркости, как смешивает свет глаз.
fn average_color(t: &Tables, px: &[u8], rect: (usize, usize, usize, usize)) -> Rgb {
    let size = protocol::SCREEN_SIZE;
    let (mut r, mut g, mut b) = (0.0, 0.0, 0.0);
    let mut n = 0usize;
    for y in rect.1..rect.3 {
        for x in rect.0..rect.2 {
            let i = (y * size + x) * 2;
            if i + 1 >= px.len() {
                continue;
            }
            let value = rgb565_at(px, i);
            r += t.l5[usize::from((value >> 11) & 0x1F)];
            g += t.l6[usize::from((value >> 5) & 0x3F)];
            b += t.l5[usize::from(value & 0x1F)];
            n += 1;
        }
    }
    if n == 0 {
        return (0, 0, 0);
    }
    let n = n as f64;
    (to_byte((r / n).powf(1.0 / 2.2)), to_byte((g / n).powf(1.0 / 2.2)), to_byte((b / n).powf(1.0 / 2.2)))
}

fn smooth(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn store(target: &mut [u8], at: usize, c: Rgb) {
    target[at] = c.0;
    target[at + 1] = c.1;
    target[at + 2] = c.2;
}

fn mix(source: &[u8], a: usize, b: usize, k: f64, scale: f64) -> Rgb {
    let ch = |i: usize| {
        let x = f64::from(source[a + i]);
        let y = f64::from(source[b + i]);
        ((x + (y - x) * k) * scale).round_ties_even().clamp(0.0, 255.0) as u8
    };
    (ch(0), ch(1), ch(2))
}

fn put(frame: &mut [u8], key: u8, c: Rgb) {
    let o = usize::from(key) * 4;
    frame[o] = key;
    frame[o + 1] = c.0;
    frame[o + 2] = c.1;
    frame[o + 3] = c.2;
}

pub struct GifLight {
    style: i32,
    /// [кадр][клавиша][R,G,B] — уже яркость светодиода (линейная).
    key_led: Vec<u8>,
    /// [кадр][R,G,B].
    dominant_led: Vec<u8>,
    /// Начало кадра в цикле гифки и сколько экран его показывает, мс.
    start: Vec<i32>,
    length: Vec<i32>,
    /// 0 — у экранчика, 1 — самая дальняя клавиша.
    distance: Vec<f64>,
    pub loop_ms: i32,
    /// Общий цвет гифки — им клавиатура светит сама, когда программа не шлёт кадры.
    pub fallback: Rgb,
}

impl GifLight {
    pub fn new(frames: &[ScreenFrame], style: i32) -> Result<Self, String> {
        if frames.is_empty() {
            return Err("Нет кадров".into());
        }
        let style = if style == PICTURE { PICTURE } else { RIPPLE };
        let count = frames.len().min(protocol::MAX_FRAMES);
        let keys = KEYS.len();
        let t = Tables::new();
        let rects = (style == PICTURE).then(sample_rects);

        let mut start = vec![0; count];
        let mut length = vec![0; count];
        let mut dominant_led = vec![0u8; count * 3];
        let mut key_led = if style == PICTURE { vec![0u8; count * keys * 3] } else { Vec::new() };
        let (mut sr, mut sg, mut sb) = (0.0, 0.0, 0.0);
        let mut acc = 0i32;

        for (f, frame) in frames.iter().take(count).enumerate() {
            // Экран держит кадр столько, сколько записано в заголовке: единицы по 2 мс, 1…255.
            let delay = (frame.delay_ms / 2).max(1).clamp(1, 255) * 2;
            start[f] = acc;
            length[f] = delay.max(20);
            acc += length[f];

            let px = &frame.rgb565;
            let dom = dominant(px);
            sr += f64::from(dom.0);
            sg += f64::from(dom.1);
            sb += f64::from(dom.2);
            store(&mut dominant_led, f * 3, to_led(vivid(dom, 0.5, 0.72)));

            if let Some(rects) = &rects {
                // Тёмные участки картинки не гасим совсем: там тускло светится главный цвет кадра.
                let glow = to_led(with_value(vivid(dom, 0.5, 0.0), 0.30));
                for (k, rect) in rects.iter().enumerate() {
                    let led = to_led(vivid(average_color(&t, px, *rect), 0.35, 0.0));
                    let led = (led.0.max(glow.0), led.1.max(glow.1), led.2.max(glow.2));
                    store(&mut key_led, (f * keys + k) * 3, led);
                }
            }
        }
        let n = count as f64;
        let fallback = vivid(((sr / n) as u8, (sg / n) as u8, (sb / n) as u8), 0.5, 0.72);
        Ok(Self { style, key_led, dominant_led, start, length, distance: distances(), loop_ms: acc.max(1), fallback })
    }

    pub fn frame_count(&self) -> usize {
        self.start.len()
    }

    /// Кадр гифки в момент ms: текущий, следующий и доля перехода (вторая половина кадра, ≤ 400 мс).
    fn frame_at(&self, ms: f64) -> (usize, usize, f64, f64, i32) {
        let lp = f64::from(self.loop_ms);
        let mut at = ms % lp;
        if at < 0.0 {
            at += lp;
        }
        let i = match self.start.binary_search(&(at as i32)) {
            Ok(i) => i as isize,
            Err(ins) => ins as isize - 1,
        };
        let i = i.clamp(0, self.start.len() as isize - 1) as usize;
        let since = at - f64::from(self.start[i]);
        let length = self.length[i];
        let len = f64::from(length);
        let fade = (len * 0.5).min(400.0);
        let k = if since <= len - fade { 0.0 } else { smooth((since - (len - fade)) / fade) };
        (i, (i + 1) % self.start.len(), k, since, length)
    }

    /// Таблица цветов (576 байт, клавиша по смещению код×4) на момент seconds.
    pub fn build(&self, seconds: f64, brightness: i32, speed: i32) -> Vec<u8> {
        let scale = f64::from(brightness.clamp(0, 5)) / 5.0;
        let mut frame = vec![0u8; 9 * 64];
        let n = frame.len();
        frame[n - 2] = 0xAA;
        frame[n - 1] = 0x55;
        let ms = seconds * 1000.0;
        let keys = KEYS.len();

        if self.style == PICTURE {
            let (a, b, k, _, _) = self.frame_at(ms);
            for (key, cap) in KEYS.iter().enumerate() {
                put(&mut frame, cap.code, mix(&self.key_led, (a * keys + key) * 3, (b * keys + key) * 3, k, scale));
            }
            return frame;
        }

        // Волна: клавиша на расстоянии d от экрана показывает то, что экран показывал d·travel назад.
        let travel = (1.6 - 0.2 * f64::from(speed.clamp(0, 5))) * 1000.0;
        for (key, cap) in KEYS.iter().enumerate() {
            let d = self.distance[key];
            let (a, b, k, since, length) = self.frame_at(ms - d * travel);
            // У медленных гифок на каждой смене кадра от экрана расходится светлое кольцо.
            let flash = ((f64::from(length) - 120.0) / 300.0).clamp(0.0, 1.0) * (-since / 200.0).exp();
            let level = scale * (1.0 - 0.3 * d) * (0.82 + 0.18 * flash);
            put(&mut frame, cap.code, mix(&self.dominant_led, a * 3, b * 3, k, level));
        }
        frame
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(lo: u8, hi: u8, delay_ms: i32) -> ScreenFrame {
        let n = protocol::SCREEN_SIZE * protocol::SCREEN_SIZE;
        ScreenFrame { rgb565: [lo, hi].repeat(n), delay_ms }
    }

    #[test]
    fn dominant_of_solid_colors() {
        assert_eq!(dominant(&solid(0x00, 0xF8, 100).rgb565), (255, 0, 0)); // красный RGB565 0xF800
        assert_eq!(dominant(&solid(0x1F, 0x00, 100).rgb565), (0, 0, 255)); // синий 0x001F
        assert_eq!(dominant(&solid(0x00, 0x00, 100).rgb565), (0, 0, 0)); // чёрный — фон
    }

    #[test]
    fn build_both_styles() {
        let frames = vec![solid(0x00, 0xF8, 100), solid(0x1F, 0x00, 300)];
        for style in [PICTURE, RIPPLE] {
            let g = GifLight::new(&frames, style).unwrap();
            assert_eq!(g.loop_ms, 400);
            let f = g.build(0.05, 5, 3);
            assert_eq!(f.len(), 576);
            assert_eq!((f[574], f[575]), (0xAA, 0x55));
            for k in KEYS {
                assert_eq!(f[usize::from(k.code) * 4], k.code);
            }
        }
        // «Картинка» в начале первого кадра — красный на всех клавишах.
        let g = GifLight::new(&frames, PICTURE).unwrap();
        let f = g.build(0.0, 5, 3);
        let at = usize::from(KEYS[0].code) * 4;
        assert!(f[at + 1] > 200 && f[at + 3] < 30);
    }

    #[test]
    fn frame_at_wraps() {
        let frames = vec![solid(0x00, 0xF8, 100), solid(0x1F, 0x00, 300)];
        let g = GifLight::new(&frames, RIPPLE).unwrap();
        assert_eq!(g.frame_at(50.0).0, 0);
        assert_eq!(g.frame_at(150.0).0, 1);
        assert_eq!(g.frame_at(-50.0).0, 1);
        assert_eq!(g.frame_at(450.0).0, 0);
    }
}
