//! Порт LightMirror.cs: что сейчас горит на клавиатуре.
//! Клавиатура (кабель или донгл) записывает сюда всё, что отправила: встроенный эффект прошивки,
//! свою раскраску, живые кадры. Вентиляторы (ARGB, режим «Как клавиатура») и схема клавиш читают
//! отсюда цвета. Состояние — неизменяемые снимки под Mutex, подменяются целиком.

use std::collections::BTreeMap;
use std::f64::consts::PI;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use crate::keypaint;
use crate::Rgb;

/// Живые кадры перестали приходить — клавиатура сама вернулась к сохранённому свету.
const LIVE_TIMEOUT_MS: u64 = 1000;
/// С какого цвета клавиатура начинает «Спектр» после включения эффекта (240° — синий).
const SPECTRUM_START_HUE: f64 = 240.0;

#[derive(Clone, Debug, PartialEq)]
pub struct EffectState {
    pub mode: i32,
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub rainbow: bool,
    pub brightness: i32,
    pub speed: i32,
    pub direction: i32,
    since_ms: u64,
}

#[derive(Default)]
struct State {
    effect: Option<EffectState>,
    user: Option<Arc<Vec<u8>>>,
    live: Option<Arc<Vec<u8>>>,
    live_at: u64,
    rates: BTreeMap<String, f64>,
}

fn state() -> &'static Mutex<State> {
    static S: OnceLock<Mutex<State>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(State::default()))
}

fn lock() -> std::sync::MutexGuard<'static, State> {
    state().lock().unwrap_or_else(|e| e.into_inner())
}

/// Монотонные миллисекунды с запуска программы (Clock.Ms в C#).
pub fn now_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

fn live_fresh(s: &State) -> bool {
    s.live.is_some() && now_ms().saturating_sub(s.live_at) < LIVE_TIMEOUT_MS
}

/// Отправлен встроенный эффект прошивки. `restart` — клавиатура точно начала анимацию заново.
#[allow(clippy::too_many_arguments)]
pub fn effect(mode: i32, r: u8, g: u8, b: u8, rainbow: bool, brightness: i32, speed: i32, direction: i32, restart: bool) {
    let speed = speed.clamp(0, 5);
    let direction = direction.clamp(0, 3);
    let mut s = lock();
    // Тот же эффект с другим цветом — анимация продолжается, а не начинается заново.
    let since_ms = match &s.effect {
        Some(old) if !restart && old.mode == mode && old.speed == speed && old.direction == direction => old.since_ms,
        _ => now_ms(),
    };
    s.effect = Some(EffectState { mode, r, g, b, rainbow, brightness: brightness.clamp(0, 5), speed, direction, since_ms });
    s.user = None;
    s.live = None;
}

/// Сохранена своя раскраска (таблица [код, R, G, B] по смещению код×4).
pub fn user_light(bank: &[u8]) {
    let mut s = lock();
    s.user = Some(Arc::new(bank.to_vec()));
    s.live = None;
}

/// Ушёл живой кадр: записи [код, R, G, B] подряд.
pub fn live(frame: &[u8]) {
    let mut s = lock();
    s.live = Some(Arc::new(frame.to_vec()));
    s.live_at = now_ms();
}

/// Сейчас горит встроенный эффект прошивки (не своя раскраска и не живые кадры) — его номер.
pub fn active_effect_mode() -> Option<i32> {
    let s = lock();
    if live_fresh(&s) || s.user.is_some() {
        return None;
    }
    s.effect.as_ref().map(|e| e.mode)
}

/// Режим и скорость горящего эффекта прошивки (для подгонки скорости).
pub fn active_effect() -> Option<(i32, i32)> {
    let s = lock();
    if live_fresh(&s) || s.user.is_some() {
        return None;
    }
    s.effect.as_ref().map(|e| (e.mode, e.speed))
}

