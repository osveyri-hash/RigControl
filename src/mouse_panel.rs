//! Страница «Мышь» в духе M HUB (MainWindow.Mouse*.cs): шапка с моделью, зарядом, связью и встроенными
//! профилями 1–3, вкладки «Настройка клавиш», «DPI», «Производительность», «Другие».
//! Работает поверх `mouse` и `mousekeys` — те же команды, что в CLI. Запись идёт в фоновом потоке,
//! после неё настройки перечитываются из мыши; как в M HUB, всё применяется сразу, без кнопки «Записать».

use std::sync::mpsc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, TextureHandle, Vec2};

use crate::mouse::{McMouse, MouseConfig, MouseKey, MouseStatus};
use crate::mousekeys;
use crate::ui_kit as kit;

/// Всё, что читается из мыши при обновлении (собирается в фоновом потоке).
struct Snap {
    max_dpi: u16,
    rates: Vec<u32>,
    wireless: bool,
    head: String,
    battery: String,
    /// Заряд и «заряжается», если мышь на связи.
    charge: Option<(u8, bool)>,
    cfg: Option<MouseConfig>,
    note: String,
    fw_mouse: Option<String>,
    fw_receiver: Option<String>,
}

fn read_all() -> Result<Snap, String> {
    let (m, st) = open()?;
    // Статус уже пришёл при поиске; «свежий» (с паузой 300 мс) просим только если его нет.
    let st = st.or_else(|| m.read_status_fresh());
    let (cfg, note) = match m.read_config() {
        Some(c) if c.valid => (Some(c), String::new()),
        Some(c) => (None, format!(" · настройки выглядят странно (raw {})", c.raw)),
        None => (None, " · настройки не прочитались (пошевелите мышью, закройте M HUB)".to_string()),
    };
    Ok(Snap {
        max_dpi: m.max_dpi(st.as_ref()),
        rates: m.rates().to_vec(),
        wireless: m.via_dongle(),
        head: m.model(st.as_ref()),
        battery: battery_text(st.as_ref()),
        charge: st.as_ref().filter(|s| s.has_mouse()).map(|s| (s.battery, s.charging())),
        cfg,
        note,
        fw_mouse: m.read_version(false),
        fw_receiver: if m.via_dongle() { m.read_version(true) } else { None },
    })
}

fn open() -> Result<(McMouse, Option<MouseStatus>), String> {
    let api = hidapi::HidApi::new().map_err(|e| format!("hidapi: {e}"))?;
    match McMouse::find(&api) {
        (Some(x), _) => Ok(x),
        (None, info) => Err(info),
    }
}

/// Что показать на плитке «Мышь» и в боковой панели.
#[derive(Clone, Debug, Default)]
pub struct MouseTile {
    /// Мышь (или её донгл) найдена.
    pub found: bool,
    /// Мышь на связи и отвечает.
    pub online: bool,
    /// Идёт чтение.
    pub reading: bool,
    pub model: String,
    pub battery: Option<u8>,
    pub charging: bool,
    pub wireless: bool,
    pub profile: Option<u8>,
    /// DPI текущего уровня.
    pub dpi: Option<u16>,
    pub rate_hz: Option<u32>,
}

pub struct MousePanel {
    pub info: String,
    /// Последнее чтение: найдена ли мышь, её заряд.
    found: bool,
    charge: Option<(u8, bool)>,
    tried_at: Option<Instant>,
    head: String,
    battery: String,
    link: String,
    last_poll: Option<Instant>,
    poll_rx: Option<mpsc::Receiver<Option<MouseStatus>>>,
    load_rx: Option<mpsc::Receiver<Result<Snap, String>>>,
    cfg: Option<MouseConfig>,
    max_dpi: u16,
    rates: Vec<u32>,
    wireless: bool,
    dpi: Vec<u16>,
    level: usize,
    rate: usize,
    lod: u8,
    mode: u8,
    ripple: bool,
    angle: bool,
    motion: bool,
    rotate: i8,
    debounce: u8,
    sleep: u8,
    keys: [MouseKey; mousekeys::COUNT],
    /// DPI по X и Y всех 6 уровней, сколько уровней включено и отдельные X/Y.
    dpi_x: [u16; 6],
    dpi_y: [u16; 6],
    dpi_count: usize,
    dpi_separate: bool,
    fw_mouse: Option<String>,
    fw_receiver: Option<String>,
    /// Вкладка: 0 — кнопки, 1 — DPI, 2 — производительность, 3 — другие.
    tab: u8,
    /// Кнопка, выбранная на схеме, поиск функции и раздел списка (0 — «Система», 1 — «Клавиатура»).
    sel_key: usize,
    key_search: String,
    key_cat: u8,
    /// «Клавиатура»: ждём нажатия сочетания и что набрано.
    listening: bool,
    shortcut: String,
    /// Последний ненулевой сон — чтобы снять «Отключить спящий режим» без потери значения.
    sleep_last: u8,
    /// «Заводские настройки»: первое нажатие спрашивает, второе сбрасывает.
    reset_armed: Option<Instant>,
    /// Запись в мышь в фоне: (что сделали, итог записи, перечитанные настройки).
    op_rx: Option<mpsc::Receiver<(String, Result<(), String>, Result<Snap, String>)>>,
    picture: Option<TextureHandle>,
}

impl Default for MousePanel {
    fn default() -> Self {
        Self::new()
    }
}

impl MousePanel {
    pub fn new() -> Self {
        Self {
            info: "Нажмите «Обновить».".into(),
            found: false,
            charge: None,
            tried_at: None,
            head: String::new(),
            battery: String::new(),
            link: String::new(),
            last_poll: None,
            poll_rx: None,
            load_rx: None,
            cfg: None,
            max_dpi: 26000,
            rates: Vec::new(),
            wireless: false,
            dpi: Vec::new(),
            level: 0,
            rate: 0,
            lod: 0,
            mode: 0,
            ripple: false,
            angle: false,
            motion: false,
            rotate: 0,
            debounce: 0,
            sleep: 0,
            keys: [MouseKey::default(); mousekeys::COUNT],
            dpi_x: [400, 800, 1600, 3200, 6400, 12800],
            dpi_y: [400, 800, 1600, 3200, 6400, 12800],
            dpi_count: 6,
            dpi_separate: false,
            fw_mouse: None,
            fw_receiver: None,
            tab: 0,
            sel_key: 0,
            key_search: String::new(),
            key_cat: 0,
            listening: false,
            shortcut: String::new(),
            sleep_last: 3,
            reset_armed: None,
            op_rx: None,
            picture: None,
        }
    }

    /// Найти мышь и прочитать настройки (синхронно — после команд, когда окно и так ждёт).
    pub fn refresh(&mut self) {
        let r = read_all();
        self.apply(r);
    }