/// Цвета клавиш, как их видно глазом: таблица 576 байт ([код, R, G, B] по смещению код×4).
/// None — программа ещё ничего не отправляла. Второе значение — true, если это эффект прошивки
/// (его кадры рисует сама клавиатура, здесь — похожая картинка).
pub fn display() -> Option<(Vec<u8>, bool)> {
    let (live, user, effect, rates) = {
        let s = lock();
        (if live_fresh(&s) { s.live.clone() } else { None }, s.user.clone(), s.effect.clone(), s.rates.clone())
    };
    if let Some(l) = live {
        return Some((from_live(&l), false));
    }
    if let Some(u) = user {
        return Some((from_bank(&u), false));
    }
    let e = effect?;
    let approximate = !matches!(e.mode, 0 | 1 | 3) && e.brightness > 0;
    Some((simulate(&e, &rates), approximate))
}

/// Цвет эффекта прошивки в точке (x, y — 0…1), как его видно глазом. None — эффекта прошивки сейчас нет.
pub fn effect_sample(x: f64, y: f64, seed: f64) -> Option<Rgb> {
    let (e, rates) = {
        let s = lock();
        if live_fresh(&s) || s.user.is_some() {
            return None;
        }
        (s.effect.clone()?, s.rates.clone())
    };
    Some(effect_at(&e, &rates, x, y, seed))
}

/// Цвет клавиши `code` по таблице из [`display`].
pub fn bank_color(bank: &[u8], code: u8) -> Option<Rgb> {
    let at = code as usize * 4;
    (at + 3 < bank.len() && bank[at] == code).then(|| (bank[at + 1], bank[at + 2], bank[at + 3]))
}

// ---- подгонка скорости под настоящую клавиатуру ----

fn default_rate(mode: i32) -> f64 {
    if mode == 16 { 0.92 } else { 1.0 }
}

fn rate_for(rates: &BTreeMap<String, f64>, mode: i32, speed: i32) -> f64 {
    rates.get(&format!("{mode}:{speed}")).copied().unwrap_or_else(|| default_rate(mode))
}

pub fn set_rates(rates: Option<&BTreeMap<String, f64>>) {
    lock().rates = rates.cloned().unwrap_or_default();
}

pub fn rates() -> BTreeMap<String, f64> {
    lock().rates.clone()
}

pub fn rate_of(mode: i32, speed: i32) -> f64 {
    rate_for(&lock().rates, mode, speed)
}

/// Ручная подгонка множителя для «режим:скорость»; фаза сохраняется, цвета не прыгают.
pub fn set_rate(mode: i32, speed: i32, rate: f64) {
    let rate = rate.clamp(0.1, 10.0);
    let mut s = lock();
    let old = rate_for(&s.rates, mode, speed);
    let key = format!("{mode}:{speed}");
    if (rate - default_rate(mode)).abs() < 0.0005 {
        s.rates.remove(&key);
    } else {
        s.rates.insert(key, rate);
    }
    if let Some(e) = s.effect.as_mut() {
        if e.mode == mode && e.speed == speed && (old - rate).abs() > 0.0001 {
            let now = now_ms();
            let elapsed = now.saturating_sub(e.since_ms) as f64 * old / rate;
            e.since_ms = now.saturating_sub(elapsed.round() as u64);
        }
    }
}

/// Сколько «фаз» занимает один круг эффекта в одной точке. None — эффект сам не повторяется.
fn circle_phase(mode: i32) -> Option<f64> {
    Some(match mode {
        8 => 360.0 / 140.0,
        6 => 360.0 / 60.0,
        9 | 10 | 11 | 12 | 16 | 18 => 360.0 / 120.0,
        4 => 1.0 / 1.7,
        5 => 1.0 / 1.4,
        7 => 1.0 / 0.8,
        17 => 1.0 / 1.3,
        19 => 2.0 / 0.8,
        _ => return None,
    })
}

pub fn can_calibrate(mode: i32) -> bool {
    circle_phase(mode).is_some()
}

/// Замер: на клавиатуре прошло `circles` полных кругов за `seconds` с. Возвращает новый множитель.
pub fn calibrate(seconds: f64, circles: u32) -> Option<f64> {
    let e = lock().effect.clone()?;
    let phases = circle_phase(e.mode)?;
    if circles < 1 || seconds / circles as f64 <= 0.5 {
        return None;
    }
    let base_period = phases / (0.15 + 0.12 * e.speed as f64);
    let rate = ((base_period * circles as f64 / seconds).clamp(0.1, 10.0) * 1000.0).round() / 1000.0;
    set_rate(e.mode, e.speed, rate);
    Some(rate)
}