    /// То же самое, но в фоновом потоке: вкладка открывается сразу, данные подтягиваются следом.
    pub fn refresh_async(&mut self, ctx: &egui::Context) {
        if self.load_rx.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(read_all());
            ctx.request_repaint();
        });
        self.load_rx = Some(rx);
        self.tried_at = Some(Instant::now());
        self.info = "Читаю мышь…".into();
    }

    /// Забрать результат фонового чтения, если он готов.
    fn pump(&mut self) -> bool {
        let Some(rx) = &self.load_rx else { return false };
        match rx.try_recv() {
            Ok(r) => {
                self.load_rx = None;
                self.apply(r);
                false
            }
            Err(mpsc::TryRecvError::Empty) => true,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.load_rx = None;
                false
            }
        }
    }

    /// Для главной и боковой панели: мышь не прочитана — читать раз в 15 с, прочитана — обновлять заряд.
    pub fn keep_fresh(&mut self, ctx: &egui::Context) {
        if self.pump() {
            return;
        }
        if self.cfg.is_none() {
            let due = self.tried_at.map_or(true, |t| t.elapsed() >= Duration::from_secs(15));
            if due {
                self.refresh_async(ctx);
            }
        } else {
            self.poll_battery(ctx);
        }
    }

    pub fn tile(&self) -> MouseTile {
        let cfg = self.cfg.as_ref();
        let dpi = cfg.and_then(|c| {
            let level = if self.wireless { c.g_dpi } else { c.usb_dpi } as usize;
            let n = (c.count as usize).clamp(1, 6);
            c.dpi.get(level.min(n - 1)).copied()
        });
        let rate_hz = cfg.and_then(|c| {
            let i = if self.wireless { c.g_rate } else { c.usb_rate } as usize;
            self.rates.get(i).copied()
        });
        MouseTile {
            found: self.found,
            online: self.charge.is_some(),
            reading: self.load_rx.is_some(),
            model: if self.head.is_empty() { "Мышь".to_string() } else { self.head.clone() },
            battery: self.charge.map(|l| l.0),
            charging: self.charge.is_some_and(|l| l.1),
            wireless: self.wireless,
            profile: cfg.map(|c| c.profile.min(2)),
            dpi,
            rate_hz,
        }
    }

    fn apply(&mut self, r: Result<Snap, String>) {
        match r {
            Ok(s) => {
                self.found = true;
                self.charge = s.charge;
                self.max_dpi = s.max_dpi;
                self.rates = s.rates;
                self.wireless = s.wireless;
                self.head = s.head;
                self.battery = s.battery;
                self.link = if self.wireless { "через донгл" } else { "по кабелю" }.into();
                self.last_poll = Some(Instant::now());
                self.compose();
                self.info += &s.note;
                self.fw_mouse = s.fw_mouse;
                self.fw_receiver = s.fw_receiver;
                if let Some(c) = &s.cfg {
                    self.load(c);
                }
                self.cfg = s.cfg;
            }
            Err(e) => {
                self.info = e;
                self.found = false;
                self.charge = None;
                self.cfg = None;
            }
        }
    }

    fn compose(&mut self) {
        self.info = format!("{}{} · {}", self.head, self.battery, self.link);
    }

    /// Заряд обновляется сам раз в 15 с (как в C#), в фоновом потоке — окно не подвисает.
    fn poll_battery(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.poll_rx {
            match rx.try_recv() {
                Ok(st) => {
                    self.poll_rx = None;
                    self.last_poll = Some(Instant::now());
                    if let Some(st) = st {
                        self.battery = battery_text(Some(&st));
                        self.charge = st.has_mouse().then(|| (st.battery, st.charging()));
                        self.compose();
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => self.poll_rx = None,
            }
        }
        let due = self.last_poll.map(|t| t.elapsed() >= Duration::from_secs(15)).unwrap_or(false);
        if due && self.poll_rx.is_none() && self.cfg.is_some() {
            let (tx, rx) = mpsc::channel();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let st = open().ok().and_then(|(m, _)| m.read_status_fresh());
                let _ = tx.send(st);
                ctx.request_repaint();
            });
            self.poll_rx = Some(rx);
        }
        ctx.request_repaint_after(Duration::from_secs(5));
    }

    fn load(&mut self, c: &MouseConfig) {
        let n = (c.count as usize).clamp(1, 6);
        self.dpi = c.dpi[..n].to_vec();
        self.dpi_x = c.dpi;
        self.dpi_y = c.dpi_y;
        self.dpi_count = n;
        self.dpi_separate = (0..n).any(|i| c.dpi[i] != c.dpi_y[i]);
        if c.sleep > 0 {
            self.sleep_last = c.sleep;
        }
        self.level = (if self.wireless { c.g_dpi } else { c.usb_dpi } as usize).min(n - 1);
        self.rate = if self.wireless { c.g_rate } else { c.usb_rate } as usize;
        self.lod = c.lod();
        self.mode = c.mode();
        self.ripple = c.ripple();
        self.angle = c.angle_snap();
        self.motion = c.motion_sync();
        self.rotate = c.rotate.clamp(-30, 30);
        self.debounce = c.debounce.min(20);
        self.sleep = c.sleep;
        for b in 0..mousekeys::COUNT {
            self.keys[b] = c.keys[b];
        }
    }

    /// Текущий встроенный профиль мыши (0–2), если настройки уже прочитаны.
    pub fn current_profile(&self) -> Option<u8> {
        self.cfg.as_ref().map(|c| c.profile.min(2))
    }

    /// ApplyProfileAsync, часть мыши (MainWindow.Profiles.cs): переключить встроенный
    /// профиль мыши, если он отличается от текущего. Возвращает текст для заметки профиля.
    pub fn switch_to(&mut self, index: u8) -> String {
        let index = index.min(2);
        if self.cfg.as_ref().map(|c| c.profile) == Some(index) {
            return format!("мышь уже на профиле {}", index + 1);
        }
        match open().and_then(|(m, _)| m.switch_profile(index)) {
            Ok(()) => {
                std::thread::sleep(std::time::Duration::from_millis(300));
                self.refresh();
                format!("мышь: профиль {}", index + 1)
            }
            Err(e) => format!("мышь: {e}"),
        }
    }


    /// Запись в мышь в фоновом потоке, затем перечитать настройки. Пока идёт запись, страница не принимает правки.
    fn run(&mut self, ctx: &egui::Context, what: impl Into<String>, f: impl FnOnce(&McMouse) -> Result<(), String> + Send + 'static) {
        if self.op_rx.is_some() {
            return;
        }
        let what = what.into();
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let r = open().and_then(|(m, _)| f(&m));
            std::thread::sleep(Duration::from_millis(200));
            let snap = read_all();
            let _ = tx.send((what, r, snap));
            ctx.request_repaint();
        });
        self.op_rx = Some(rx);
    }

    /// Запись, которой нужны прочитанные настройки текущего профиля.
    fn run_cfg(
        &mut self,
        ctx: &egui::Context,
        what: impl Into<String>,
        f: impl FnOnce(&McMouse, &MouseConfig) -> Result<(), String> + Send + 'static,
    ) {
        let Some(cfg) = self.cfg.clone() else { return };
        self.run(ctx, what, move |m| f(m, &fresh_config(m, cfg.profile)?));
    }

    fn busy(&self) -> bool {
        self.op_rx.is_some() || self.load_rx.is_some()
    }

    /// Профиль, настройки которого сейчас на странице.
    fn shown_profile(&self) -> Option<u8> {
        self.cfg.as_ref().map(|c| c.profile)
    }

    /// Итог фоновой записи: ошибка — сообщением, настройки — на страницу.
    fn pump_op(&mut self) -> Option<String> {
        let rx = self.op_rx.as_ref()?;
        match rx.try_recv() {
            Ok((what, r, snap)) => {
                self.op_rx = None;
                self.apply(snap);
                match r {
                    Ok(()) if what.is_empty() => None,
                    Ok(()) => Some(format!("Мышь: {what}")),
                    Err(e) => Some(format!("Мышь: {e}")),
                }
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.op_rx = None;
                None
            }
        }
    }

    fn write_sensor(&mut self, ctx: &egui::Context) {
        let onoff = |b: bool| if b { 1u8 } else { 2u8 };
        let (lod, rip, ang, mot, mode, rot) =
            (self.lod, onoff(self.ripple), onoff(self.angle), onoff(self.motion), self.mode + 1, self.rotate);
        // Все поля сенсора пишутся разом — с этой страницы, то есть из показанного профиля.
        self.run_cfg(ctx, "", move |m, _| m.write_sensor(lod, rip, ang, mot, mode, Some(rot)));
    }

    fn write_dpi(&mut self, ctx: &egui::Context) {
        let (mut x, mut y, count, level) = (self.dpi_x, self.dpi_y, self.dpi_count, self.level);
        let max = self.max_dpi;
        for i in 0..6 {
            x[i] = x[i].clamp(50, max);
            y[i] = if self.dpi_separate { y[i].clamp(50, max) } else { x[i] };
        }
        self.run_cfg(ctx, "", move |m, c| m.write_dpi(c, &x, &y, count, level.min(count - 1)));
    }

    fn assign(&mut self, ctx: &egui::Context, b: usize, key: MouseKey) -> Option<String> {
        let key = mousekeys::normalize(b, key);
        // Защита из MainWindow.MouseKeys.cs: нельзя остаться без левого клика.
        let any_left = keeps_left_click(&self.keys, b, key);
        if !any_left {
            return Some("Мышь: после этого ни одна кнопка не будет левым кликом — не записываю.".into());
        }
        let Some(profile) = self.shown_profile() else {
            return Some("Мышь: настройки ещё не прочитаны — кнопка не записана.".into());
        };
        self.run(ctx, "", move |m| {
            // Проверку повторяем по настройкам, прочитанным прямо перед записью: страница могла показывать
            // другой профиль (его переключают и кнопкой на самой мыши), а в нём левый клик — на другой кнопке.
            let fresh = fresh_config(m, profile)?;
            if !keeps_left_click(&fresh.keys, b, key) {
                return Err("после этого ни одна кнопка не будет левым кликом — не записываю".into());
            }
            m.write_key(b as u8, key)
        });
        None
    }

    /// Нарисовать страницу. Возвращает сообщение для всплывающей подсказки.
    pub fn ui(&mut self, ui: &mut egui::Ui) -> Option<String> {
        let mut msg = self.pump_op();
        let reading = self.pump();
        if self.reset_armed.is_some_and(|t| t.elapsed() > Duration::from_secs(5)) {
            self.reset_armed = None;
        }
        if self.picture.is_none() {
            self.picture = image::load_from_memory(include_bytes!("../assets/mouse_top.png")).ok().map(|img| {
                let img = img.to_rgba8();
                let ci = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
                ui.ctx().load_texture("mouse-top", ci, egui::TextureOptions::LINEAR)
            });
        }
        let ctx = ui.ctx().clone();

        if self.cfg.is_none() {
            kit::card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                if reading {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Читаю мышь…");
                    });
                    return;
                }
                ui.label(egui::RichText::new("Мышь не найдена").font(kit::semibold(17.0)).color(kit::TEXT));
                ui.add_space(4.0);
                kit::cap(ui, "Подключите донгл 2,4 ГГц или кабель и пошевелите мышью. Если открыта M HUB — закройте её: она занимает мышь. По Bluetooth настройки недоступны.");
                if !self.info.is_empty() {
                    kit::cap(ui, kit::ellipsize(&self.info, 200));
                }
                ui.add_space(10.0);
                if ui.button("Найти мышь заново").clicked() {
                    self.refresh_async(&ctx);
                }
            });
            return msg;
        }

        // Шапка: модель, заряд, связь, встроенные профили, «Найти заново».
        kit::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(self.head.clone()).font(kit::semibold(22.0)).color(kit::TEXT));
                ui.add_space(14.0);
                if let Some((p, charging)) = self.charge {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(27.0, 14.0), Sense::hover());
                    kit::battery_horizontal(ui.painter(), rect.min + egui::vec2(0.0, 1.0), p, crate::battery::LOW_PERCENT);
                    let col = if p <= crate::battery::LOW_PERCENT { kit::BAD } else { kit::TEXT };
                    ui.label(egui::RichText::new(format!("{p}%{}", if charging { " · заряжается" } else { "" })).color(col));
                    ui.add_space(10.0);
                }
                let link = if self.wireless { "2.4 ГГц" } else { "USB" };
                ui.label(kit::icon_text(if self.wireless { kit::glyph::WIRELESS } else { kit::glyph::USB }, 14.0, kit::TEXT));
                ui.label(egui::RichText::new(link).color(kit::TEXT));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_enabled(!self.busy(), egui::Button::new("Найти заново")).clicked() {
                        self.refresh_async(&ctx);
                    }
                    ui.add_space(8.0);
                    let cur = self.cfg.as_ref().map(|c| c.profile.min(2));
                    let mut pick = None;
                    for i in (0..3u8).rev() {
                        let sel = cur == Some(i);
                        if segment(ui, sel, &format!("Профиль {}", i + 1), !self.busy()).clicked() && !sel {
                            pick = Some(i);
                        }
                    }
                    kit::cap(ui, "Встроенный профиль:");
                    if let Some(i) = pick {
                        self.run(&ctx, format!("профиль {}", i + 1), move |m| {
                            m.switch_profile(i)?;
                            std::thread::sleep(Duration::from_millis(100));
                            Ok(())
                        });
                    }
                });
            });
        });
        ui.add_space(14.0);

        // Вкладки как в M HUB.
        ui.horizontal(|ui| {
            for (i, t) in ["Настройка клавиш", "DPI", "Производительность", "Другие"].into_iter().enumerate() {
                if tab_button(ui, self.tab == i as u8, t).clicked() {
                    self.tab = i as u8;
                    self.listening = false;
                }
                ui.add_space(18.0);
            }
            if self.busy() {
                ui.spinner();
            }
        });
        ui.add_space(14.0);

        let enabled = !self.busy();
        ui.add_enabled_ui(enabled, |ui| match self.tab {
            0 => {
                if let Some(m) = self.ui_keys(ui, &ctx) {
                    msg = Some(m);
                }
            }
            1 => self.ui_dpi(ui, &ctx),
            2 => self.ui_perf(ui, &ctx),
            _ => {
                if let Some(m) = self.ui_other(ui, &ctx) {
                    msg = Some(m);
                }
            }
        });
        msg
    }

    // ── Настройка клавиш ────────────────────────────────────────────────────

    fn ui_keys(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) -> Option<String> {
        let mut msg = None;
        let list_w = 340.0;
        let scheme_w = (ui.available_width() - list_w - 16.0).max(420.0);
        ui.horizontal_top(|ui| {
            // Схема мыши с подписями кнопок.
            ui.vertical(|ui| {
                ui.set_width(scheme_w);
                kit::card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    if let Some(i) = self.key_scheme(ui) {
                        self.sel_key = i;
                        self.listening = false;
                    }
                    ui.vertical_centered(|ui| {
                        if ui.button("↺ Сброс").on_hover_text("Все кнопки текущего профиля — как с завода").clicked() {
                            self.run(ctx, "кнопки как с завода", |m| m.reset_keys());
                        }
                    });
                });
            });
            ui.add_space(16.0);
            // Список функций.
            ui.vertical(|ui| {
                ui.set_width(list_w);
                kit::card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let b = self.sel_key;
                    ui.label(egui::RichText::new(mousekeys::TITLES[b]).font(kit::semibold(15.0)).color(kit::TEXT));
                    kit::cap(ui, format!("Сейчас: {}", mousekeys::describe(b, self.keys[b])));
                    ui.add_space(6.0);
                    if ui.small_button("Вернуть родную").on_hover_text("Вернуть выбранной кнопке её обычную функцию").clicked() {
                        msg = self.assign(ctx, b, MouseKey::default());
                    }
                    ui.add_space(10.0);
                    ui.add(egui::TextEdit::singleline(&mut self.key_search).hint_text("🔍 Поиск функции").desired_width(f32::INFINITY));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        for (i, t) in ["Система", "Клавиатура"].into_iter().enumerate() {
                            if tab_button(ui, self.key_cat == i as u8, t).clicked() {
                                self.key_cat = i as u8;
                                self.listening = false;
                            }
                            ui.add_space(12.0);
                        }
                    });
                    ui.add_space(6.0);
                    if self.key_cat == 0 {
                        if let Some(key) = self.action_list(ui) {
                            msg = self.assign(ctx, b, key);
                        }
                    } else if let Some(key) = self.shortcut_box(ui) {
                        msg = self.assign(ctx, b, key);
                    }
                });
            });
        });
        msg
    }

    /// Схема: картинка мыши сверху, слева ЛКМ / Вперёд / Назад, справа ПКМ / СКМ, линии к кнопкам.
    fn key_scheme(&mut self, ui: &mut egui::Ui) -> Option<usize> {
        let w = ui.available_width();
        let h = 470.0;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), Sense::hover());
        let pic_h = h - 30.0;
        let pic_w = pic_h * 320.0 / 637.0;
        let pic = Rect::from_center_size(egui::pos2(rect.center().x, rect.top() + pic_h / 2.0 + 10.0), egui::vec2(pic_w, pic_h));
        if let Some(tex) = &self.picture {
            ui.painter().image(tex.id(), pic, Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE);
        }
        let at = |fx: f32, fy: f32| egui::pos2(pic.left() + pic.width() * fx, pic.top() + pic.height() * fy);
        // (кнопка, точка на мыши, сторона: true — слева, высота подписи в долях картинки)
        let spots: [(usize, Pos2, bool, f32); 5] = [
            (mousekeys::LEFT, at(0.27, 0.22), true, 0.12),
            (mousekeys::FORWARD, at(0.02, 0.35), true, 0.36),
            (mousekeys::BACK, at(0.02, 0.52), true, 0.52),
            (mousekeys::RIGHT, at(0.73, 0.22), false, 0.12),
            (mousekeys::MIDDLE, at(0.5, 0.19), false, 0.26),
        ];
        let mut picked = None;
        for (b, point, left, fy) in spots {
            let label = mousekeys::SHORT[b];
            let value = kit::ellipsize(&mousekeys::describe(b, self.keys[b]), 18);
            let size = egui::vec2(150.0, 44.0);
            let y = pic.top() + pic.height() * fy;
            let x = if left { (pic.left() - 60.0 - size.x).max(rect.left()) } else { (pic.right() + 60.0).min(rect.right() - size.x) };
            let pill = Rect::from_min_size(egui::pos2(x, y - size.y / 2.0), size);
            let id = ui.id().with(("mouse-key", b));
            let resp = ui.interact(pill, id, Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
            let sel = self.sel_key == b;
            let painter = ui.painter();
            let edge = if left { pill.right_center() } else { pill.left_center() };
            let line = if sel { kit::TEXT } else { kit::LINE_HOVER };
            painter.line_segment([edge, point], Stroke::new(1.0_f32, line));
            painter.circle_filled(point, 4.5, if sel { kit::TEXT } else { Color32::from_gray(60) });
            painter.circle_stroke(point, 4.5, Stroke::new(1.5_f32, Color32::from_gray(30)));
            let fill = if sel { kit::SURFACE2 } else if resp.hovered() { kit::HOVER } else { kit::CARD };
            let stroke = if sel { kit::GLASS_BORDER } else { kit::LINE };
            painter.rect(pill, 22.0, fill, Stroke::new(1.0_f32, stroke));
            painter.text(pill.center_top() + egui::vec2(0.0, 13.0), Align2::CENTER_CENTER, label, kit::semibold(13.0), kit::TEXT);
            painter.text(pill.center_top() + egui::vec2(0.0, 30.0), Align2::CENTER_CENTER, value, FontId::proportional(11.0), kit::MUTED);
            if resp.clicked() {
                picked = Some(b);
            }
        }
        picked
    }

    /// Функции по группам (как список M HUB), с поиском. Возвращает выбранную.
    fn action_list(&mut self, ui: &mut egui::Ui) -> Option<MouseKey> {
        let q = self.key_search.trim().to_lowercase();
        let cur = mousekeys::normalize(self.sel_key, self.keys[self.sel_key]);
        let mut picked = None;
        egui::ScrollArea::vertical().id_salt("mouse-actions").max_height(400.0).show(ui, |ui| {
            let mut groups: Vec<&str> = Vec::new();
            for a in mousekeys::ACTIONS {
                if !groups.contains(&a.group) {
                    groups.push(a.group);
                }
            }
            for g in groups {
                let items: Vec<&mousekeys::MouseAction> = mousekeys::ACTIONS
                    .iter()
                    .filter(|a| a.group == g)
                    .filter(|a| q.is_empty() || a.name.to_lowercase().contains(&q) || a.hint.is_some_and(|h| h.to_lowercase().contains(&q)))
                    .collect();
                if items.is_empty() {
                    continue;
                }
                egui::CollapsingHeader::new(egui::RichText::new(g).font(kit::semibold(13.0)).color(kit::TEXT))
                    .id_salt(("mouse-group", g))
                    .default_open(true)
                    .show(ui, |ui| {
                        for a in items {
                            let sel = mousekeys::normalize(self.sel_key, a.key) == cur;
                            let text = match a.hint {
                                Some(h) => format!("{}   {h}", a.name),
                                None => a.name.to_string(),
                            };
                            let w = ui.available_width();
                            let r = ui.add_sized([w, 28.0], egui::SelectableLabel::new(sel, text));
                            if r.clicked() && !sel {
                                picked = Some(a.key);
                            }
                        }
                    });
            }
        });
        picked
    }

    /// «Клавиатура»: нажать сочетание (или ввести «ctrl+shift+f5») и назначить его кнопке.
    fn shortcut_box(&mut self, ui: &mut egui::Ui) -> Option<MouseKey> {
        kit::cap(ui, "Кнопка мыши будет нажимать клавишу или сочетание клавиш.");
        ui.add_space(8.0);
        if self.listening {
            let captured = ui.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Key { key, pressed: true, modifiers, .. } => key_name(*key).map(|n| {
                        let mut parts = Vec::new();
                        if modifiers.ctrl {
                            parts.push("ctrl");
                        }
                        if modifiers.shift {
                            parts.push("shift");
                        }
                        if modifiers.alt {
                            parts.push("alt");
                        }
                        let mut s = parts.join("+");
                        if !s.is_empty() {
                            s.push('+');
                        }
                        s + &n
                    }),
                    _ => None,
                })
            });
            if let Some(s) = captured {
                self.shortcut = s;
                self.listening = false;
            }
            let w = ui.available_width();
            if ui.add_sized([w, 40.0], egui::Button::new("Нажмите сочетание на клавиатуре… (щелчок — отмена)")).clicked() {
                self.listening = false;
            }
            ui.ctx().request_repaint();
        } else {
            let w = ui.available_width();
            if ui.add_sized([w, 40.0], egui::Button::new("⌨  Записать сочетание")).clicked() {
                self.listening = true;
            }
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.shortcut).hint_text("ctrl+shift+f5").desired_width(190.0));
        });
        kit::cap(ui, "Можно и вручную: ctrl, shift, alt, win и клавиша — a…z, 0…9, f1…f24, enter, esc, tab, space, del, home, pgup…");
        ui.add_space(10.0);
        let mut picked = None;
        let ok = mousekeys::parse_shortcut(&self.shortcut);
        let can = !self.shortcut.trim().is_empty() && ok.is_ok();
        if ui.add_enabled(can, egui::Button::new("Назначить")).clicked() {
            picked = ok.as_ref().ok().copied();
        }
        if let (false, Err(e)) = (self.shortcut.trim().is_empty(), &ok) {
            ui.label(egui::RichText::new(e.clone()).size(12.0).color(kit::BAD));
        }
        picked
    }

    // ── DPI ─────────────────────────────────────────────────────────────────

    fn ui_dpi(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let mut write = false;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Количество уровней").font(kit::semibold(13.0)).color(kit::TEXT));
            ui.add_space(8.0);
            for n in 1..=6usize {
                if segment(ui, self.dpi_count == n, &n.to_string(), true).clicked() && self.dpi_count != n {
                    // В неиспользуемых ячейках мыши может лежать 0: включённый уровень стал бы 50 DPI.
                    fill_new_levels(&mut self.dpi_x, &mut self.dpi_y, self.dpi_count, n, self.max_dpi);
                    self.dpi_count = n;
                    self.level = self.level.min(n - 1);
                    write = true;
                }
            }
            ui.add_space(18.0);
            if ui.checkbox(&mut self.dpi_separate, "Отдельные настройки X/Y").on_hover_text("Своя чувствительность по вертикали — например, для игр с прицелом").changed() {
                if !self.dpi_separate {
                    self.dpi_y = self.dpi_x;
                }
                write = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("↺ Сброс").on_hover_text("Уровни как с завода: 400, 800, 1600, 3200, 6400 и максимум").clicked() {
                    // Как с завода (OnMouseDpiReset в C#): шесть уровней, текущий — третий (1600), X = Y.
                    self.dpi_x = default_dpi(self.max_dpi);
                    self.dpi_y = self.dpi_x;
                    self.dpi_count = 6;
                    self.level = 2;
                    self.dpi_separate = false;
                    write = true;
                }
            });
        });
        ui.add_space(12.0);
        kit::cap(ui, "Нажмите на номер уровня, чтобы сделать его текущим. На мыши уровни переключает кнопка под колесом — индикатор горит цветом уровня.");
        ui.add_space(10.0);
        let col_w = ((ui.available_width() - 16.0) / 2.0).max(320.0);
        let max = self.max_dpi;
        for row in 0..3 {
            ui.horizontal_top(|ui| {
                for col in 0..2 {
                    let i = row * 2 + col;
                    if i >= self.dpi_count {
                        continue;
                    }
                    ui.vertical(|ui| {
                        ui.set_width(col_w);
                        let active = self.level == i;
                        let stroke = if active { Stroke::new(1.0_f32, kit::TEXT) } else { Stroke::new(1.0_f32, kit::LINE) };
                        kit::card().stroke(stroke).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                let (r, resp) = ui.allocate_exact_size(egui::vec2(40.0, 24.0), Sense::click());
                                let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Сделать текущим");
                                ui.painter().circle_filled(r.left_center() + egui::vec2(6.0, 0.0), 5.0, DPI_COLORS[i]);
                                ui.painter().text(r.left_center() + egui::vec2(18.0, 0.0), Align2::LEFT_CENTER, (i + 1).to_string(), kit::semibold(15.0), kit::TEXT);
                                if resp.clicked() && !active {
                                    self.level = i;
                                    write = true;
                                }
                                if active {
                                    ui.label(egui::RichText::new("сейчас").size(11.0).color(kit::MUTED));
                                }
                            });
                            ui.add_space(4.0);
                            let axes: &[(&str, bool)] = if self.dpi_separate { &[("X", false), ("Y", true)] } else { &[("", false)] };
                            for &(name, is_y) in axes {
                                ui.horizontal(|ui| {
                                    if !name.is_empty() {
                                        ui.label(egui::RichText::new(name).color(kit::MUTED));
                                    }
                                    let v = if is_y { &mut self.dpi_y[i] } else { &mut self.dpi_x[i] };
                                    let box_w = 78.0;
                                    let slider_w = ui.available_width() - box_w - 10.0;
                                    let r = dpi_slider(ui, v, max, DPI_COLORS[i], active, slider_w);
                                    if r.drag_stopped() || (r.clicked() && !r.dragged()) {
                                        write = true;
                                    }
                                    let dv = ui.add_sized([box_w, 30.0], egui::DragValue::new(v).range(50..=max).speed(50.0));
                                    if dv.lost_focus() || dv.drag_stopped() {
                                        write = true;
                                    }
                                });
                            }
                        });
                    });
                    ui.add_space(16.0);
                }
            });
            ui.add_space(12.0);
        }
        kit::cap(ui, format!("Предел этой мыши — {max} DPI."));
        if write {
            if !self.dpi_separate {
                self.dpi_y = self.dpi_x;
            }
            self.write_dpi(ctx);
        }
    }

    // ── Производительность ──────────────────────────────────────────────────

    fn ui_perf(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let mut sensor = false;
        let half = ((ui.available_width() - 16.0) / 2.0).max(340.0);
        ui.horizontal_top(|ui| {
            // Левая колонка: режим, частота, антидребезг; питание.
            ui.vertical(|ui| {
                ui.set_width(half);
                kit::card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    kit::h2(ui, "Производительность и отклик");
                    ui.add_space(10.0);
                    section(ui, "Выбор режима", None);
                    ui.horizontal(|ui| {
                        let w = (ui.available_width() - 16.0) / 3.0;
                        for (v, title, sub) in [
                            (0u8, "Режим мощности", "Экономит батарею, отклик — для работы и игр"),
                            (1, "Игровой режим", "Быстрее отклик, батарея садится быстрее"),
                            (2, "Суперрежим", "Максимум сенсора; нужна свежая прошивка"),
                        ] {
                            if mode_card(ui, w, self.mode == v, title, sub).clicked() && self.mode != v {
                                self.mode = v;
                                sensor = true;
                            }
                        }
                    });
                    ui.add_space(14.0);
                    section(ui, "Частота опроса, Гц", None);
                    let rates = self.rates.clone();
                    let mut rate = None;
                    ui.horizontal(|ui| {
                        for (i, r) in rates.iter().enumerate() {
                            if segment(ui, self.rate == i, &r.to_string(), true).clicked() && self.rate != i {
                                rate = Some(i);
                            }
                        }
                    });
                    if let Some(i) = rate {
                        self.rate = i;
                        let wl = self.wireless;
                        self.run(ctx, "", move |m| m.write_rate(i as u8, wl));
                    }
                    ui.add_space(14.0);
                    section(ui, "Время отключения кнопки (антидребезг), мс", Some("Меньше — быстрее клик; если появился двойной клик — прибавьте."));
                    let r = value_slider(ui, &mut self.debounce, 0..=20);
                    if r.drag_stopped() || (r.changed() && !r.dragged()) {
                        let ms = self.debounce;
                        self.run(ctx, "", move |m| m.write_debounce(ms));
                    }
                });
                ui.add_space(16.0);
                kit::card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    kit::h2(ui, "Питание и система");
                    ui.add_space(10.0);
                    section(ui, "Сон, минут", Some("Без движения мышь засыпает через это время в режимах 2,4 ГГц и Bluetooth. Будит клик."));
                    let mut never = self.sleep == 0;
                    let mut write = false;
                    ui.add_enabled_ui(!never, |ui| {
                        let mut v = if never { self.sleep_last } else { self.sleep };
                        let r = value_slider(ui, &mut v, 1..=120);
                        if !never {
                            self.sleep = v;
                            self.sleep_last = v;
                        }
                        if r.drag_stopped() || (r.changed() && !r.dragged()) {
                            write = true;
                        }
                    });
                    if ui.checkbox(&mut never, "Отключить спящий режим").changed() {
                        self.sleep = if never { 0 } else { self.sleep_last.max(1) };
                        write = true;
                    }
                    if write {
                        let min = self.sleep;
                        self.run(ctx, "", move |m| m.write_sleep(min));
                    }
                });
            });
            ui.add_space(16.0);
            // Правая колонка: поворот, высота отрыва; оптимизация движения.
            ui.vertical(|ui| {
                ui.set_width(half);
                kit::card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    kit::h2(ui, "Отслеживание и калибровка");
                    ui.add_space(10.0);
                    section(ui, "Вращение", Some("Угол сенсора под ваш хват, −30…30°."));
                    if rotate_dial(ui, &mut self.rotate) {
                        sensor = true;
                    }
                    ui.add_space(10.0);
                    section(ui, "Дистанция отрыва от поверхности", Some("Высота, при которой сенсор перестаёт отслеживать поверхность."));
                    ui.horizontal(|ui| {
                        for (v, t) in [(0u8, "0,7 мм"), (1, "1 мм"), (2, "2 мм")] {
                            if ui.radio(self.lod == v, t).clicked() && self.lod != v {
                                self.lod = v;
                                sensor = true;
                            }
                            ui.add_space(10.0);
                        }
                    });
                });
                ui.add_space(16.0);
                kit::card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    kit::h2(ui, "Оптимизация движения");
                    ui.add_space(10.0);
                    for (v, title, sub) in [
                        (&mut self.motion, "Motion Sync", "Синхронизирует отчёты сенсора с частотой опроса: ровнее движение, примерно +1 мс задержки."),
                        (&mut self.ripple, "Ripple Control", "Убирает дрожание курсора на высоком DPI."),
                        (&mut self.angle, "Angle Snapping", "Выпрямляет линии. В играх обычно выключают."),
                    ] {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.set_width(ui.available_width() - 56.0);
                                ui.label(egui::RichText::new(title).font(kit::semibold(13.5)).color(kit::TEXT));
                                kit::cap(ui, sub);
                            });
                            if toggle(ui, v).clicked() {
                                sensor = true;
                            }
                        });
                        ui.add_space(10.0);
                    }
                });
            });
        });
        if sensor {
            self.write_sensor(ctx);
        }
    }

    // ── Другие ──────────────────────────────────────────────────────────────

    fn ui_other(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) -> Option<String> {
        let mut msg = None;
        ui.horizontal_top(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(200.0, 400.0), Sense::hover());
            if let Some(tex) = &self.picture {
                let r = Rect::from_center_size(rect.center(), egui::vec2(196.0, 196.0 * 637.0 / 320.0));
                ui.painter().image(tex.id(), r, Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE);
            }
            ui.add_space(30.0);
            ui.vertical(|ui| {
                ui.set_width((ui.available_width()).min(600.0));
                ui.add_space(40.0);
                kit::card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(egui::RichText::new("Об устройстве").font(kit::semibold(15.0)).color(kit::TEXT));
                    ui.add_space(8.0);
                    egui::Grid::new("mouse-about").num_columns(2).spacing([24.0, 6.0]).show(ui, |ui| {
                        let rows = [
                            ("Модель", self.head.clone()),
                            ("Прошивка мыши", self.fw_mouse.clone().unwrap_or_else(|| "—".into())),
                            ("Прошивка приёмника", if self.wireless { self.fw_receiver.clone().unwrap_or_else(|| "—".into()) } else { "по кабелю".into() }),
                            ("Связь", if self.wireless { "донгл 2,4 ГГц".into() } else { "кабель USB".into() }),
                            ("Предел DPI", self.max_dpi.to_string()),
                        ];
                        for (k, v) in rows {
                            ui.label(egui::RichText::new(k).color(kit::MUTED));
                            ui.label(egui::RichText::new(v).color(kit::TEXT));
                            ui.end_row();
                        }
                    });
                    ui.add_space(6.0);
                    kit::cap(ui, "Обновлять прошивку — в M HUB (mchose.com.cn). Пока она открыта, RigControl мышью не управляет.");
                });
                ui.add_space(16.0);
                kit::card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.set_width(ui.available_width() - 150.0);
                            ui.label(egui::RichText::new("Заводские настройки").font(kit::semibold(15.0)).color(kit::TEXT));
                            kit::cap(ui, "Кнопки, DPI и производительность текущего профиля станут как с завода. Отменить нельзя.");
                        });
                        let armed = self.reset_armed.is_some();
                        let text = if armed { "Точно сбросить?" } else { "Сбросить профиль" };
                        let mut b = egui::Button::new(text).min_size(egui::vec2(140.0, 34.0));
                        if armed {
                            b = b.fill(Color32::from_rgb(0x6B, 0x1E, 0x24));
                        }
                        if ui.add(b).clicked() {
                            if armed {
                                self.reset_armed = None;
                                self.run(ctx, "профиль сброшен к заводским", |m| m.factory_reset());
                            } else {
                                self.reset_armed = Some(Instant::now());
                                msg = Some("Нажмите ещё раз, чтобы сбросить профиль мыши.".to_string());
                            }
                        }
                    });
                });
                ui.add_space(16.0);
                kit::card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(egui::RichText::new("Шпаргалка").font(kit::semibold(15.0)).color(kit::TEXT));
                    ui.add_space(6.0);
                    for (k, v) in [
                        ("Донгл 2.4G:", "переключатель — 2.4G. Привязать заново: выньте приёмник, зажмите ЛКМ + СКМ + ПКМ на 5 секунд (индикатор часто мигает зелёным), вставьте приёмник и держите мышь ближе 20 см."),
                        ("Bluetooth:", "переключатель — B, выберите мышь в списке устройств. Настройки из программы — через донгл или кабель."),
                        ("Кабель:", "мышь сама переходит на провод и заряжается. Лучше заряжать от USB компьютера."),
                        ("Индикатор:", "мигает красным — пора заряжать, горит красным — заряжается, горит зелёным — заряжена."),
                    ] {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(egui::RichText::new(k).font(kit::semibold(12.5)).color(kit::TEXT));
                            ui.label(egui::RichText::new(v).size(12.5).color(kit::MUTED));
                        });
                        ui.add_space(4.0);
                    }
                });
            });
        });
        msg
    }
}

/// Цвета индикатора DPI по инструкции: красный, синий, зелёный, жёлтый, голубой, фиолетовый.
const DPI_COLORS: [Color32; 6] = [
    Color32::from_rgb(0xE5, 0x48, 0x4D),
    Color32::from_rgb(0x3E, 0x7B, 0xFA),
    Color32::from_rgb(0x2F, 0xB3, 0x6B),
    Color32::from_rgb(0xF2, 0xC2, 0x30),
    Color32::from_rgb(0x22, 0xC3, 0xD6),
    Color32::from_rgb(0x9B, 0x5C, 0xF6),
];

/// Шкала DPI как в M HUB: метки 200, 1200, 2200, 3200, 4200 через равные промежутки, дальше — до предела.
fn dpi_ticks(max: u16) -> [f32; 6] {
    [200.0, 1200.0, 2200.0, 3200.0, 4200.0, f32::from(max.max(4300))]
}

fn dpi_to_frac(v: u16, max: u16) -> f32 {
    let t = dpi_ticks(max);
    let v = f32::from(v).clamp(t[0], t[5]);
    for i in 0..5 {
        if v <= t[i + 1] {
            return (i as f32 + (v - t[i]) / (t[i + 1] - t[i])) / 5.0;
        }
    }
    1.0
}