/// Начать круг эффекта заново (например, «Спектр» — когда клавиатура сейчас синяя).
pub fn restart_effect_clock() {
    if let Some(e) = lock().effect.as_mut() {
        e.since_ms = now_ms();
    }
}

// ---- пересчёт таблиц ----

/// Светодиод светит линейно, экран — с гаммой: обратный пересчёт, чтобы на схеме было как вживую.
fn from_led(v: u8) -> u8 {
    static T: OnceLock<[u8; 256]> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0u8; 256];
        for (v, x) in t.iter_mut().enumerate() {
            *x = (255.0 * (v as f64 / 255.0).powf(1.0 / 2.2)).round() as u8;
        }
        t
    })[v as usize]
}

fn new_bank() -> Vec<u8> {
    vec![0u8; 9 * 64]
}

fn put(bank: &mut [u8], code: u8, (r, g, b): Rgb) {
    let at = code as usize * 4;
    if at + 3 >= bank.len() {
        return;
    }
    bank[at] = code;
    bank[at + 1] = r;
    bank[at + 2] = g;
    bank[at + 3] = b;
}

fn from_live(live: &[u8]) -> Vec<u8> {
    let mut bank = new_bank();
    for rec in live.chunks_exact(4) {
        if rec[0] == 0 {
            break;
        }
        put(&mut bank, rec[0], (from_led(rec[1]), from_led(rec[2]), from_led(rec[3])));
    }
    bank
}

fn from_bank(src: &[u8]) -> Vec<u8> {
    let mut bank = new_bank();
    let n = src.len().min(bank.len());
    let mut code = 1usize;
    while code * 4 + 3 < n {
        let at = code * 4;
        if src[at] as usize == code {
            put(&mut bank, code as u8, (from_led(src[at + 1]), from_led(src[at + 2]), from_led(src[at + 3])));
        }
        code += 1;
    }
    bank
}

fn simulate(e: &EffectState, rates: &BTreeMap<String, f64>) -> Vec<u8> {
    let mut bank = new_bank();
    for key in keypaint::KEYS {
        let x = (key.x + key.w / 2.0 - keypaint::ORIGIN_X) / keypaint::WIDTH;
        let y = (key.y + key.h / 2.0 - keypaint::ORIGIN_Y) / keypaint::HEIGHT;
        put(&mut bank, key.code, effect_at(e, rates, x, y, hash(key.code)));
    }
    bank
}

fn wave(t: f64) -> f64 {
    0.5 + 0.5 * (2.0 * PI * t).cos()
}

fn hash(code: u8) -> f64 {
    (code as f64 * 0.618_033_988_75) % 1.0
}

fn along(x: f64, y: f64, direction: i32) -> f64 {
    match direction {
        1 => 1.0 - x,
        2 => y,
        3 => 1.0 - y,
        _ => x,
    }
}

fn radius(x: f64, y: f64) -> f64 {
    let dx = (x - 0.5) * 2.0;
    let dy = (y - 0.5) * 0.9;
    (dx * dx + dy * dy).sqrt()
}

fn hue(h: f64) -> Rgb {
    let h = (h % 360.0 + 360.0) % 360.0;
    let x = 1.0 - ((h / 60.0) % 2.0 - 1.0).abs();
    let (r, g, b) = if h < 60.0 {
        (1.0, x, 0.0)
    } else if h < 120.0 {
        (x, 1.0, 0.0)
    } else if h < 180.0 {
        (0.0, 1.0, x)
    } else if h < 240.0 {
        (0.0, x, 1.0)
    } else if h < 300.0 {
        (x, 0.0, 1.0)
    } else {
        (1.0, 0.0, x)
    };
    ((r * 255.0_f64).round() as u8, (g * 255.0_f64).round() as u8, (b * 255.0_f64).round() as u8)
}

fn effect_at(e: &EffectState, rates: &BTreeMap<String, f64>, x: f64, y: f64, seed: f64) -> Rgb {
    let seconds = now_ms().saturating_sub(e.since_ms) as f64 / 1000.0;
    effect_at_time(e, rate_for(rates, e.mode, e.speed), seconds, x, y, seed)
}