fn frac_to_dpi(f: f32, max: u16) -> u16 {
    let t = dpi_ticks(max);
    let x = (f.clamp(0.0, 1.0) * 5.0).min(4.9999);
    let i = x.floor() as usize;
    let v = t[i] + (t[i + 1] - t[i]) * (x - i as f32);
    let step = if v < 4200.0 { 50.0 } else { 100.0 };
    ((v / step).round() * step).clamp(50.0, f32::from(max)) as u16
}

/// Ползунок DPI с метками под ним.
fn dpi_slider(ui: &mut egui::Ui, v: &mut u16, max: u16, color: Color32, active: bool, width: f32) -> egui::Response {
    let (rect, mut resp) = ui.allocate_exact_size(egui::vec2(width.max(160.0), 40.0), Sense::click_and_drag());
    let track = Rect::from_min_max(egui::pos2(rect.left() + 8.0, rect.top() + 8.0), egui::pos2(rect.right() - 8.0, rect.top() + 16.0));
    if let Some(p) = resp.interact_pointer_pos() {
        if resp.dragged() || resp.clicked() || resp.drag_started() {
            let f = ((p.x - track.left()) / track.width()).clamp(0.0, 1.0);
            let nv = frac_to_dpi(f, max);
            if nv != *v {
                *v = nv;
                resp.mark_changed();
            }
        }
    }
    let painter = ui.painter();
    painter.rect_filled(track, 4.0, kit::SURFACE2);
    let f = dpi_to_frac(*v, max);
    let x = track.left() + track.width() * f;
    let fill = if active { color.gamma_multiply(0.75) } else { Color32::from_gray(90) };
    painter.rect_filled(Rect::from_min_max(track.min, egui::pos2(x, track.bottom())), 4.0, fill);
    let t = dpi_ticks(max);
    for (i, tick) in t.iter().enumerate() {
        let tx = track.left() + track.width() * i as f32 / 5.0;
        painter.circle_filled(egui::pos2(tx, track.center().y), 2.0, Color32::from_gray(150));
        let label = if i == 5 { max.to_string() } else { format!("{tick:.0}") };
        let align = if i == 5 { Align2::RIGHT_TOP } else if i == 0 { Align2::LEFT_TOP } else { Align2::CENTER_TOP };
        let lx = if i == 5 { tx + 8.0 } else if i == 0 { tx - 8.0 } else { tx };
        painter.text(egui::pos2(lx, track.bottom() + 6.0), align, label, FontId::proportional(10.5), kit::MUTED);
    }
    painter.circle_filled(egui::pos2(x, track.center().y), 8.0, kit::TEXT);
    painter.circle_stroke(egui::pos2(x, track.center().y), 8.0, Stroke::new(2.0_f32, Color32::from_gray(40)));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Ползунок с числом справа (антидребезг, сон).
fn value_slider(ui: &mut egui::Ui, v: &mut u8, range: std::ops::RangeInclusive<u8>) -> egui::Response {
    let mut r = None;
    ui.horizontal(|ui| {
        let w = ui.available_width() - 50.0;
        ui.spacing_mut().slider_width = w.max(120.0);
        r = Some(ui.add(egui::Slider::new(v, range).show_value(false)));
        ui.label(egui::RichText::new(v.to_string()).font(kit::semibold(14.0)).color(kit::TEXT));
    });
    r.unwrap_or_else(|| ui.label(""))
}

/// Заголовок пункта с подсказкой под ним.
fn section(ui: &mut egui::Ui, title: &str, hint: Option<&str>) {
    ui.label(egui::RichText::new(title).font(kit::semibold(13.5)).color(kit::TEXT));
    if let Some(h) = hint {
        kit::cap(ui, h);
    }
    ui.add_space(6.0);
}

/// Вкладка с подчёркиванием (как в M HUB).
fn tab_button(ui: &mut egui::Ui, selected: bool, text: &str) -> egui::Response {
    let font = kit::semibold(14.5);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font.clone(), kit::TEXT);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + egui::vec2(0.0, 10.0), Sense::click());
    let color = if selected || resp.hovered() { kit::TEXT } else { kit::MUTED };
    ui.painter().text(rect.left_top(), Align2::LEFT_TOP, text, font, color);
    if selected {
        ui.painter().line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(2.0_f32, kit::TEXT));
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Кнопка сегментного переключателя.
fn segment(ui: &mut egui::Ui, selected: bool, text: &str, enabled: bool) -> egui::Response {
    let fill = if selected { kit::TEXT } else { kit::SURFACE2 };
    let color = if selected { Color32::BLACK } else { kit::TEXT };
    let b = egui::Button::new(egui::RichText::new(text).font(kit::semibold(13.0)).color(color))
        .fill(fill)
        .min_size(egui::vec2(52.0, 32.0));
    ui.add_enabled(enabled, b)
}

/// Карточка режима сенсора: выбранная — светлая.
fn mode_card(ui: &mut egui::Ui, w: f32, selected: bool, title: &str, sub: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 92.0), Sense::click());
    let (fill, tc, sc) = if selected {
        (kit::TEXT, Color32::BLACK, Color32::from_gray(70))
    } else if resp.hovered() {
        (kit::HOVER, kit::TEXT, kit::MUTED)
    } else {
        (kit::SURFACE2, kit::TEXT, kit::MUTED)
    };
    let painter = ui.painter();
    painter.rect(rect, 10.0, fill, Stroke::new(1.0_f32, kit::LINE));
    painter.text(rect.left_top() + egui::vec2(12.0, 12.0), Align2::LEFT_TOP, title, kit::semibold(13.5), tc);
    let galley = painter.layout(sub.to_string(), FontId::proportional(11.5), sc, w - 24.0);
    painter.galley(rect.left_top() + egui::vec2(12.0, 34.0), galley, sc);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Переключатель (ToggleSwitch). Возвращает ответ с clicked при смене.
fn toggle(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let (rect, mut resp) = ui.allocate_exact_size(egui::vec2(42.0, 22.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool(resp.id, *on);
    let painter = ui.painter();
    let fill = if *on { kit::TEXT } else { kit::SURFACE2 };
    painter.rect(rect, 11.0, fill, Stroke::new(1.0_f32, kit::LINE_HOVER));
    let x = rect.left() + 11.0 + (rect.width() - 22.0) * t;
    painter.circle_filled(egui::pos2(x, rect.center().y), 8.0, if *on { Color32::BLACK } else { kit::MUTED });
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Шкала поворота сенсора −30…30° с кнопками − и +. true — значение пора записать.
fn rotate_dial(ui: &mut egui::Ui, value: &mut i8) -> bool {
    let mut write = false;
    ui.horizontal(|ui| {
        if ui.add(egui::Button::new("−").min_size(egui::vec2(36.0, 36.0))).clicked() && *value > -30 {
            *value -= 1;
            write = true;
        }
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(200.0, 170.0), Sense::click_and_drag());
        let c = egui::pos2(rect.center().x, rect.top() + 100.0);
        let radius = 82.0;
        // 30° на шкале — 135° от вертикали.
        let to_angle = |v: f32| (v * 4.5_f32).to_radians();
        let point = |a: f32, r: f32| egui::pos2(c.x + r * a.sin(), c.y - r * a.cos());
        if let Some(p) = resp.interact_pointer_pos() {
            if resp.dragged() || resp.clicked() {
                let a = (p.x - c.x).atan2(c.y - p.y).to_degrees();
                let nv = (a / 4.5).round().clamp(-30.0, 30.0) as i8;
                if nv != *value {
                    *value = nv;
                }
            }
        }
        if resp.drag_stopped() || resp.clicked() {
            write = true;
        }
        let painter = ui.painter();
        for step in -30..=30i32 {
            let a = to_angle(step as f32);
            let long = step % 15 == 0;
            let inner = if long { radius - 12.0 } else { radius - 6.0 };
            let col = if long { kit::TEXT } else { Color32::from_gray(90) };
            painter.line_segment([point(a, inner), point(a, radius)], Stroke::new(if long { 1.6_f32 } else { 1.0_f32 }, col));
            if long {
                painter.text(point(a, radius + 12.0), Align2::CENTER_CENTER, format!("{step}°"), FontId::proportional(10.5), kit::MUTED);
            }
        }
        let a = to_angle(f32::from(*value));
        painter.circle_filled(point(a, radius - 3.0), 6.0, kit::TEXT);
        // Мышь в центре, повёрнутая на угол.
        let body = Rect::from_center_size(c, egui::vec2(34.0, 58.0));
        painter.rect(body, 16.0, kit::SURFACE2, Stroke::new(1.5_f32, kit::LINE_HOVER));
        painter.line_segment([c + Vec2::angled(a - std::f32::consts::FRAC_PI_2) * 26.0, c], Stroke::new(1.5_f32, kit::MUTED));
        painter.text(c + egui::vec2(0.0, 6.0), Align2::CENTER_CENTER, format!("{}°", *value), kit::semibold(15.0), kit::TEXT);
        if ui.add(egui::Button::new("+").min_size(egui::vec2(36.0, 36.0))).clicked() && *value < 30 {
            *value += 1;
            write = true;
        }
        if *value != 0 && ui.small_button("Вернуть 0°").clicked() {
            *value = 0;
            write = true;
        }
    });
    write
}

/// Клавиша egui → имя для mousekeys::usage_from_name.
fn key_name(key: egui::Key) -> Option<String> {
    use egui::Key as K;
    let fixed = match key {
        K::ArrowLeft => "left",
        K::ArrowRight => "right",
        K::ArrowUp => "up",
        K::ArrowDown => "down",
        K::Escape => "esc",
        K::Tab => "tab",
        K::Backspace => "backspace",
        K::Enter => "enter",
        K::Space => "space",
        K::Insert => "insert",
        K::Delete => "delete",
        K::Home => "home",
        K::End => "end",
        K::PageUp => "pageup",
        K::PageDown => "pagedown",
        K::Minus => "-",
        _ => "",
    };
    if !fixed.is_empty() {
        return Some(fixed.to_string());
    }
    let name = key.name().to_lowercase();
    mousekeys::usage_from_name(&name).map(|_| name)
}

fn battery_text(st: Option<&MouseStatus>) -> String {
    st.filter(|s| s.has_mouse())
        .map(|s| format!(" · заряд {}%{}", s.battery, if s.charging() { " (заряжается)" } else { "" }))
        .unwrap_or_default()
}

/// Заводские уровни DPI (DefaultDpi в C#): последний — предел сенсора.
fn default_dpi(max: u16) -> [u16; 6] {
    [400, 800, 1600, 3200, 6400, max]
}

/// Увеличили число уровней с old до new: негодные значения в новых ячейках (вне 200…50000) заменить
/// заводскими (OnMouseDpiCount в C#).
fn fill_new_levels(x: &mut [u16; 6], y: &mut [u16; 6], old: usize, new: usize, max: u16) {
    let defaults = default_dpi(max);
    for i in old.min(6)..new.min(6) {
        if !(200..=50000).contains(&x[i]) {
            x[i] = defaults[i];
        }
        if !(200..=50000).contains(&y[i]) {
            y[i] = x[i];
        }
    }
}

/// Останется ли левый клик хоть на одной кнопке, если кнопке b назначить key.
fn keeps_left_click(keys: &[MouseKey], b: usize, key: MouseKey) -> bool {
    (0..mousekeys::COUNT).any(|i| mousekeys::is_left_click(i, if i == b { key } else { keys[i] }))
}

/// Настройки, прочитанные прямо перед записью. Ошибка — если не прочитались или мышь уже на другом
/// профиле, чем показан на странице: иначе уровни DPI и кнопки одного профиля записались бы в другой.
fn fresh_config(m: &McMouse, shown_profile: u8) -> Result<MouseConfig, String> {
    let fresh = m
        .read_config()
        .filter(|c| c.valid)
        .ok_or("не удалось прочитать текущий профиль мыши — ничего не записано")?;
    if fresh.profile != shown_profile {
        return Err(format!(
            "на мыши включён профиль {}, а страница показывала профиль {} — настройки перечитаны, повторите",
            fresh.profile + 1,
            shown_profile + 1
        ));
    }
    Ok(fresh)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_dpi_levels_get_factory_values() {
        // Один рабочий уровень, в остальных ячейках мыши — нули.
        let (mut x, mut y) = ([1600, 0, 0, 0, 0, 0], [1600, 0, 0, 0, 0, 0]);
        fill_new_levels(&mut x, &mut y, 1, 3, 26000);
        assert_eq!(x, [1600, 800, 1600, 0, 0, 0]);
        assert_eq!(y, [1600, 800, 1600, 0, 0, 0]);
        // Годные значения не трогаем; предел сенсора — у шестого уровня.
        let (mut x, mut y) = ([400, 1000, 0, 0, 0, 0], [400, 900, 0, 0, 0, 0]);
        fill_new_levels(&mut x, &mut y, 1, 6, 26000);
        assert_eq!(x, [400, 1000, 1600, 3200, 6400, 26000]);
        assert_eq!(y, [400, 900, 1600, 3200, 6400, 26000]);
    }

    #[test]
    fn left_click_guard_uses_given_keys() {
        // Родные функции (тип 0) на всех кнопках: левый клик — только на левой.
        let keys = [MouseKey::default(); mousekeys::COUNT];
        let other = mousekeys::keyboard_key(0, 4);
        assert!(keeps_left_click(&keys, mousekeys::LEFT + 1, other));
        assert!(!keeps_left_click(&keys, mousekeys::LEFT, other));
        // В другом профиле левая кнопка уже занята клавишей, а левый клик висит на последней кнопке:
        // её переназначать нельзя — проверять надо по настройкам этого профиля, а не показанного.
        let mut second = keys;
        second[mousekeys::LEFT] = other;
        second[mousekeys::COUNT - 1] = MouseKey { kind: 1, value: 0x01_0000 }; // «левый клик»
        assert!(keeps_left_click(&keys, mousekeys::COUNT - 1, other));
        assert!(!keeps_left_click(&second, mousekeys::COUNT - 1, other));
    }

    #[test]
    fn dpi_scale_round_trip() {
        let max = 42000;
        assert_eq!(dpi_to_frac(200, max), 0.0);
        assert!((dpi_to_frac(1200, max) - 0.2).abs() < 1e-6);
        assert!((dpi_to_frac(42000, max) - 1.0).abs() < 1e-6);
        assert_eq!(frac_to_dpi(0.2, max), 1200);
        assert_eq!(frac_to_dpi(1.0, max), 42000);
        for v in [400u16, 800, 1600, 3200] {
            assert_eq!(frac_to_dpi(dpi_to_frac(v, max), max), v);
        }
    }
}