/// Похожая картинка эффекта прошивки (сами кадры рисует клавиатура).
fn effect_at_time(e: &EffectState, rate: f64, seconds: f64, x: f64, y: f64, seed: f64) -> Rgb {
    let phase = seconds * (0.15 + 0.12 * e.speed as f64) * rate;
    let level = if e.mode == 0 { 0.0 } else { (e.brightness as f64 / 5.0).powf(1.0 / 2.2) };
    let rainbow = e.rainbow || matches!(e.mode, 6 | 8);
    let d = along(x, y, e.direction);
    let mut k = 1.0;
    let mut h = x * 300.0 - phase * 90.0;
    match e.mode {
        0 => k = 0.0,                                  // выключено
        2 | 13 | 14 | 15 => k = 0.12,                  // горят от нажатий — в покое почти темно
        4 => k = 0.2 + 0.8 * wave(phase * 1.7 + seed).powi(3), // мерцание
        5 => k = 0.12 + 0.88 * wave(y * 1.2 - phase * 1.4 + seed * 0.35).powi(4), // падение
        6 => h = x * 360.0 + y * 60.0 - phase * 60.0,  // разноцветный
        7 => k = 0.06 + 0.94 * wave(phase * 0.8),      // дыхание
        8 => h = SPECTRUM_START_HUE - phase * 140.0,   // спектр
        9 => {
            // волна от центра
            let r = radius(x, y);
            k = 0.2 + 0.8 * wave(r * 1.5 - phase);
            h = r * 300.0 - phase * 120.0;
        }
        10 | 16 => {
            // прокрутка, течение
            k = if e.mode == 10 { 0.25 + 0.75 * wave(d - phase) } else { 0.55 + 0.45 * wave(d - phase) };
            h = d * 360.0 - phase * 120.0;
        }
        11 | 18 => {
            // перекат, наклон — диагональные полосы
            let t = x * 0.75 + y * 0.45;
            k = 0.25 + 0.75 * wave(t * 1.3 - phase);
            h = t * 300.0 - phase * 120.0;
        }
        12 => {
            // вращение вокруг центра
            let angle = ((y - 0.5) * 0.45).atan2(x - 0.5) / (2.0 * PI);
            k = 0.25 + 0.75 * wave(angle - phase);
            h = angle * 360.0 - phase * 120.0;
        }
        17 => k = 0.15 + 0.85 * wave(phase * 1.3).powi(2), // пульсация
        19 => {
            // шаттл — полоса ходит туда-обратно
            let at = 1.0 - ((phase * 0.8) % 2.0 - 1.0).abs();
            k = (1.0 - (d - at).abs() * 4.0).max(0.12);
        }
        _ => h = x * 300.0, // статичный цвет и прочие ровные режимы
    }
    let (r0, g0, b0) = if rainbow { hue(h) } else { (e.r, e.g, e.b) };
    let scale = level * k.clamp(0.0, 1.0);
    (
        (r0 as f64 * scale).round() as u8,
        (g0 as f64 * scale).round() as u8,
        (b0 as f64 * scale).round() as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(mode: i32, brightness: i32, rainbow: bool) -> EffectState {
        EffectState { mode, r: 255, g: 0, b: 0, rainbow, brightness, speed: 2, direction: 0, since_ms: 0 }
    }

    #[test]
    fn off_is_black_static_is_color() {
        assert_eq!(effect_at_time(&st(0, 5, false), 1.0, 1.0, 0.5, 0.5, 0.0), (0, 0, 0));
        assert_eq!(effect_at_time(&st(1, 5, false), 1.0, 1.0, 0.5, 0.5, 0.0), (255, 0, 0));
    }

    #[test]
    fn spectrum_starts_blue() {
        assert_eq!(effect_at_time(&st(8, 5, false), 1.0, 0.0, 0.3, 0.3, 0.0), (0, 0, 255));
    }

    #[test]
    fn gamma_table_ends() {
        assert_eq!(from_led(0), 0);
        assert_eq!(from_led(255), 255);
    }

    #[test]
    fn bank_roundtrip() {
        let mut bank = new_bank();
        put(&mut bank, 5, (1, 2, 3));
        assert_eq!(bank_color(&bank, 5), Some((1, 2, 3)));
        assert_eq!(bank_color(&bank, 6), None);
    }
}
