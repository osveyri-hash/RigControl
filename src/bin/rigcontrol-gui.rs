//! RigControl — окно с вкладками (замена WPF MainWindow).
//! Сборка: cargo build --release --features gui  →  target\\release\\rigcontrol-gui.exe
//!
//! Окно живёт, только пока открыто: при открытии оно ставит фоновую службу (rigcontrol-service.exe,
//! значок в трее) на паузу и само ведёт устройства, при закрытии отдаёт их службе и выходит целиком,
//! не оставляя в памяти ни окна, ни его текстур. `--tray` (старый автозапуск) — просто запустить службу.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::time::Duration;

use eframe::egui;
use rigcontrol_core::device::describe_vendor_interfaces;
use rigcontrol_core::kb_light::KbLight;
use rigcontrol_core::device_art::{self, DeviceArt};
use rigcontrol_core::mouse_panel::{MousePanel, MouseTile};
use rigcontrol_core::ui_kit::{self as kit, glyph};
use rigcontrol_core::service::Service;
use rigcontrol_core::service_link;
use rigcontrol_core::settings::{self, Settings};
use rigcontrol_core::{autostart, lcdscene, logfile, protocol};
use rigcontrol_core::{fan_curve, profiles};
use rigcontrol_core::fans::{FanControl, FanState};
use rigcontrol_core::lhm::Fan;
use rigcontrol_core::cooler;
use rigcontrol_core::battery::BatteryWatch;
use rigcontrol_core::memory;
use rigcontrol_core::pump_editor::PumpEditor;
use rigcontrol_core::system_panel::SystemPanel;
use rigcontrol_core::key_painter::KeyPainter;
use rigcontrol_core::keypaint;
use rigcontrol_core::protocol::ScreenFrame;
use rigcontrol_core::screen;
use rigcontrol_core::screen_upload::{self, ScreenUpload, UploadState};

const ARGB_MODES: &[(i32, &str)] = &[
    (0, "Не управлять"),
    (1, "Выключить"),
    (2, "Постоянный цвет"),
    (3, "Дыхание"),
    (4, "Спектр"),
    (5, "Бегущая радуга"),
    (6, "Комета"),
    (7, "Цвет по температуре CPU"),
    (8, "Как клавиатура"),
];

#[derive(PartialEq, Clone, Copy)]
// App = «Настройки» (PageSettings), System = «О системе» (PageSystem), Fans = «Вентиляторы» (PageFans).
enum Tab { Devices, Keyboard, Mouse, Pump, Argb, Fans, System, App }

impl Tab {
    /// Страница рисует свои карточки сама (без общей карточки вокруг).
    fn own_cards(self) -> bool {
        matches!(self, Tab::Devices | Tab::Keyboard | Tab::Mouse | Tab::Pump | Tab::Fans | Tab::Argb | Tab::System)
    }

    /// Заголовок страницы (H1 в MainWindow.xaml).
    fn title(self) -> &'static str {
        match self {
            Tab::Devices => "Устройства",
            Tab::Keyboard => "Клавиатура AJAZZ AK820 Pro",
            Tab::Mouse => "Мышь MCHOSE",
            Tab::Pump => "Экран помпы",
            Tab::Argb => "Подсветка СЖО",
            Tab::Fans => "Вентиляторы",
            Tab::System => "О системе",
            Tab::App => "Настройки",
        }
    }
}

/// Пункты бокового меню: (страница, значок MDL2, подпись), с подписями разделов как в C#.
const NAV_TOP: &[(Tab, char, &str)] = &[(Tab::Devices, glyph::DEVICES, "Устройства")];
const NAV_DEVICES: &[(Tab, char, &str)] = &[
    (Tab::Keyboard, glyph::KEYBOARD, "Клавиатура"),
    (Tab::Mouse, glyph::MOUSE, "Мышь"),
    (Tab::Pump, glyph::PUMP, "Помпа"),
    (Tab::Argb, glyph::LIGHT, "Подсветка"),
    (Tab::Fans, glyph::FANS, "Вентиляторы"),
];
const NAV_SYSTEM: &[(Tab, char, &str)] = &[
    (Tab::System, glyph::SYSTEM, "О системе"),
    (Tab::App, glyph::SETTINGS, "Настройки"),
];

/// Состояние на плитке: точка и текст, или заряд и тип связи («80% · 2.4G», как в M HUB).
enum TileStatus {
    Text(String, bool),
    Link { battery: Option<u8>, charging: bool, link: Option<&'static str> },
}

struct Tile {
    key: &'static str,
    glyph: char,
    title: &'static str,
    tab: Tab,
    name: String,
    detail: String,
    status: TileStatus,
    online: bool,
}

fn hex(c: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

fn rgb(s: &str) -> [u8; 3] {
    let (r, g, b) = lcdscene::parse_color(s).unwrap_or((255, 255, 255));
    [r, g, b]
}

/// Заголовок окна.
const WINDOW_TITLE: &str = "RigControl";

/// Показать окно (вторую копию запустили — она просит первую показаться).
fn restore(ctx: &egui::Context) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    ctx.request_repaint();
}

/// Логотип RigControl (пиксель-арт, как app.ico в C#-версии) размером size×size, RGBA.
fn logo_rgba(size: u32) -> Option<Vec<u8>> {
    let png = include_bytes!("../../assets/logo.png");
    let img = image::load_from_memory_with_format(png, image::ImageFormat::Png).ok()?;
    let small = image::imageops::resize(&img.to_rgba8(), size, size, image::imageops::FilterType::Triangle);
    Some(small.into_raw())
}

struct App {
    s: Settings,
    tab: Tab,
    svc: Option<Service>,
    msg: String,
    devices: Vec<String>,
    mouse: MousePanel,
    autostart_on: bool,
    /// Задача автозапуска есть, но запускает другую программу (например, C#-версию): её команда.
    autostart_other: Option<String>,
    kb_color: [u8; 3],
    /// Клавиатура: поток подсветки (свои эффекты кадрами, «Выключить», слежение за кабелем).
    kb: KbLight,
    /// Вкладка страницы клавиатуры: 0 — «Экран и подсветка», 1 — «Раскраска».
    kb_tab: u8,
    /// «Радуга» включена сама (у «Разноцветного» и «Спектра» она всегда), как _rainbowAuto в C#.
    rainbow_auto: bool,
    /// «Подобрать»: какой эффект из списка показываем и с какого момента.
    demo: Option<(usize, std::time::Instant)>,
    /// Последнее событие клавиатуры, уже показанное всплывающим сообщением.
    kb_event_at: Option<std::time::Instant>,
    /// Поля #RRGGBB у цвета подсветки и у кисти.
    color_text: String,
    paint_text: String,
    argb_colors: [[u8; 3]; 3],
    editor: PumpEditor,
    /// Страница «О системе»: датчики в своём потоке, пики, средние под нагрузкой.
    sys: SystemPanel,
    scr_path: String,
    scr_frames: Vec<ScreenFrame>,
    scr_tex: Option<egui::TextureHandle>,
    scr_idx: usize,
    scr_t0: std::time::Instant,
    scr_upload: Option<ScreenUpload>,
    /// Итог заливки уже показан.
    scr_reported: bool,
    painter: KeyPainter,
    /// Замер скорости эффекта клавиатуры: когда начат и сколько кругов отмечено.
    cal_start: Option<std::time::Instant>,
    cal_circles: u32,
    applied: Option<Settings>,
    changed_at: Option<std::time::Instant>,
    seen: Option<Settings>,
    /// «Вентиляторы»: поток управления скоростью (MainWindow.Fans.cs).
    fans: FanControl,
    /// Заряд клавиатуры по радио (MainWindow.Battery.cs).
    battery: BatteryWatch,
    /// «ОЗУ: N МБ» (MainWindow.Memory.cs).
    ram_label: String,
    ram_at: std::time::Instant,
    trim_at: Option<std::time::Instant>,
    was_minimized: bool,
    /// Картинки устройств для плиток и логотип в шапке меню.
    art: DeviceArt,
    logo: Option<egui::TextureHandle>,
    /// Всплывающее сообщение внизу справа (MouseToast/ProfileNote в C#).
    toast: String,
    toast_at: Option<std::time::Instant>,
    /// Картинка экрана клавиатуры загружается при первом входе на страницу клавиатуры.
    scr_loaded: bool,
    /// Вентиляторы: анимация плиток, выбранный канал, раскрыт ли список неподключённых.
    fan_anim: std::collections::HashMap<String, FanAnim>,
    fan_sel: Option<String>,
    /// Где плитка выбранного вентилятора (рядом с ней — окошко настроек) и когда окошко открылось.
    fan_sel_rect: Option<egui::Rect>,
    fan_pop_for: Option<String>,
    fan_pop_at: std::time::Instant,
    /// Подсветка: движки эффектов для большого предпросмотра и для карточек режимов.
    argb_hero: rigcontrol_core::argb::Renderer,
    argb_minis: Vec<rigcontrol_core::argb::Renderer>,
    /// Картинки СЖО и вентилятора с живыми светодиодами и крыльчаткой.
    led_art: rigcontrol_core::led_art::LedArt,
    /// Armoury Crate: работает ли служба подсветки, когда проверяли, ответ проверки, идёт ли остановка/запуск.
    armoury_running: Option<bool>,
    armoury_at: Option<std::time::Instant>,
    armoury_rx: Option<std::sync::mpsc::Receiver<bool>>,
    armoury_op: Option<std::sync::mpsc::Receiver<(bool, Result<(), String>)>>,
    armoury_confirm: bool,
    /// Идёт перезапуск службы (остановка старой и запуск новой) в фоновом потоке.
    svc_job: Option<std::thread::JoinHandle<Service>>,
    /// Пока шёл перезапуск, настройки поменялись ещё раз (значение — выставить ли клавиатуру).
    svc_again: Option<bool>,
    /// Состояние службы на момент начала перезапуска.
    svc_last: Vec<(String, String)>,
    /// Помпа: вывод остановлен кнопкой (или автовывод при запуске выключен).
    pump_off: bool,
    pump_diag: bool,
    /// Идёт «Сбросить USB экрана»: ответ потока.
    pump_reset: Option<std::sync::mpsc::Receiver<Result<(), String>>>,
    /// Миниатюры тем экрана помпы.
    theme_thumbs: std::collections::HashMap<&'static str, Option<egui::TextureHandle>>,
    /// Предпросмотр режима «Картинка или гифка»: ключ (путь|обрезка), кадры, всего кадров, начало.
    pump_preview: Option<(String, Vec<(egui::TextureHandle, u64)>, usize, std::time::Instant)>,
    /// Фоновая служба на паузе, пока открыто окно (None — службы не было).
    /// Последнее поле: при закрытии окна оно освобождается последним, когда устройства уже отданы.
    service: Option<service_link::Pause>,
}

impl App {
    fn new(service: Option<service_link::Pause>) -> Self {
        logfile::cleanup_on_startup();
        let s = Settings::load();
        rigcontrol_core::light_mirror::set_rates(s.effect_rates.as_ref());
        // Клавиатуру ведёт свой поток (KbLight), служба — ARGB и помпу.
        let svc = Some(Service::start_with(&s, false, s.auto_output));
        let kb = KbLight::start(&s);
        let rainbow_auto = matches!(protocol::effect_mode(s.effect), 6 | 8) && s.rainbow;
        let editor = PumpEditor::new(&s);
        let painter = KeyPainter::new(&s);
        let pump_off = !s.auto_output;
        let mut app = Self {
            editor,
            painter,
            cal_start: None,
            cal_circles: 0,
            applied: None,
            changed_at: None,
            seen: None,
            fans: FanControl::start(&s),
            battery: BatteryWatch::start(None),
            ram_label: memory::label(),
            ram_at: std::time::Instant::now(),
            trim_at: None,
            was_minimized: false,
            scr_path: s.last_image.clone().unwrap_or_default(),
            scr_frames: Vec::new(),
            scr_tex: None,
            scr_idx: 0,
            scr_t0: std::time::Instant::now(),
            scr_upload: None,
            scr_reported: true,
            kb_color: rgb(&s.color),
            kb,
            kb_tab: 0,
            rainbow_auto,
            demo: None,
            kb_event_at: None,
            color_text: String::new(),
            paint_text: String::new(),
            argb_colors: [rgb(&s.argb_color), rgb(&s.argb_color2), rgb(&s.argb_color3)],
            s,
            tab: Tab::Devices,
            sys: SystemPanel::new(),
            svc,
            msg: String::new(),
            devices: Vec::new(),
            mouse: MousePanel::new(),
            autostart_on: false,
            autostart_other: None,
            art: DeviceArt::default(),
            logo: None,
            toast: String::new(),
            toast_at: None,
            scr_loaded: false,
            fan_anim: Default::default(),
            fan_sel: None,
            fan_sel_rect: None,
            fan_pop_for: None,
            fan_pop_at: std::time::Instant::now(),
            argb_hero: rigcontrol_core::argb::Renderer::new(),
            argb_minis: (0..ARGB_MODES.len()).map(|_| rigcontrol_core::argb::Renderer::new()).collect(),
            led_art: Default::default(),
            armoury_running: None,
            armoury_at: None,
            armoury_rx: None,
            armoury_op: None,
            armoury_confirm: false,
            svc_job: None,
            svc_again: None,
            svc_last: Vec::new(),
            pump_off,
            pump_diag: false,
            pump_reset: None,
            theme_thumbs: Default::default(),
            pump_preview: None,
            service,
        };
        app.refresh_devices();
        app.refresh_autostart();
        app
    }

    fn refresh_autostart(&mut self) {
        // Старая задача запускала окно с --tray: теперь при входе в Windows запускается фоновая служба.
        let mut status = autostart::status();
        if status == autostart::Status::Legacy {
            let _ = autostart::set(true, "");
            status = autostart::status();
        }
        match status {
            autostart::Status::Off => (self.autostart_on, self.autostart_other) = (false, None),
            autostart::Status::This | autostart::Status::Legacy => (self.autostart_on, self.autostart_other) = (true, None),
            autostart::Status::Other(cmd) => (self.autostart_on, self.autostart_other) = (false, Some(cmd)),
        }
    }

    /// Страницы загружают картинки сами, когда их открывают; уходя со страницы, отдаём её тяжёлые части
    /// (текстуры, кадры гифок, шрифты сцены) — при возврате они загрузятся снова.
    fn leave(&mut self, from: Tab, to: Tab) {
        if from == to {
            return;
        }
        let heavy = match from {
            Tab::Pump => {
                self.pump_preview = None;
                self.theme_thumbs.clear();
                self.editor.release();
                true
            }
            Tab::Keyboard => {
                self.painter.release();
                true
            }
            Tab::Devices => {
                self.art = DeviceArt::default();
                false
            }
            _ => false,
        };
        // Картинки СЖО и вентилятора нужны только «Подсветке» и «Вентиляторам».
        let led = |t: Tab| matches!(t, Tab::Argb | Tab::Fans);
        if led(from) && !led(to) {
            self.led_art = Default::default();
        }
        if heavy || (led(from) && !led(to)) {
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_millis(800));
                memory::trim();
            });
        }
    }

    fn refresh_devices(&mut self) {
        self.devices = hidapi::HidApi::new()
            .map(|api| describe_vendor_interfaces(&api))
            .unwrap_or_else(|e| vec![format!("hidapi: {e}")]);
    }

    fn save(&mut self) {
        self.fans.configure(&self.s);
        if let Err(e) = self.s.save() {
            self.msg = format!("Не удалось сохранить: {e}");
        }
    }

    /// Цвета из пикеров → настройки (раньше это делали кнопки «Применить»).
    fn sync_colors(&mut self) {
        self.s.color = hex(self.kb_color);
        // Три цвета — палитра волны без радуги (ArgbColor2/3 в C#), не цвета разных разъёмов.
        self.s.argb_color = hex(self.argb_colors[0]);
        self.s.argb_color2 = hex(self.argb_colors[1]);
        self.s.argb_color3 = hex(self.argb_colors[2]);
    }

    /// Автоприменение: ждём, пока пользователь перестанет менять настройки (AUTO_DELAY),
    /// потом сохраняем. Клавиатуру выставляем напрямую (быстро), всё остальное —
    /// перезапуском фоновой службы (ARGB, помпа).
    fn auto_apply(&mut self, ctx: &egui::Context) {
        const AUTO_DELAY: Duration = Duration::from_millis(500);
        self.sync_colors();
        let Some(applied) = self.applied.clone() else {
            self.applied = Some(self.s.clone());
            self.seen = Some(self.s.clone());
            return;
        };
        if self.seen.as_ref() != Some(&self.s) {
            self.seen = Some(self.s.clone());
            self.changed_at = Some(std::time::Instant::now());
        }
        let Some(at) = self.changed_at else { return };
        if self.s == applied {
            self.changed_at = None;
            return;
        }
        if at.elapsed() < AUTO_DELAY {
            ctx.request_repaint_after(AUTO_DELAY - at.elapsed() + Duration::from_millis(10));
            return;
        }
        self.changed_at = None;

        let paint_mode = protocol::effect_mode(self.s.effect) == rigcontrol_core::effects::PAINT;
        let kb_changed = applied.effect != self.s.effect
            || applied.color != self.s.color
            || applied.rainbow != self.s.rainbow
            || applied.brightness != self.s.brightness
            || applied.speed != self.s.speed
            || applied.direction != self.s.direction
            || applied.sync_light != self.s.sync_light
            || applied.sync_style != self.s.sync_style
            || ((self.s.sync_light || protocol::effect_mode(self.s.effect) == rigcontrol_core::effects::GIF_FLOW)
                && (applied.last_image != self.s.last_image
                    || applied.crop != self.s.crop
                    || applied.max_frames != self.s.max_frames))
            || (paint_mode
                && (applied.key_paint != self.s.key_paint
                    || applied.paint_anim != self.s.paint_anim
                    || applied.paint_speed != self.s.paint_speed));
        // Поля клавиатуры и её экрана службу (ARGB, помпа) не касаются.
        let mut rest = applied.clone();
        rest.effect = self.s.effect;
        rest.color = self.s.color.clone();
        rest.rainbow = self.s.rainbow;
        rest.brightness = self.s.brightness;
        rest.speed = self.s.speed;
        rest.direction = self.s.direction;
        rest.key_paint = self.s.key_paint.clone();
        rest.paint_color = self.s.paint_color.clone();
        rest.paint_anim = self.s.paint_anim;
        rest.paint_speed = self.s.paint_speed;
        rest.paint_mirror = self.s.paint_mirror;
        rest.sync_light = self.s.sync_light;
        rest.sync_style = self.s.sync_style;
        rest.last_image = self.s.last_image.clone();
        rest.crop = self.s.crop;
        rest.max_frames = self.s.max_frames;
        // Вентиляторы ведёт свой поток (FanControl::configure) — помпу и ARGB из-за них не перезапускаем.
        rest.fan_mode_all = self.s.fan_mode_all;
        rest.fan_modes = self.s.fan_modes.clone();
        rest.fan_percent = self.s.fan_percent.clone();
        rest.fan_curves = self.s.fan_curves.clone();
        // «Обрезать под экран» — общий флажок с экранчиком клавиатуры, но в режиме «Картинка или гифка»
        // он меняет и кадры помпы: служба читает настройки только при запуске.
        let svc_changed = rest != self.s || (self.s.cooler_mode == 1 && applied.crop != self.s.crop);

        if let Err(e) = self.s.save() {
            self.msg = format!("Не удалось сохранить: {e}");
        }
        // Как в C#: применяется молча, сообщение — только если что-то не получилось.
        if kb_changed {
            self.kb.apply(&self.s);
        }
        if svc_changed {
            self.restart(false);
        }
        self.applied = Some(self.s.clone());
    }

    /// Перезапустить потоки ARGB и помпы с текущими настройками. Остановка ждёт потоки (обмен по USB,
    /// рукопожатие помпы до 1,5 с, чтение гифки), поэтому идёт в фоне: окно не замирает на каждой правке
    /// (в C# остановка тоже ожидается асинхронно). Новая служба стартует только после остановки старой.
    fn restart(&mut self, keyboard: bool) {
        if self.svc_job.is_some() {
            // Прошлый перезапуск ещё идёт: когда закончится — ещё один, уже с самыми свежими настройками.
            self.svc_again = Some(self.svc_again.unwrap_or(false) || keyboard);
            return;
        }
        if let Some(o) = &self.svc {
            self.svc_last = o.status();
        }
        // Старая служба — в общей ячейке: если поток не создастся, её можно забрать обратно и остановить здесь.
        let old = std::sync::Arc::new(std::sync::Mutex::new(self.svc.take()));
        let cell = old.clone();
        // Кнопка «Остановить» на странице помпы: служба без вывода на экран.
        let (s, pump) = (self.s.clone(), !self.pump_off);
        let job = std::thread::Builder::new().name("svc-restart".into()).spawn(move || {
            if let Some(o) = cell.lock().unwrap_or_else(|e| e.into_inner()).take() {
                o.stop();
            }
            Service::start_with(&s, keyboard, pump)
        });
        match job {
            Ok(j) => self.svc_job = Some(j),
            // Поток не создался: останавливаем старую службу здесь и дожидаемся её потоков (stop, а не drop:
            // drop только ставит флаг, и новая служба стартовала бы рядом с ещё работающей старой).
            Err(_) => {
                if let Some(o) = old.lock().unwrap_or_else(|e| e.into_inner()).take() {
                    o.stop();
                }
                self.svc = Some(Service::start_with(&self.s, keyboard, !self.pump_off));
            }
        }
    }

    /// Раз в кадр: перезапуск закончился — берём новую службу; за это время настройки меняли ещё — повторяем.
    fn poll_restart(&mut self, ctx: &egui::Context) {
        if self.svc_job.as_ref().is_some_and(|j| j.is_finished()) {
            if let Some(j) = self.svc_job.take() {
                self.svc = j.join().ok();
            }
            if let Some(keyboard) = self.svc_again.take() {
                self.restart(keyboard);
            }
        }
        if self.svc_job.is_some() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }

    /// Остановить службу и дождаться этого (закрытие окна, «Сбросить USB экрана»).
    fn stop_service(&mut self) {
        self.svc_again = None;
        if let Some(j) = self.svc_job.take() {
            if let Ok(s) = j.join() {
                self.svc = Some(s);
            }
        }
        if let Some(s) = self.svc.take() {
            self.svc_last = s.status();
            s.stop();
        }
    }

    /// Размер экрана, с которым служба открыла помпу (None — помпа не открыта).
    fn pump_detected_size(&self) -> Option<(u32, u32)> {
        self.svc_status("Помпа·экран").as_deref().and_then(rigcontrol_core::service::parse_panel_size)
    }

    fn svc_status(&self, part: &str) -> Option<String> {
        // Пока идёт перезапуск — последнее известное состояние, чтобы строки не мигали.
        let status = match &self.svc {
            Some(s) => s.status(),
            None if self.svc_job.is_some() => self.svc_last.clone(),
            None => return None,
        };
        status.into_iter().find(|x| x.0 == part).map(|x| x.1)
    }

    /// Плитки главной (UpdateDeviceTiles в MainWindow.Devices.cs).
    fn tiles(&self) -> Vec<Tile> {
        let mut tiles = Vec::new();

        // Мышь
        let m: MouseTile = self.mouse.tile();
        let status = if m.online {
            TileStatus::Link { battery: m.battery, charging: m.charging, link: Some(if m.wireless { "2.4G" } else { "USB" }) }
        } else if m.reading {
            TileStatus::Text("Поиск…".into(), false)
        } else {
            TileStatus::Text(if m.found { "Выключена" } else { "Не найдена" }.into(), false)
        };
        let mut detail = String::new();
        if m.online {
            if let Some(p) = m.profile {
                detail = format!("Профиль {}", p + 1);
            }
            if let Some(d) = m.dpi {
                detail += &format!(" · {d} DPI");
            }
            if let Some(hz) = m.rate_hz {
                detail += &format!(" · {hz} Гц");
            }
        }
        tiles.push(Tile {
            key: device_art::MOUSE,
            glyph: glyph::MOUSE,
            title: "Мышь",
            tab: Tab::Mouse,
            name: if m.found { m.model.clone() } else { "Мышь".into() },
            detail,
            status,
            online: m.online,
        });

        // Клавиатура
        let ks = self.kb.state();
        let (status, online) = match &ks.link {
            Some(Ok(how)) => {
                let radio = how.starts_with("по радио");
                let battery = if radio { self.battery.state().level } else { None };
                (TileStatus::Link { battery, charging: false, link: Some(if radio { "2.4G" } else { "USB" }) }, true)
            }
            Some(Err(_)) => (TileStatus::Text("Не найдена".into(), false), false),
            None => (TileStatus::Text("Поиск…".into(), false), false),
        };
        let detail = if ks.off || self.s.effect == 0 {
            "Подсветка выключена".to_string()
        } else {
            format!("Эффект: {}", protocol::effect_name(self.s.effect).to_lowercase())
        };
        tiles.push(Tile {
            key: device_art::KEYBOARD,
            glyph: glyph::KEYBOARD,
            title: "Клавиатура",
            tab: Tab::Keyboard,
            name: "AK820 Pro".into(),
            detail,
            status,
            online,
        });

        // Экран помпы
        let pump = self.svc_status("Помпа").unwrap_or_default();
        let pm = pump.split(" · ").find_map(|x| x.strip_prefix("PM ")).map(str::to_string);
        let (text, ok) = match &pm {
            Some(pm) => (format!("На связи · PM {pm}"), true),
            None if pump.starts_with("автовывод") => ("Автовывод выключен".to_string(), false),
            None => ("Не найдена".to_string(), false),
        };
        let detail = if !ok {
            "Экран остановлен".to_string()
        } else if self.s.cooler_mode == 1 {
            "Экран работает · картинка".to_string()
        } else {
            format!("Экран работает · «{}»", self.s.cooler_theme.clone().unwrap_or_else(|| "Классика".into()))
        };
        tiles.push(Tile {
            key: device_art::PUMP,
            glyph: glyph::PUMP,
            title: "Экран помпы",
            tab: Tab::Pump,
            name: "Thermalright".into(),
            detail,
            status: TileStatus::Text(text, ok),
            online: ok,
        });

        // Подсветка СЖО
        let argb = self.svc_status("ARGB").unwrap_or_default();
        let ok = argb.starts_with("ASUS AURA");
        let text = if ok {
            "На связи".to_string()
        } else if argb.starts_with("не управляется") {
            "Не управляется".to_string()
        } else {
            "Не найдена".to_string()
        };
        let mode = ARGB_MODES.iter().find(|m| m.0 == self.s.argb_mode).map(|m| m.1).unwrap_or("?");
        tiles.push(Tile {
            key: device_art::ARGB,
            glyph: glyph::LIGHT,
            title: "Подсветка СЖО",
            tab: Tab::Argb,
            name: "ASUS AURA".into(),
            detail: format!("Режим: {}", mode.to_lowercase()),
            status: TileStatus::Text(text, ok),
            online: ok,
        });

        // Вентиляторы
        let st = self.fans.state();
        let n = st.fans.len();
        let (text, ok) = if n > 0 {
            let temp = st.cpu_temp.map(|t| format!(" · CPU {t:.0} °C")).unwrap_or_default();
            (format!("{n} {}{temp}", kit::plural(n, "канал", "канала", "каналов")), true)
        } else if st.active && !st.read && st.error.is_none() {
            ("Поиск…".to_string(), false)
        } else {
            ("Нет датчиков".to_string(), false)
        };
        tiles.push(Tile {
            key: device_art::FANS,
            glyph: glyph::FANS,
            title: "Вентиляторы",
            tab: Tab::Fans,
            name: st.board.clone().map(|b| b.split(" · ").next().unwrap_or("").to_string()).filter(|b| !b.is_empty()).unwrap_or_else(|| "Плата".into()),
            detail: format!("Режим: {}", fan_mode_summary(&self.s).to_lowercase()),
            status: TileStatus::Text(text, ok),
            online: ok,
        });
        tiles
    }

    /// Главная: плитки устройств (PageDevices).
    fn ui_devices(&mut self, ui: &mut egui::Ui) -> Option<Tab> {
        let tiles = self.tiles();
        let online = tiles.iter().filter(|t| t.online).count();
        kit::cap(ui, format!("На связи {online} из {}. Нажмите на плитку, чтобы открыть настройки.", tiles.len()));
        ui.add_space(20.0);
        let mut open = None;
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(16.0, 16.0);
            ui.horizontal_wrapped(|ui| {
                for t in &tiles {
                    let tex = self.art.texture(ui.ctx(), t.key);
                    if device_tile(ui, tex.as_ref(), t).clicked() {
                        open = Some(t.tab);
                    }
                }
            });
        });
        open
    }

    /// Страница «Клавиатура» (PageKeyboard): состояние, вкладки «Экран и подсветка» и «Раскраска».
    fn ui_keyboard(&mut self, ui: &mut egui::Ui) {
        let ks = self.kb.state();
        // Строка состояния и «Найти клавиатуру» (StatusHint, ReconnectButton).
        ui.horizontal(|ui| {
            let (text, ok) = match &ks.link {
                Some(Ok(how)) if how.starts_with("по кабелю") => ("Подключена по кабелю USB-C — работают экран, свои эффекты и анимация.".to_string(), true),
                Some(Ok(_)) => ("Подключена по радио 2,4 ГГц. Экран, свои эффекты и анимация раскраски — только по кабелю.".to_string(), true),
                Some(Err(e)) => (format!("Не найдена: {}", kit::ellipsize(e, 110)), false),
                None => ("Поиск…".to_string(), false),
            };
            let (dot, _) = ui.allocate_exact_size(egui::vec2(10.0, 16.0), egui::Sense::hover());
            kit::status_dot(ui.painter(), dot.center(), 4.0, ok);
            kit::cap(ui, text);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Найти клавиатуру").clicked() {
                    self.kb.reconnect();
                }
            });
        });
        ui.add_space(16.0);

        // Вкладки (KbdTabScreen / KbdTabPaint).
        egui::Frame::none().fill(theme::SURFACE).rounding(8.0).inner_margin(4.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                for (i, t) in ["Экран и подсветка", "Раскраска"].into_iter().enumerate() {
                    let sel = self.kb_tab == i as u8;
                    let text = egui::RichText::new(t).font(kit::semibold(13.0)).color(if sel { theme::TEXT } else { theme::MUTED });
                    let b = egui::Button::new(text)
                        .fill(if sel { theme::SURFACE2 } else { egui::Color32::TRANSPARENT })
                        .stroke(egui::Stroke::NONE)
                        .min_size(egui::vec2(150.0, 32.0));
                    if ui.add(b).clicked() {
                        self.kb_tab = i as u8;
                    }
                }
            });
        });
        ui.add_space(16.0);

        if self.kb_tab == 0 {
            let right_w = (ui.available_width() - 348.0).clamp(300.0, 460.0);
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(330.0);
                    kit::card().show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        self.ui_screen(ui, &ks);
                    });
                });
                ui.add_space(18.0);
                ui.vertical(|ui| {
                    ui.set_width(right_w);
                    kit::card().show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        self.ui_light(ui, &ks);
                    });
                });
            });
        } else {
            self.ui_paint(ui, &ks);
        }
    }

    /// «Подсветка»: эффект, радуга, цвет, яркость, скорость, направление, «Применить / Выключить / Подобрать».
    fn ui_light(&mut self, ui: &mut egui::Ui, ks: &rigcontrol_core::kb_light::KbState) {
        ui.label(egui::RichText::new("Подсветка").font(kit::semibold(15.0)).color(theme::TEXT));
        ui.add_space(12.0);
        kit::cap(ui, "Эффект");
        let before = self.s.effect;
        let w = ui.available_width();
        egui::ComboBox::from_id_salt("kb_effect")
            .selected_text(protocol::effect_name(self.s.effect))
            .width(w)
            .height(420.0)
            .show_ui(ui, |ui| {
                for (i, (name, _)) in protocol::EFFECTS.iter().enumerate() {
                    if i == 20 {
                        ui.separator();
                        ui.label(egui::RichText::new("Свои эффекты — только по кабелю").size(11.0).color(theme::MUTED));
                    }
                    ui.selectable_value(&mut self.s.effect, i as i32, *name);
                }
            });
        if self.s.effect != before {
            self.on_effect_changed();
        }

        // Радуга недоступна: у «Разноцветного» и «Спектра» она всегда (в прошивке), при синхронизации цвет
        // берётся из гифки, свои эффекты рисуются выбранным цветом (UpdateRainbowUi в C#).
        ui.add_space(14.0);
        let mode = protocol::effect_mode(self.s.effect);
        let forced = matches!(mode, 6 | 8);
        let custom = rigcontrol_core::effects::is_custom(mode);
        let locked = forced || self.s.sync_light || custom;
        let tip = if forced {
            "«Разноцветный» и «Спектр» в прошивке всегда радужные — отключить нельзя."
        } else if self.s.sync_light {
            "При синхронизации с гифкой цвет берётся из кадров — радуга недоступна."
        } else if rigcontrol_core::effects::is_rainbow_flow(mode) {
            "Этот эффект всегда радужный."
        } else if custom {
            "Свои эффекты рисуются выбранным цветом — радуга к ним не применяется."
        } else {
            "Эффект прошивки переливается всеми цветами, выбранный цвет не используется."
        };
        ui.add_enabled(!locked, egui::Checkbox::new(&mut self.s.rainbow, "Радуга (цвет не используется)"))
            .on_hover_text(tip)
            .on_disabled_hover_text(tip);

        // Подсветка в такт гифке (SyncLightCheck / SyncStyleCombo).
        ui.add_space(10.0);
        ui.checkbox(&mut self.s.sync_light, "Подсветка в такт гифке");
        let styles = ["Картинка гифки на клавишах", "Волна от экрана", "Один цвет кадра"];
        let w = ui.available_width();
        ui.add_enabled_ui(self.s.sync_light, |ui| {
            egui::ComboBox::from_id_salt("kb_sync_style")
                .selected_text(styles[self.s.sync_style.clamp(0, 2) as usize])
                .width(w)
                .show_ui(ui, |ui| {
                    for (i, t) in styles.iter().enumerate() {
                        ui.selectable_value(&mut self.s.sync_style, i as i32, *t);
                    }
                });
        });
        if self.s.sync_light {
            ui.add_space(4.0);
            kit::cap(ui, match self.s.sync_style.clamp(0, 2) {
                0 => "Кадр гифки растянут на клавиши — каждая светит цветом своего участка. Только по кабелю, вместо эффекта.",
                1 => "Главный цвет кадра расходится от экранчика волной, «Скорость» — как быстро. Только по кабелю.",
                _ => "Выбранный эффект перекрашивается в главный цвет кадра. Работает и по радио.",
            });
            if self.scr_frames.len() < 2 {
                kit::cap(ui, "Нужна гифка: выберите её слева, в блоке «Экран».");
            }
        }

        ui.add_space(14.0);
        kit::cap(ui, "Цвет");
        ui.add_space(6.0);
        let rainbow = self.s.rainbow;
        ui.add_enabled_ui(!rainbow, |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(140.0);
                    rigcontrol_core::color_wheel::color_wheel(ui, "kb_light_wheel", &mut self.kb_color, 140.0);
                });
                ui.add_space(16.0);
                ui.vertical(|ui| {
                    swatches(ui, &mut self.kb_color);
                    ui.add_space(6.0);
                    hex_row(ui, "kb_light_hex", &mut self.color_text, &mut self.kb_color);
                });
            });
        });

        ui.add_space(16.0);
        kit::cap(ui, format!("Яркость: {}", self.s.brightness));
        let sw = ui.available_width();
        ui.spacing_mut().slider_width = sw;
        ui.add(egui::Slider::new(&mut self.s.brightness, 0..=5).show_value(false));
        ui.add_space(10.0);
        kit::cap(ui, format!("Скорость: {}", self.s.speed));
        ui.add(egui::Slider::new(&mut self.s.speed, 0..=5).show_value(false));
        ui.add_space(12.0);
        kit::cap(ui, "Направление");
        let dirs = ["Вариант 1 (по умолчанию)", "Вариант 2", "Вариант 3", "Вариант 4"];
        let w = ui.available_width();
        egui::ComboBox::from_id_salt("kb_dir")
            .selected_text(dirs[self.s.direction.clamp(0, 3) as usize])
            .width(w)
            .show_ui(ui, |ui| {
                for (i, d) in dirs.iter().enumerate() {
                    ui.selectable_value(&mut self.s.direction, i as i32, *d);
                }
            });

        ui.add_space(20.0);
        let ready = ks.connected();
        ui.horizontal(|ui| {
            let apply = egui::Button::new(egui::RichText::new("Применить").font(kit::semibold(14.0)).color(egui::Color32::BLACK))
                .fill(theme::TEXT)
                .min_size(egui::vec2(120.0, 38.0));
            if ui.add_enabled(ready, apply).clicked() {
                self.demo = None;
                self.sync_colors();
                self.save();
                self.kb.apply_now(&self.s);
            }
            let off_text = if ks.off { "Включить" } else { "Выключить" };
            if ui.add_enabled(ready, egui::Button::new(off_text).min_size(egui::vec2(104.0, 38.0))).clicked() {
                self.demo = None;
                self.kb.set_on(ks.off);
            }
            let demo_text = if self.demo.is_some() { "Остановить" } else { "Подобрать" };
            if ui
                .add_enabled(ready, egui::Button::new(demo_text).min_size(egui::vec2(104.0, 38.0)))
                .on_hover_text("Показать по очереди эффекты с плавной волной, по 4 секунды каждый")
                .clicked()
            {
                if self.demo.is_some() {
                    self.demo = None;
                    self.msg = "Проверка остановлена.".into();
                } else {
                    self.demo = Some((0, std::time::Instant::now() - Duration::from_secs(10)));
                }
            }
        });
        if let Some(n) = &ks.note {
            ui.add_space(8.0);
            ui.add(egui::Label::new(egui::RichText::new(n.clone()).size(12.5).color(theme::BAD)).wrap());
        }
        ui.add_space(12.0);
        kit::cap(ui, "Свои эффекты (в конце списка) и анимация раскраски — только по кабелю.");
    }

    /// Смена эффекта в списке: «Радуга» сама у «Разноцветного» и «Спектра» (OnEffectChanged в C#).
    fn on_effect_changed(&mut self) {
        if matches!(protocol::effect_mode(self.s.effect), 6 | 8) {
            self.s.rainbow = true;
            self.rainbow_auto = true;
        } else if self.rainbow_auto {
            self.s.rainbow = false;
            self.rainbow_auto = false;
        }
    }

    /// «Подобрать» (OnEffectDemo): эффекты с плавной волной по 4 секунды; звать каждый кадр.
    fn tick_demo(&mut self, ctx: &egui::Context) {
        const CANDIDATES: [i32; 8] = [9, 16, 10, 18, 12, 15, 17, 19];
        let Some((step, at)) = self.demo else { return };
        if at.elapsed() >= Duration::from_secs(4) {
            let next = if self.s.effect == CANDIDATES[step.min(7)] && at.elapsed() < Duration::from_secs(9) { step + 1 } else { step };
            if next >= CANDIDATES.len() {
                self.demo = None;
                self.msg = "Проверка закончена. Выберите эффект, который понравился.".into();
                return;
            }
            self.s.effect = CANDIDATES[next];
            self.on_effect_changed();
            self.msg = format!(
                "Проверка: «{}» — 4 секунды. Нажмите «Остановить», чтобы оставить этот эффект.",
                protocol::effect_name(self.s.effect)
            );
            self.demo = Some((next, std::time::Instant::now()));
        }
        ctx.request_repaint_after(Duration::from_millis(250));
    }

    /// «Раскраска» (KbdPaintPanel): схема, «в реальном времени», кисть, наборы, анимация.
    fn ui_paint(&mut self, ui: &mut egui::Ui, ks: &rigcontrol_core::kb_light::KbState) {
        kit::card().inner_margin(egui::Margin::same(14.0)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let mirror = self.s.paint_mirror;
            self.painter.canvas(ui, &mut self.s, mirror);
            ui.add_space(14.0);
            ui.horizontal_top(|ui| {
                ui.checkbox(&mut self.s.paint_mirror, "");
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new("Подсветка в реальном времени").font(kit::semibold(13.0)).color(theme::TEXT));
                    kit::cap(ui, "На схеме — текущие цвета клавиатуры. Во время рисования показывается ваша раскраска.");
                    if self.s.paint_mirror {
                        if let Some(h) = KeyPainter::mirror_hint() {
                            kit::cap(ui, h);
                        }
                    }
                });
            });
        });
        ui.add_space(14.0);
        let half = ((ui.available_width() - 14.0) / 2.0).max(300.0);
        let mut brush = self.painter.brush;
        let mut apply = false;
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(half);
                kit::card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(egui::RichText::new("Кисть").font(kit::semibold(15.0)).color(theme::TEXT));
                    kit::cap(ui, "ЛКМ — красить · Shift — стереть · ПКМ — пипетка");
                    ui.add_space(10.0);
                    ui.horizontal_top(|ui| {
                        ui.vertical(|ui| {
                            ui.set_width(150.0);
                            rigcontrol_core::color_wheel::color_wheel(ui, "kb_paint_wheel", &mut brush, 150.0);
                        });
                        ui.add_space(16.0);
                        ui.vertical(|ui| {
                            swatches(ui, &mut brush);
                            ui.add_space(6.0);
                            hex_row(ui, "kb_paint_hex", &mut self.paint_text, &mut brush);
                        });
                    });
                });
            });
            ui.add_space(14.0);
            ui.vertical(|ui| {
                ui.set_width(half);
                kit::card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(egui::RichText::new("Наборы").font(kit::semibold(15.0)).color(theme::TEXT));
                    ui.add_space(10.0);
                    ui.horizontal_wrapped(|ui| {
                        for (i, (name, _)) in keypaint::PRESETS.iter().enumerate() {
                            if ui.button(*name).clicked() {
                                self.painter.preset(i, &mut self.s);
                            }
                        }
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button("Залить всё").clicked() {
                            self.painter.fill_all(&mut self.s);
                        }
                        if ui.button("Очистить").clicked() {
                            self.painter.clear(&mut self.s);
                        }
                        if ui.add_enabled(self.painter.can_undo(), egui::Button::new("Отменить")).clicked() {
                            self.painter.undo(&mut self.s);
                        }
                    });
                    ui.add_space(18.0);
                    ui.label(egui::RichText::new("Анимация").font(kit::semibold(15.0)).color(theme::TEXT));
                    ui.add_space(8.0);
                    let w = ui.available_width();
                    let anim = self.s.paint_anim.clamp(0, keypaint::ANIMATIONS.len() as i32 - 1);
                    egui::ComboBox::from_id_salt("kb_paint_anim")
                        .selected_text(keypaint::ANIMATIONS[anim as usize])
                        .width(w)
                        .show_ui(ui, |ui| {
                            for (i, n) in keypaint::ANIMATIONS.iter().enumerate() {
                                ui.selectable_value(&mut self.s.paint_anim, i as i32, *n);
                            }
                        });
                    ui.add_space(10.0);
                    kit::cap(ui, format!("Скорость: {}", self.s.paint_speed));
                    let sw = ui.available_width();
                    ui.spacing_mut().slider_width = sw;
                    ui.add(egui::Slider::new(&mut self.s.paint_speed, 0..=5).show_value(false));
                    kit::cap(ui, "Анимация — только по кабелю.");
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        let b = egui::Button::new(egui::RichText::new("Применить раскраску").font(kit::semibold(14.0)).color(egui::Color32::BLACK))
                            .fill(theme::TEXT)
                            .min_size(egui::vec2(170.0, 36.0));
                        if ui.add_enabled(ks.connected(), b).on_hover_text("Включить на клавиатуре «Свою раскраску»").clicked() {
                            apply = true;
                        }
                        kit::cap(ui, format!("Закрашено {} из {}", self.painter.count(), keypaint::KEYS.len()));
                    });
                });
            });
        });
        if brush != self.painter.brush {
            self.painter.set_brush(brush, &mut self.s);
        }
        if apply {
            if let Some(i) = protocol::effect_index(rigcontrol_core::effects::PAINT) {
                if self.s.effect != i {
                    self.s.effect = i;
                    self.on_effect_changed();
                }
            }
            self.save();
            self.kb.apply_now(&self.s);
        }
    }

    fn scr_load(&mut self) {
        let path = self.scr_path.trim().trim_matches('"').to_string();
        if path.is_empty() {
            return;
        }
        let max = self.s.max_frames.max(0) as usize;
        match screen::load(std::path::Path::new(&path), self.s.crop, max) {
            Ok(f) => {
                self.scr_frames = f;
                self.scr_t0 = std::time::Instant::now();
                self.s.last_image = Some(path);
            }
            Err(e) => {
                self.scr_frames.clear();
                self.msg = format!("Экран: {e}");
            }
        }
        self.scr_tex = None;
    }

    /// «Экран» (левая колонка): предпросмотр 128×128, выбор файла, кадр, число кадров, заливка, часы.
    fn ui_screen(&mut self, ui: &mut egui::Ui, ks: &rigcontrol_core::kb_light::KbState) {
        // Картинка экрана — при первом входе на страницу, а не при запуске программы.
        if !self.scr_loaded {
            self.scr_loaded = true;
            if !self.scr_path.is_empty() {
                self.scr_load();
            }
        }
        ui.label(egui::RichText::new("Экран").font(kit::semibold(15.0)).color(theme::TEXT));
        ui.add_space(12.0);
        let dropped = ui.ctx().input(|i| i.raw.dropped_files.first().and_then(|f| f.path.clone()));
        if let Some(p) = dropped {
            self.scr_path = p.display().to_string();
            self.scr_load();
        }

        // Предпросмотр: рамка 3 px, скругление 10, внутри 244×244 (как PreviewImage в C#).
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 278.0), egui::Sense::hover());
        let frame = egui::Rect::from_center_size(rect.center(), egui::vec2(278.0, 278.0));
        ui.painter().rect(frame, 10.0, egui::Color32::BLACK, egui::Stroke::new(3.0_f32, theme::LINE));
        let img_rect = egui::Rect::from_center_size(frame.center(), egui::vec2(244.0, 244.0));
        if !self.scr_frames.is_empty() {
            let t = self.scr_t0.elapsed().as_millis() as u64;
            let idx = screen_upload::frame_at(&self.scr_frames, t).min(self.scr_frames.len() - 1);
            if self.scr_tex.is_none() || idx != self.scr_idx {
                let n = protocol::SCREEN_SIZE;
                let rgb = screen_upload::rgb565_to_rgb(&self.scr_frames[idx].rgb565);
                if rgb.len() == n * n * 3 {
                    let img = egui::ColorImage::from_rgb([n, n], &rgb);
                    match &mut self.scr_tex {
                        Some(tex) => tex.set(img, egui::TextureOptions::NEAREST),
                        None => self.scr_tex = Some(ui.ctx().load_texture("kb-screen", img, egui::TextureOptions::NEAREST)),
                    }
                }
                self.scr_idx = idx;
            }
            if let Some(tex) = &self.scr_tex {
                let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                ui.painter().image(tex.id(), img_rect, uv, egui::Color32::WHITE);
            }
            if self.scr_frames.len() > 1 {
                ui.ctx().request_repaint_after(Duration::from_millis(20));
            }
        } else {
            ui.painter().text(img_rect.center(), egui::Align2::CENTER_CENTER, "128 × 128", egui::FontId::proportional(13.0), theme::MUTED);
        }
        ui.add_space(10.0);
        let info = if self.scr_frames.is_empty() {
            "Картинка не выбрана".to_string()
        } else {
            let n = self.scr_frames.len();
            format!("{} · {n} {}", kit::ellipsize(&file_name(&self.scr_path), 26), kit::plural(n, "кадр", "кадра", "кадров"))
        };
        kit::cap(ui, info);

        let busy = self.scr_upload.as_ref().is_some_and(|u| u.running());
        let w = ui.available_width();
        ui.add_space(12.0);
        if ui.add_enabled(!busy, egui::Button::new("Выбрать картинку или гифку…").min_size(egui::vec2(w, 34.0))).clicked() {
            if let Some(p) = rfd::FileDialog::new()
                .add_filter("Картинки и гифки", &["png", "jpg", "jpeg", "gif", "bmp"])
                .pick_file()
            {
                self.scr_path = p.display().to_string();
                self.scr_load();
            }
        }

        let mut reload = false;
        ui.add_space(14.0);
        kit::cap(ui, "Кадр");
        ui.add_enabled_ui(!busy, |ui| {
            egui::ComboBox::from_id_salt("kb_fit")
                .selected_text(if self.s.crop { "Обрезать по краям" } else { "Вписать целиком" })
                .width(w)
                .show_ui(ui, |ui| {
                    reload |= ui.selectable_value(&mut self.s.crop, true, "Обрезать по краям").changed();
                    reload |= ui.selectable_value(&mut self.s.crop, false, "Вписать целиком").changed();
                });
        });
        ui.add_space(12.0);
        kit::cap(ui, "Кадров в гифке");
        let counts = [(0, "Все"), (60, "не больше 60"), (40, "не больше 40"), (20, "не больше 20"), (10, "не больше 10")];
        let cur = counts.iter().find(|c| c.0 == self.s.max_frames).map(|c| c.1).unwrap_or("Все");
        ui.add_enabled_ui(!busy, |ui| {
            egui::ComboBox::from_id_salt("kb_frames").selected_text(cur).width(w).show_ui(ui, |ui| {
                for (v, t) in counts {
                    reload |= ui.selectable_value(&mut self.s.max_frames, v, t).changed();
                }
            });
        });
        if reload {
            self.scr_load();
        }

        // Заливка и итог (UploadProgress).
        if let Some(u) = &self.scr_upload {
            match u.state() {
                UploadState::Running { done, total } => {
                    ui.add_space(14.0);
                    let f = if total == 0 { 0.0 } else { done as f32 / total as f32 };
                    ui.add(egui::ProgressBar::new(f).text(format!("{done}/{total} — не отключайте клавиатуру")));
                    ui.ctx().request_repaint_after(Duration::from_millis(100));
                }
                UploadState::Done(m) if !self.scr_reported => {
                    self.scr_reported = true;
                    self.msg = m;
                }
                UploadState::Failed(e) if !self.scr_reported => {
                    self.scr_reported = true;
                    self.msg = format!("Экран: {e}");
                }
                _ => {}
            }
        }
        ui.add_space(14.0);
        let can_upload = !busy && ks.wired() && !self.scr_frames.is_empty();
        let upload = egui::Button::new(egui::RichText::new("Отправить на экран").font(kit::semibold(14.0)).color(egui::Color32::BLACK))
            .fill(theme::TEXT)
            .min_size(egui::vec2(w, 40.0));
        if ui.add_enabled(can_upload, upload).clicked() {
            self.scr_reported = false;
            self.scr_upload = Some(self.kb.upload(self.scr_frames.clone()));
        }
        ui.add_space(8.0);
        if ui.add_enabled(!busy && ks.connected(), egui::Button::new("Синхронизировать часы").min_size(egui::vec2(w, 34.0))).clicked() {
            self.kb.sync_time();
        }
        ui.add_space(10.0);
        kit::cap(ui, "PNG, JPG, GIF · 128×128 · заливка только по кабелю.");
    }

    /// «Подсветка СЖО» — PageArgb: живой предпросмотр цепочки вентиляторов (тот же движок эффектов, что
    /// шлёт кадры на AURA), карточки режимов с маленькими анимированными кольцами, настройки на стекле.
    /// «Подсветка СЖО» — PageArgb: настоящая картинка СЖО, на которой светодиоды горят тем же кадром,
    /// что уходит на AURA (тот же argb::Renderer), карточки режимов с вентилятором в этом режиме, настройки.
    fn ui_argb(&mut self, ui: &mut egui::Ui) {
        use rigcontrol_core::led_art::Art;
        use rigcontrol_core::service::{argb_config, argb_mode_from_settings};
        let time = ui.input(|i| i.time);
        ui.ctx().request_repaint_after(Duration::from_millis(33));
        self.sync_colors();
        let cpu_temp = rigcontrol_core::telemetry::live().and_then(|s| s.cpu_temp);
        let temp = cpu_temp.unwrap_or(f64::NAN);
        // Как видно глазу (Preview в C#): мощность светодиодов с поправкой на гамму.
        let bri = (rigcontrol_core::argb::led_brightness(self.s.argb_brightness) as f32 / 100.0).powf(1.0 / 2.2);
        self.poll_armoury();
        let leds = self.s.argb_leds.clamp(1, rigcontrol_core::argb::MAX_LEDS as i32) as usize;
        let fans = self.s.argb_fans.clamp(1, 6) as usize;
        let managed = argb_mode_from_settings(self.s.argb_mode);
        let mut frame = vec![0u8; leds * 3];
        if let Some(m) = managed {
            self.argb_hero.render(&argb_config(&self.s, m), time, temp, &mut frame);
        }

        // ── «Сейчас горит»: СЖО целиком ──
        kit::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            let status = self.svc_status("ARGB").unwrap_or_default();
            let mode_name = ARGB_MODES.iter().find(|m| m.0 == self.s.argb_mode).map(|m| m.1).unwrap_or("?");
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    kit::cap(ui, "СЕЙЧАС ГОРИТ");
                    ui.label(egui::RichText::new(mode_name).font(kit::semibold(19.0)).color(theme::TEXT));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let ok = managed.is_some() && !status.is_empty() && !status.starts_with("нет связи");
                    let line = if managed.is_none() {
                        "Не управляется: подсветкой занимается плата или Armoury Crate".to_string()
                    } else if status.is_empty() {
                        "Ищу контроллер AURA…".to_string()
                    } else {
                        kit::ellipsize(&status, 70)
                    };
                    if managed.is_some()
                        && ui.button("Найти заново").on_hover_text("Переподключиться к контроллеру ASUS Aura").clicked()
                    {
                        self.restart(false);
                    }
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(line).size(12.5).color(theme::MUTED));
                    let (dot, _) = ui.allocate_exact_size(egui::vec2(10.0, 16.0), egui::Sense::hover());
                    kit::status_dot(ui.painter(), dot.center(), 4.0, ok);
                });
            });
            ui.add_space(8.0);
            let w = ui.available_width();
            let (area, _) = ui.allocate_exact_size(egui::vec2(w, (w * 0.5).clamp(220.0, 330.0)), egui::Sense::hover());
            let lit = if managed.is_some() { Some(frame.as_slice()) } else { None };
            self.led_art.paint(ui, area, Art::Cooler, lit, leds, fans, bri, None);
            if managed.is_some() {
                kit::cap(ui, "Подсветка на картинке идёт слева направо, как на СЖО: первый светодиод цепочки — слева.");
            }
        });
        ui.add_space(16.0);
        self.ui_armoury(ui);

        // ── Режимы: на каждой карточке вентилятор горит этим режимом ──
        kit::h2(ui, "Режим");
        ui.add_space(10.0);
        let mut picked = None;
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(12.0, 12.0);
            ui.horizontal_wrapped(|ui| {
                for (n, (v, name)) in ARGB_MODES.iter().enumerate() {
                    let (r, resp) = ui.allocate_exact_size(egui::vec2(236.0, 96.0), egui::Sense::click());
                    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                    let hover = ui.ctx().animate_bool(resp.id, resp.hovered());
                    let sel = self.s.argb_mode == *v;
                    let stroke = if sel {
                        egui::Stroke::new(1.5_f32, theme::TEXT)
                    } else {
                        egui::Stroke::new(1.0_f32, if hover > 0.5 { kit::LINE_HOVER } else { theme::LINE })
                    };
                    ui.painter().rect(r, 12.0, theme::CARD, stroke);
                    let mut mini = [0u8; 16 * 3];
                    let mode = argb_mode_from_settings(*v);
                    if let (Some(m), Some(rd)) = (mode, self.argb_minis.get_mut(n)) {
                        let mut cfg = argb_config(&self.s, m);
                        cfg.leds = 16;
                        cfg.fans = 1;
                        rd.render(&cfg, time, temp, &mut mini);
                    }
                    let art = egui::Rect::from_min_size(r.left_top() + egui::vec2(12.0, 10.0), egui::vec2(76.0, 76.0));
                    let lit = if mode.is_some() { Some(&mini[..]) } else { None };
                    self.led_art.paint(ui, art, Art::Fan, lit, 16, 1, 1.0, None);
                    let p = ui.painter();
                    let x0 = r.left() + 100.0;
                    p.text(egui::pos2(x0, r.top() + 26.0), egui::Align2::LEFT_TOP, kit::ellipsize(name, 18), kit::semibold(14.0), theme::TEXT);
                    p.text(egui::pos2(x0, r.top() + 50.0), egui::Align2::LEFT_TOP, kit::ellipsize(argb_hint(*v), 24), egui::FontId::proportional(12.0), theme::MUTED);
                    if resp.clicked() {
                        picked = Some(*v);
                    }
                }
            });
        });
        if let Some(v) = picked {
            self.s.argb_mode = v;
        }
        ui.add_space(20.0);

        // ── Настройки режима ──
        let mode = self.s.argb_mode;
        if mode == 0 {
            return;
        }
        let board = self.s.argb_board_effect;
        let uses_color = matches!(mode, 2 | 3 | 5 | 6);
        let uses_speed = !board && matches!(mode, 3 | 4 | 5 | 6);
        let hint = argb_mode_hint(mode);
        if !hint.is_empty() {
            kit::cap(ui, hint);
            ui.add_space(12.0);
        }
        let total = ui.available_width();
        let left_w = ((total - 18.0) / 2.0).max(300.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(left_w);
                kit::card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(egui::RichText::new("Эффект").font(kit::semibold(15.0)).color(theme::TEXT));
                    ui.add_space(10.0);
                    if mode == 5 {
                        ui.checkbox(&mut self.s.argb_rainbow, "Радуга")
                            .on_hover_text("Выключите — волна пойдёт тремя своими цветами, они плавно смешаются без тёмных провалов.");
                        ui.add_space(8.0);
                    }
                    let palette = mode == 5 && !self.s.argb_rainbow;
                    if uses_color && !(mode == 5 && self.s.argb_rainbow) {
                        kit::cap(ui, if palette { "Цвета волны" } else { "Цвет" });
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            let n = if palette { 3 } else { 1 };
                            for k in 0..n {
                                rigcontrol_core::color_wheel::wheel_button(ui, ("argb", k), &mut self.argb_colors[k]);
                                ui.add_space(6.0);
                            }
                        });
                        ui.add_space(6.0);
                        swatches(ui, &mut self.argb_colors[0]);
                        if palette {
                            kit::cap(ui, "Палитра меняет первый цвет; второй и третий — кругом.");
                        }
                        ui.add_space(12.0);
                    }
                    if mode != 1 {
                        let b = self.s.argb_brightness;
                        kit::cap(ui, if b == 0 { "Яркость: выключено".to_string() } else { format!("Яркость: {b} %") });
                        ui.add(egui::Slider::new(&mut self.s.argb_brightness, 0..=100).show_value(false));
                        ui.add_space(10.0);
                    }
                    if uses_speed {
                        kit::cap(ui, "Скорость");
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            for v in 1..=5 {
                                if pill(ui, &v.to_string(), self.s.argb_speed == v).clicked() {
                                    self.s.argb_speed = v;
                                }
                            }
                        });
                        ui.add_space(10.0);
                    }
                    match mode {
                        1 => kit::cap(ui, "Все светодиоды погашены."),
                        7 => {
                            let t = cpu_temp.map(|t| format!("{t:.0} °C")).unwrap_or_else(|| "нет датчика".into());
                            kit::cap(ui, format!("CPU сейчас: {t}."))
                        }
                        8 => kit::cap(ui, "Цвета и скорость берутся у эффекта клавиатуры."),
                        _ => ui.label(""),
                    };
                });
            });
            ui.add_space(18.0);
            ui.vertical(|ui| {
                ui.set_width((total - left_w - 18.0).max(260.0));
                kit::card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(egui::RichText::new("Подключение").font(kit::semibold(15.0)).color(theme::TEXT));
                    ui.add_space(10.0);
                    kit::cap(ui, "Способ");
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        if pill(ui, "Программа", !self.s.argb_board_effect).clicked() {
                            self.s.argb_board_effect = false;
                        }
                        if pill(ui, "Эффект платы", self.s.argb_board_effect).clicked() {
                            self.s.argb_board_effect = true;
                        }
                    });
                    kit::cap(ui, if self.s.argb_board_effect {
                        "Эффект крутит сам контроллер платы — работает и после выхода из программы, но только простые: цвет, дыхание, смена цветов, радуга. Скорость не настраивается."
                    } else {
                        "Каждый кадр рисует программа (до 80 в секунду): работают все эффекты, пока RigControl запущена."
                    });
                    ui.add_space(10.0);
                    kit::cap(ui, "Вентиляторов в цепочке");
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        for v in 1..=6 {
                            if pill(ui, &v.to_string(), self.s.argb_fans == v).clicked() {
                                self.s.argb_fans = v;
                            }
                        }
                    });
                    ui.add_space(10.0);
                    kit::cap(ui, format!("Светодиодов на разъёме: {}", self.s.argb_leds));
                    ui.add(egui::Slider::new(&mut self.s.argb_leds, 1..=rigcontrol_core::argb::MAX_LEDS as i32).show_value(false));
                    ui.add_space(10.0);
                    kit::cap(ui, "Разъём ARGB на плате");
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        // Сколько разъёмов у платы — по ответу контроллера (пока не нашли — три, как у AURA).
                        let headers = self.svc_status("ARGB·разъёмы").and_then(|x| x.parse::<i32>().ok()).unwrap_or(3).clamp(1, 8);
                        for v in 0..=headers {
                            let t = if v == 0 { "Все".to_string() } else { format!("ARGB {v}") };
                            if pill(ui, &t, self.s.argb_target == v).clicked() {
                                self.s.argb_target = v;
                            }
                        }
                    });
                });
            });
        });
        if mode == 8 {
            ui.add_space(16.0);
            kit::card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                self.ui_effect_rate(ui);
            });
        }
    }

    /// Armoury Crate каждые 5 с: работает ли его служба подсветки (в своём потоке — tasklist небыстрый).
    fn poll_armoury(&mut self) {
        if let Some(rx) = &self.armoury_rx {
            if let Ok(v) = rx.try_recv() {
                self.armoury_running = Some(v);
                self.armoury_rx = None;
            }
        }
        if self.armoury_rx.is_none() && self.armoury_at.map(|t| t.elapsed() > Duration::from_secs(5)).unwrap_or(true) {
            self.armoury_at = Some(std::time::Instant::now());
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(rigcontrol_core::armoury::running());
            });
            self.armoury_rx = Some(rx);
        }
        if let Some(rx) = &self.armoury_op {
            if let Ok((stopped, r)) = rx.try_recv() {
                self.armoury_op = None;
                match r {
                    Ok(()) => {
                        self.s.armoury_stopped = stopped;
                        self.armoury_running = Some(!stopped);
                        self.save();
                        self.msg = if stopped {
                            "Служба подсветки Armoury Crate остановлена.".into()
                        } else {
                            "Служба подсветки Armoury Crate снова работает.".into()
                        };
                        // Контроллер мог быть занят службой — подключаемся заново.
                        self.restart(false);
                    }
                    Err(e) => self.msg = format!("Не получилось: {e}"),
                }
            }
        }
    }

    /// Карточка Armoury Crate (ArmouryPanel / ArmouryOffPanel в C#).
    fn ui_armoury(&mut self, ui: &mut egui::Ui) {
        let controlled = self.s.argb_mode != 0;
        let running = self.armoury_running.unwrap_or(false);
        let busy = self.armoury_op.is_some();
        let show_running = controlled && running;
        let show_stopped = self.s.armoury_stopped && !running && self.armoury_running.is_some();
        if !show_running && !show_stopped {
            self.armoury_confirm = false;
            return;
        }
        kit::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_max_width((ui.available_width() - 260.0).max(220.0));
                    if show_running {
                        ui.label(egui::RichText::new("Armoury Crate перебивает подсветку").font(kit::semibold(14.0)).color(theme::BAD));
                        kit::cap(ui, "Работает его служба LightingService: она забирает плату и меняет цвета программы. Остановите её — пока она стоит, Armoury Crate не трогает подсветку платы и других устройств ASUS.");
                    } else {
                        ui.label(egui::RichText::new("Служба Armoury Crate остановлена").font(kit::semibold(14.0)).color(theme::TEXT));
                        kit::cap(ui, "Её остановила программа. Вернуть — кнопкой справа (запустится и будет стартовать с Windows).");
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if busy {
                        ui.spinner();
                    } else if show_running {
                        if self.armoury_confirm {
                            if ui.button("Отмена").clicked() {
                                self.armoury_confirm = false;
                            }
                            if ui.button(egui::RichText::new("Да, остановить").color(theme::BAD)).clicked() {
                                self.armoury_confirm = false;
                                let (tx, rx) = std::sync::mpsc::channel();
                                std::thread::spawn(move || {
                                    let _ = tx.send((true, rigcontrol_core::armoury::stop()));
                                });
                                self.armoury_op = Some(rx);
                            }
                        } else if ui.button("Остановить службу").clicked() {
                            self.armoury_confirm = true;
                        }
                    } else if ui.button("Вернуть службу").clicked() {
                        let (tx, rx) = std::sync::mpsc::channel();
                        std::thread::spawn(move || {
                            let _ = tx.send((false, rigcontrol_core::armoury::start()));
                        });
                        self.armoury_op = Some(rx);
                    }
                });
            });
        });
        ui.add_space(16.0);
    }

    /// Подгонка скорости: вентиляторы повторяют эффект прошивки клавиатуры и должны идти в такт.
    /// Как в C# (MainWindow.Argb.cs): ручной множитель или замер «старт → круг → … → готово».
    fn ui_effect_rate(&mut self, ui: &mut egui::Ui) {
        use rigcontrol_core::light_mirror as lm;
        ui.label(egui::RichText::new("Скорость эффекта клавиатуры на вентиляторах").strong());
        let Some((mode, speed)) = lm::active_effect() else {
            ui.label("На клавиатуре сейчас не встроенный эффект (или программа его ещё не выставляла) — подгонять нечего.");
            self.cal_start = None;
            return;
        };
        ui.label(format!("Эффект прошивки №{mode}, скорость {speed}"));
        let mut rate = lm::rate_of(mode, speed);
        let old = rate;
        ui.horizontal(|ui| {
            ui.label("Множитель");
            ui.add(egui::DragValue::new(&mut rate).speed(0.005).range(0.1..=10.0).fixed_decimals(3));
            if ui.button("Сбросить").clicked() {
                rate = if mode == 16 { 0.92 } else { 1.0 };
            }
        });
        if (rate - old).abs() > 1e-6 {
            lm::set_rate(mode, speed, rate);
            self.save_rates();
        }
        if !lm::can_calibrate(mode) {
            ui.label("Этот эффект не повторяется сам — замерять нечего.");
            return;
        }
        ui.label("Замер: нажмите «Старт», когда на клавиатуре начинается круг («Спектр» — когда она синяя), «Круг» — на каждом повторе, потом «Готово».");
        ui.horizontal(|ui| match self.cal_start {
            None => {
                if ui.button("Старт").clicked() {
                    self.cal_start = Some(std::time::Instant::now());
                    self.cal_circles = 0;
                    if mode == 8 {
                        lm::restart_effect_clock();
                    }
                }
            }
            Some(t0) => {
                let secs = t0.elapsed().as_secs_f64();
                ui.label(format!("{secs:.1} с, кругов: {}", self.cal_circles));
                if ui.button("Круг").clicked() {
                    self.cal_circles += 1;
                }
                if ui.button("Готово").clicked() {
                    self.cal_start = None;
                    self.msg = match lm::calibrate(secs, self.cal_circles) {
                        Some(r) => {
                            self.save_rates();
                            format!("Множитель скорости: {r:.3}")
                        }
                        None => "Замер не засчитан: нужен хотя бы один круг длиннее полсекунды".into(),
                    };
                }
                if ui.button("Отмена").clicked() {
                    self.cal_start = None;
                }
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
            }
        });
        if ui.button("Сбросить все подгонки").clicked() {
            lm::set_rates(None);
            self.save_rates();
        }
    }

    /// Подгонки скорости — в settings.json (EffectRates), как в C#.
    fn save_rates(&mut self) {
        let rates = rigcontrol_core::light_mirror::rates();
        self.s.effect_rates = if rates.is_empty() { None } else { Some(rates) };
        if let Err(e) = self.s.save() {
            self.msg = format!("Не удалось сохранить настройки: {e}");
        }
    }

    /// «Экран помпы» — PagePump из MainWindow.xaml: статус и кнопки, режим, редактор сцены,
    /// темы с картинками, фон, вывод на экран (частота, поворот, размер), диагностика, сброс USB.
    fn ui_pump(&mut self, ui: &mut egui::Ui) {
        let time = ui.input(|i| i.time);
        // Сброс USB закончился — подключаемся заново.
        let done = self.pump_reset.as_ref().map(|rx| rx.try_recv());
        if let Some(done) = done {
            let done = match done {
                Ok(r) => Ok(r),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => Ok(Err("поток сброса остановился".to_string())),
                Err(e) => Err(e),
            };
            if let Ok(r) = done {
                self.pump_reset = None;
                self.pump_off = false;
                self.restart(false);
                self.msg = match r {
                    Ok(()) => "USB экрана сброшен, подключаюсь заново.".into(),
                    Err(e) => format!("Не удалось сбросить USB экрана: {e}"),
                };
            } else {
                ui.ctx().request_repaint_after(Duration::from_millis(200));
            }
        }
        let status = self.svc_status("Помпа").unwrap_or_default();
        let diag = self.svc_status("Помпа·диагностика").unwrap_or_default();
        let resetting = self.pump_reset.is_some();
        let (title, detail, ok) = if resetting {
            ("Сбрасываю USB экрана…".to_string(), "Windows переподключает помпу, это около 5 секунд.".to_string(), false)
        } else if self.pump_off {
            ("Экран остановлен".to_string(), "Нажмите «Запустить», чтобы выводить сцену на помпу.".to_string(), false)
        } else if status.starts_with("нет связи") {
            ("Помпа не найдена".to_string(), status.clone(), false)
        } else if status.is_empty() {
            ("Ищу помпу…".to_string(), String::new(), false)
        } else {
            ("Экран работает".to_string(), status.clone(), true)
        };

        // ── Шапка: состояние и кнопки ──
        kit::card().inner_margin(egui::Margin::symmetric(18.0, 14.0)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                if resetting {
                    let a = (time * 3.0).sin() as f32 * 0.5 + 0.5;
                    ui.painter().circle_filled(dot.center(), 5.0, kit::lerp_color(theme::MUTED, theme::TEXT, a));
                } else {
                    kit::status_dot(ui.painter(), dot.center(), 5.0, ok);
                }
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(title).font(kit::semibold(15.0)).color(theme::TEXT));
                    if !detail.is_empty() {
                        ui.label(egui::RichText::new(kit::ellipsize(&detail, 110)).size(12.0).color(theme::MUTED));
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_enabled_ui(!resetting, |ui| {
                        if ui.button("Сбросить USB экрана")
                            .on_hover_text("Удалить устройство экрана и пересканировать USB, чтобы Windows подняла его заново (как в C#-версии). Помогает, когда помпа зависла на логотипе.")
                            .clicked()
                        {
                            self.stop_service();
                            let (tx, rx) = std::sync::mpsc::channel();
                            std::thread::spawn(move || {
                                let _ = tx.send(rigcontrol_core::service::reset_pump_usb());
                            });
                            self.pump_reset = Some(rx);
                        }
                        if ui.button("Найти помпу заново").clicked() {
                            self.pump_off = false;
                            self.restart(false);
                        }
                        let label = if self.pump_off { "Запустить" } else { "Остановить" };
                        if ui.add(egui::Button::new(egui::RichText::new(label).font(kit::semibold(13.0)))).clicked() {
                            self.pump_off = !self.pump_off;
                            self.restart(false);
                        }
                    });
                });
            });
        });
        ui.add_space(14.0);

        // ── Режим ──
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if pill(ui, "Редактор экрана", self.s.cooler_mode != 1).clicked() {
                self.s.cooler_mode = 0;
            }
            if pill(ui, "Картинка или гифка", self.s.cooler_mode == 1).clicked() {
                self.s.cooler_mode = 1;
            }
        });
        ui.add_space(14.0);

        let total = ui.available_width();
        let side = 330.0_f32.min(total * 0.42);
        let gap = 16.0;
        let main = (total - side - gap).max(320.0);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            ui.vertical(|ui| {
                ui.set_width(main);
                kit::card().inner_margin(egui::Margin::same(18.0)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    if self.s.cooler_mode == 1 {
                        self.ui_pump_image(ui);
                    } else {
                        kit::h2(ui, "Редактор экрана");
                        ui.add_space(6.0);
                        let detected = self.pump_detected_size();
                        self.editor.set_panel(&self.s, detected);
                        self.editor.show(ui, &mut self.s);
                    }
                });
            });
            ui.vertical(|ui| {
                ui.set_width(side);
                if self.s.cooler_mode != 1 {
                    kit::card().inner_margin(egui::Margin::same(16.0)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        kit::h2(ui, "Темы");
                        ui.add_space(6.0);
                        self.ui_pump_themes(ui);
                    });
                    ui.add_space(12.0);
                    kit::card().inner_margin(egui::Margin::same(16.0)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        kit::h2(ui, "Фон");
                        ui.add_space(6.0);
                        self.ui_pump_background(ui);
                    });
                    ui.add_space(12.0);
                }
                kit::card().inner_margin(egui::Margin::same(16.0)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    kit::h2(ui, "Вывод на экран");
                    ui.add_space(6.0);
                    self.ui_pump_output(ui, &diag, time);
                });
            });
        });
    }

    /// Режим «Картинка или гифка»: выбор файла и предпросмотр (кадры идут как на помпе).
    fn ui_pump_image(&mut self, ui: &mut egui::Ui) {
        kit::h2(ui, "Картинка или гифка");
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.button("Выбрать картинку или гифку…").clicked() {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("Картинки и гифки", &["png", "jpg", "jpeg", "gif", "bmp"])
                    .pick_file()
                {
                    self.s.cooler_image = Some(p.display().to_string());
                }
            }
            ui.label(muted_text(file_name(self.s.cooler_image.as_deref().unwrap_or(""))));
        });
        ui.checkbox(&mut self.s.crop, "Обрезать под экран (иначе — вписать с полями)");
        ui.add_space(8.0);
        let path = self.s.cooler_image.clone().unwrap_or_default();
        if path.trim().is_empty() {
            ui.label(muted_text("Выберите картинку или гифку — она будет показываться на помпе вместо сцены."));
            return;
        }
        // Кадр готовим под тот же экран, что и служба: размер, выбранный вручную, иначе ответ помпы,
        // иначе 320×240; при повороте на 90°/270° — вертикальный. Показываем «как видно на помпе».
        let panel = rigcontrol_core::service::panel_size(&self.s, self.pump_detected_size());
        let (pw, ph) = lcdscene::scene_size(panel, self.s.cooler_rotation);
        let key = format!("{path}|{}|{pw}x{ph}", self.s.crop);
        if self.pump_preview.as_ref().map(|p| p.0 != key).unwrap_or(true) {
            let frames = cooler::load_image(std::path::Path::new(&path), pw, ph, 0, self.s.crop).unwrap_or_default();
            let ctx = ui.ctx().clone();
            let tex: Vec<(egui::TextureHandle, u64)> = frames
                .iter()
                .take(120)
                .enumerate()
                .map(|(n, f)| {
                    let c = egui::ColorImage::from_rgb([f.image.width() as usize, f.image.height() as usize], f.image.as_raw());
                    (ctx.load_texture(format!("pump_img_{n}"), c, egui::TextureOptions::LINEAR), u64::from(f.delay_ms.max(20)))
                })
                .collect();
            self.pump_preview = Some((key, tex, frames.len(), std::time::Instant::now()));
        }
        let Some((_, tex, count, t0)) = &self.pump_preview else { return };
        if tex.is_empty() {
            ui.label(egui::RichText::new("Не удалось открыть файл.").color(theme::BAD));
            return;
        }
        let total: u64 = tex.iter().map(|t| t.1).sum();
        let mut idx = 0;
        if tex.len() > 1 {
            let mut t = t0.elapsed().as_millis() as u64 % total.max(1);
            for (n, f) in tex.iter().enumerate() {
                if t < f.1 {
                    idx = n;
                    break;
                }
                t -= f.1;
            }
            ui.ctx().request_repaint_after(Duration::from_millis(30));
        }
        let [tw, th] = tex[idx].0.size();
        let aspect = th.max(1) as f32 / tw.max(1) as f32;
        let w = ui.available_width().min(640.0).min(480.0 / aspect);
        let size = egui::vec2(w, w * aspect);
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        egui::Image::from_texture(egui::load::SizedTexture::from_handle(&tex[idx].0)).rounding(10.0).paint_at(ui, rect);
        ui.add_space(6.0);
        ui.label(muted_text(if *count > 1 { format!("Гифка: {count} кадр.") } else { "Картинка".to_string() }));
    }

    /// Плитки тем с картинкой темы (как ThemeList в C#). Щелчок — раскладка и фон темы целиком.
    fn ui_pump_themes(&mut self, ui: &mut egui::Ui) {
        let themes = lcdscene::all_themes();
        let current = self.s.cooler_theme.clone().unwrap_or_else(|| lcdscene::default_theme().name.to_string());
        let gap = 8.0;
        let w = ((ui.available_width() - gap) / 2.0).floor();
        let mut picked = None;
        egui::Grid::new("pump_themes").spacing([gap, gap]).show(ui, |ui| {
            for (n, t) in themes.iter().enumerate() {
                let thumb = self.theme_thumb(ui.ctx(), t.image);
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 54.0), egui::Sense::click());
                let hover = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered(), 0.15);
                let p = ui.painter();
                let top = lcdscene::parse_color(t.top).unwrap_or((20, 20, 26));
                let bottom = lcdscene::parse_color(t.bottom).unwrap_or((5, 5, 8));
                let light_top = 0.299 * f64::from(top.0) + 0.587 * f64::from(top.1) + 0.114 * f64::from(top.2) > 150.0;
                let rect = rect.shrink(1.0 - hover);
                match &thumb {
                    Some(tex) => {
                        egui::Image::from_texture(egui::load::SizedTexture::from_handle(tex)).rounding(8.0).paint_at(ui, rect);
                        kit::gradient_rounded(p, rect, 8.0, egui::Color32::TRANSPARENT, egui::Color32::from_black_alpha(150));
                    }
                    None => kit::gradient_rounded(
                        p, rect, 8.0,
                        egui::Color32::from_rgb(top.0, top.1, top.2),
                        egui::Color32::from_rgb(bottom.0, bottom.1, bottom.2),
                    ),
                }
                let text_color = if thumb.is_none() && light_top { egui::Color32::from_rgb(0x2B, 0x2B, 0x2B) } else { egui::Color32::WHITE };
                p.text(rect.left_bottom() + egui::vec2(10.0, -8.0), egui::Align2::LEFT_BOTTOM, t.name, kit::semibold(13.0), text_color);
                let sel = t.name.eq_ignore_ascii_case(&current);
                let stroke = if sel {
                    egui::Stroke::new(2.0_f32, theme::TEXT)
                } else {
                    egui::Stroke::new(1.0_f32, kit::lerp_color(theme::LINE, egui::Color32::from_gray(0x60), hover))
                };
                p.rect_stroke(rect, 8.0, stroke);
                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    picked = Some(n);
                }
                if n % 2 == 1 {
                    ui.end_row();
                }
            }
        });
        if let Some(n) = picked {
            self.editor.apply_theme(&mut self.s, &themes[n]);
        }
        ui.add_space(4.0);
        kit::cap(ui, "Тема меняет раскладку и фон целиком; потом всё можно поправить руками.");
    }

    /// Миниатюра картинки темы (176×54) — один раз на тему.
    fn theme_thumb(&mut self, ctx: &egui::Context, image_name: Option<&'static str>) -> Option<egui::TextureHandle> {
        let name = image_name?;
        if let Some(t) = self.theme_thumbs.get(name) {
            return t.clone();
        }
        let tex = rigcontrol_core::pump_art::bytes(name)
            .and_then(|b| image::load_from_memory(b).ok())
            .map(|img| {
                let img = img.resize_to_fill(176, 54, image::imageops::FilterType::Triangle).to_rgba8();
                let c = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
                ctx.load_texture(format!("theme_{name}"), c, egui::TextureOptions::LINEAR)
            });
        self.theme_thumbs.insert(name, tex.clone());
        tex
    }

    /// «Фон»: имя фона, цвета градиента (верх/низ), своя картинка, яркость.
    fn ui_pump_background(&mut self, ui: &mut egui::Ui) {
        let bg = self.s.cooler_background.clone().filter(|p| !p.trim().is_empty());
        let name = match &bg {
            Some(p) if rigcontrol_core::pump_art::is_resource(p) => {
                format!("Тема «{}»", self.s.cooler_theme.clone().unwrap_or_default())
            }
            Some(p) => file_name(p),
            None => "Стандартный тёмный фон".to_string(),
        };
        ui.label(egui::RichText::new(name).color(theme::TEXT));
        ui.add_space(6.0);
        let theme_now = self.s.cooler_theme.as_deref().and_then(lcdscene::find_theme).unwrap_or_else(lcdscene::default_theme);
        let mut clear_image = false;
        ui.horizontal(|ui| {
            for (n, label) in [(0, "Верх"), (1, "Низ")] {
                let field = if n == 0 { &mut self.s.cooler_bg_top } else { &mut self.s.cooler_bg_bottom };
                let fallback = if n == 0 { theme_now.top } else { theme_now.bottom };
                let c = field.as_deref().and_then(lcdscene::parse_color)
                    .or_else(|| lcdscene::parse_color(fallback))
                    .unwrap_or((16, 18, 26));
                let mut rgb3 = [c.0, c.1, c.2];
                ui.label(muted_text(label));
                if rigcontrol_core::color_wheel::wheel_button(ui, ("pump_bg", n), &mut rgb3) {
                    *field = Some(hex(rgb3));
                    // Цвета градиента видны только без картинки: выбор цвета её убирает (как в C#).
                    clear_image = true;
                }
                ui.add_space(10.0);
            }
        });
        if clear_image {
            self.s.cooler_background = None;
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.button("Выбрать картинку…").clicked() {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("Картинки и гифки", &["png", "jpg", "jpeg", "gif", "bmp"])
                    .pick_file()
                {
                    self.s.cooler_background = Some(p.display().to_string());
                }
            }
            if ui.add_enabled(bg.is_some(), egui::Button::new("Сброс")).on_hover_text("Убрать картинку — останется градиент").clicked() {
                self.s.cooler_background = None;
            }
        });
        ui.add_space(8.0);
        ui.label(muted_text(format!("Яркость фона: {}%", self.s.cooler_brightness)));
        ui.add(egui::Slider::new(&mut self.s.cooler_brightness, 0..=100).show_value(false));
    }

    /// «Вывод на экран»: частота, поворот, размер экрана, диагностика.
    fn ui_pump_output(&mut self, ui: &mut egui::Ui, diag: &str, _time: f64) {
        self.s.cooler_fps = self.s.cooler_fps.clamp(1, 10);
        ui.label(muted_text(format!("Частота: до {} кадр/с", self.s.cooler_fps)));
        ui.add(egui::Slider::new(&mut self.s.cooler_fps, 1..=10).show_value(false));
        ui.add_space(8.0);
        ui.label(muted_text("Поворот"));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for r in [0, 90, 180, 270] {
                if pill(ui, &format!("{r}°"), self.s.cooler_rotation == r).clicked() {
                    self.s.cooler_rotation = r;
                }
            }
        });
        ui.add_space(8.0);
        ui.label(muted_text("Размер экрана"));
        const SIZES: [&str; 5] = ["Авто (по ответу помпы)", "320 × 320", "320 × 240", "240 × 320", "640 × 480"];
        let mut size = self.s.cooler_size.clamp(0, 4) as usize;
        egui::ComboBox::from_id_salt("pump_size").width(ui.available_width() - 8.0).selected_text(SIZES[size]).show_ui(ui, |ui| {
            for (n, t) in SIZES.iter().enumerate() {
                ui.selectable_value(&mut size, n, *t);
            }
        });
        self.s.cooler_size = size as i32;
        ui.add_space(10.0);
        ui.checkbox(&mut self.pump_diag, "Показать диагностику");
        if self.pump_diag {
            ui.add_space(4.0);
            let text = if diag.is_empty() { "Помпа не подключена.".to_string() } else { diag.to_string() };
            egui::Frame::none().fill(theme::SURFACE).rounding(6.0).inner_margin(egui::Margin::same(8.0)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add(egui::Label::new(egui::RichText::new(text).monospace().size(11.0).color(theme::MUTED)).wrap());
            });
        }
    }

    fn ui_mouse(&mut self, ui: &mut egui::Ui) {
        if let Some(m) = self.mouse.ui(ui) {
            self.msg = m;
        }
    }

    /// «Настройки» — порт PageSettings из MainWindow.xaml.
    fn ui_app(&mut self, ui: &mut egui::Ui) {
        let mut a = self.autostart_on;
        if switch_row(
            ui,
            &mut a,
            "Запускать вместе с Windows",
            "При входе в Windows запускается фоновая служба (значок в трее, без окна): сразу подсветка, экран помпы и вентиляторы.",
        ) {
            match autostart::set(a, "") {
                Ok(m) => self.msg = m,
                Err(e) => self.msg = e,
            }
            self.refresh_autostart();
        }
        if let Some(cmd) = &self.autostart_other {
            ui.label(egui::RichText::new(format!(
                "Сейчас вместе с Windows запускается другая программа: {cmd}. Включите переключатель, чтобы вместо неё запускалась эта."
            )).color(theme::BAD));
        }
        ui.add_space(12.0);
        if switch_row(
            ui,
            &mut self.s.close_to_tray,
            "Работать в фоне после закрытия окна",
            "Окно закрывается целиком и не занимает память, а подсветку, экран помпы и вентиляторы дальше ведёт лёгкая фоновая служба со значком в трее. Открыть окно — двойной щелчок по значку, выключить всё — «Выход» в его меню. Выключите — с окном остановится и всё остальное.",
        ) {
            self.save();
        }
        ui.label(muted_text(match (&self.service, self.s.close_to_tray) {
            (Some(_), _) => "Фоновая служба запущена и ждёт, пока окно открыто.",
            (None, true) => "Фоновая служба запустится, когда вы закроете окно.",
            (None, false) => "Фоновая служба не запущена.",
        }));
        ui.add_space(12.0);
        // auto_output читается при запуске окна: Service::start_with(.., pump = auto_output).
        if switch_row(ui, &mut self.s.auto_output, "Автовывод на помпу при запуске", "Сразу после старта выводить сцену на экран помпы.") {
            self.save();
        }
        ui.add_space(18.0);
        ui.separator();
        ui.add_space(10.0);
        ui.label(egui::RichText::new("Файлы").size(16.0).strong().color(theme::TEXT));
        ui.add_space(4.0);
        ui.label(egui::RichText::new(format!("Настройки: {}", settings::file_path().display())).color(theme::MUTED));
        if ui.button("Открыть папку настроек и журналов").clicked() {
            let _ = std::process::Command::new("explorer").arg(settings::app_dir()).spawn();
        }
        ui.add_space(18.0);
        ui.separator();
        ui.add_space(10.0);
        ui.label(egui::RichText::new("О программе").size(16.0).strong().color(theme::TEXT));
        ui.add_space(4.0);
        ui.label(egui::RichText::new(concat!("RigControl (Rust) ", env!("CARGO_PKG_VERSION"))).color(theme::TEXT));
        ui.label(egui::RichText::new(
            "Управление клавиатурой, мышью, помпой и ARGB без родных программ. Порт C#-версии на Rust (egui).",
        ).color(theme::MUTED));
    }

    /// «Вентиляторы» — PageFans (MainWindow.Fans.cs + FanCurveEditor.cs) в виде «жидкого стекла»:
    /// работающие каналы — плитки с крутящимся вентилятором (скорость по оборотам, свечение по нагрузке),
    /// неподключённые — отдельно, компактно. Щелчок по плитке — настройки канала.
    fn ui_fans(&mut self, ui: &mut egui::Ui) {
        let st = self.fans.state();
        let (time, dt) = ui.input(|i| (i.time, i.stable_dt.min(0.1)));
        let mut save = false;
        // Щелчок по плитке в этом кадре — окошко настроек не закрываем как «щелчок мимо».
        let mut clicked_tile = false;

        let spinning = |f: &Fan| f.rpm.map(|r| r > 50.0).unwrap_or(false);
        let active: Vec<Fan> = st.fans.iter().filter(|f| spinning(f)).cloned().collect();
        let idle: Vec<Fan> = st.fans.iter().filter(|f| !spinning(f)).cloned().collect();
        ui.ctx().request_repaint_after(if active.is_empty() { Duration::from_millis(500) } else { Duration::from_millis(33) });

        // ── Плата и температура CPU ──
        kit::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    kit::cap(ui, "МАТЕРИНСКАЯ ПЛАТА");
                    let board = st.board.clone().unwrap_or_else(|| "Материнская плата".to_string());
                    ui.label(egui::RichText::new(kit::ellipsize(&board, 70)).font(kit::semibold(17.0)).color(theme::TEXT));
                    let (line, bad) = if let Some(e) = &st.error {
                        (format!("Датчики платы недоступны: {}", kit::ellipsize(e, 80)), true)
                    } else if !st.read {
                        ("Ищу датчики платы…".to_string(), false)
                    } else if !st.admin {
                        ("Нет прав администратора: перезапустите RigControl от имени администратора.".to_string(), true)
                    } else if st.fans.is_empty() {
                        ("Каналов не найдено: нет драйвера PawnIO (pawnio.eu) или чип платы не поддерживается.".to_string(), true)
                    } else {
                        (format!(
                            "{} {} · {} не подключено",
                            active.len(),
                            kit::plural(active.len(), "работает", "работают", "работают"),
                            idle.len()
                        ), false)
                    };
                    ui.label(egui::RichText::new(line).size(12.5).color(if bad { theme::BAD } else { theme::MUTED }));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let temp = st.cpu_temp.map(|t| format!("{t:.0} °C")).unwrap_or_else(|| "—".into());
                    ui.vertical(|ui| {
                        kit::cap(ui, "CPU");
                        ui.label(egui::RichText::new(temp).font(kit::semibold(24.0)).color(theme::TEXT));
                    });
                });
            });
        });
        ui.add_space(16.0);

        // ── Режим для всех ──
        let modes = profiles::FAN_MODE_NAMES;
        kit::cap(ui, "Режим для всех");
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
            for (i, n) in modes.iter().enumerate() {
                if pill(ui, n, self.s.fan_mode_all == i as i32).clicked() && self.s.fan_mode_all != i as i32 {
                    self.s.fan_mode_all = i as i32;
                    save = true;
                }
            }
        });
        let all = self.s.fan_mode_all;
        if (1..=3).contains(&all) {
            ui.add_space(2.0);
            kit::cap(ui, format!("Кривая режима: {}", preset_text(all)));
        }
        ui.add_space(4.0);
        kit::cap(ui, "От 90 °C на CPU — все управляемые на 100 %. Меньше 20 % нельзя. При выходе — снова плате (BIOS). Помпу СЖО лучше оставить на «Авто» или «Максимум»; какой канал какой — поставьте «Максимум» и посмотрите, чьи обороты выросли.");
        ui.add_space(20.0);

        // ── Работают ──
        if !active.is_empty() {
            kit::h2(ui, &format!("Работают · {}", active.len()));
            ui.add_space(10.0);
            ui.scope(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(16.0, 16.0);
                ui.horizontal_wrapped(|ui| {
                    for fan in &active {
                        let mode = self.fan_mode_label(&fan.id);
                        let sel = self.fan_sel.as_deref() == Some(fan.id.as_str());
                        let (clicked, rect) = self.fan_tile(ui, fan, &mode, sel, dt);
                        if sel {
                            self.fan_sel_rect = Some(rect);
                        }
                        if clicked {
                            clicked_tile = true;
                            self.fan_sel = if sel { None } else { Some(fan.id.clone()) };
                            self.fan_sel_rect = Some(rect);
                        }
                    }
                });
            });
            ui.add_space(22.0);
        } else if st.read && st.error.is_none() && !st.fans.is_empty() {
            kit::cap(ui, "Сейчас ни один вентилятор не крутится.");
            ui.add_space(16.0);
        }
        // Забыть анимацию остановившихся: при новом старте вентилятор снова раскрутится с нуля.
        self.fan_anim.retain(|id, _| active.iter().any(|f| &f.id == id));

        // ── Не подключены ──
        if !idle.is_empty() {
            kit::h2(ui, &format!("Не подключены · {}", idle.len()));
            ui.add_space(4.0);
            kit::cap(ui, "Обороты 0: на разъёме ничего нет или вентилятор стоит. Щелчок — настройки канала.");
            ui.add_space(10.0);
            ui.scope(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(12.0, 12.0);
                ui.horizontal_wrapped(|ui| {
                    for fan in &idle {
                        let sel = self.fan_sel.as_deref() == Some(fan.id.as_str());
                        let (clicked, rect) = self.fan_idle_tile(ui, fan, sel);
                        if sel {
                            self.fan_sel_rect = Some(rect);
                        }
                        if clicked {
                            clicked_tile = true;
                            self.fan_sel = if sel { None } else { Some(fan.id.clone()) };
                            self.fan_sel_rect = Some(rect);
                        }
                    }
                });
            });
        }

        // ── Окошко настроек рядом с плиткой ──
        if self.fan_pop_for != self.fan_sel {
            self.fan_pop_for = self.fan_sel.clone();
            self.fan_pop_at = std::time::Instant::now();
        }
        let selected = self.fan_sel.clone().and_then(|id| st.fans.iter().find(|f| f.id == id).cloned());
        if let (Some(fan), Some(anchor)) = (selected, self.fan_sel_rect) {
            let ctx = ui.ctx().clone();
            let screen = ctx.screen_rect();
            let (w, gap) = (420.0, 12.0);
            let x = if anchor.right() + gap + w <= screen.right() - 8.0 {
                anchor.right() + gap
            } else if anchor.left() - gap - w >= screen.left() + 8.0 {
                anchor.left() - gap - w
            } else {
                (anchor.center().x - w / 2.0).clamp(screen.left() + 8.0, (screen.right() - 8.0 - w).max(screen.left() + 8.0))
            };
            let t = (self.fan_pop_at.elapsed().as_secs_f32() / 0.15).min(1.0);
            let k = 1.0 - (1.0 - t).powi(3);
            if t < 1.0 {
                ctx.request_repaint();
            }
            let area = egui::Area::new(egui::Id::new("fan_popup"))
                .order(egui::Order::Foreground)
                .fixed_pos(egui::pos2(x, anchor.top() + (1.0 - k) * 6.0))
                .constrain(true)
                .show(&ctx, |ui| {
                    ui.set_opacity(k);
                    popup_frame().show(ui, |ui| {
                        ui.set_width(w - 40.0);
                        self.fan_settings(ui, &fan, &st, time, &mut save);
                    });
                });
            let popup = area.response.rect;
            let (clicked, pos, esc) = ctx.input(|i| (i.pointer.any_click(), i.pointer.interact_pos(), i.key_pressed(egui::Key::Escape)));
            let outside = pos.map(|p| !popup.contains(p)).unwrap_or(false);
            if esc || (clicked && !clicked_tile && outside) {
                self.fan_sel = None;
            }
        }
        if save {
            self.save();
        }
    }

    /// Подпись режима на плитке: свой режим канала или общий.
    fn fan_mode_label(&self, id: &str) -> String {
        let own = self.s.fan_modes.as_ref().and_then(|m| m.get(id)).copied();
        let m = own.unwrap_or(self.s.fan_mode_all);
        let name = profiles::FAN_MODE_NAMES.get(m.max(0) as usize).copied().unwrap_or("?");
        if own.is_some() { name.to_string() } else { format!("{name} · как у всех") }
    }

    /// Плитка работающего вентилятора (в стиле плиток главной, 250×300): картинка вентилятора,
    /// крыльчатка которой крутится со скоростью по оборотам (с инерцией), имя, обороты, режим.
    fn fan_tile(&mut self, ui: &mut egui::Ui, fan: &Fan, mode: &str, selected: bool, dt: f32) -> (bool, egui::Rect) {
        use std::f32::consts::TAU;
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(250.0, 300.0), egui::Sense::click());
        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
        let hover = ui.ctx().animate_bool(resp.id, resp.hovered());
        let seed = fan.id.bytes().fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(u32::from(b)));
        let anim = self.fan_anim.entry(fan.id.clone()).or_insert_with(|| FanAnim {
            angle: (seed % 628) as f32 / 100.0,
            omega: 0.0,
        });
        let rpm = fan.rpm.unwrap_or(0.0) as f32;
        // ~2250 об/мин → ~1,6 оборота в секунду на экране; раскрутка и остановка плавные.
        let target = rpm * 0.0045;
        anim.omega += (target - anim.omega) * (1.0 - (-dt * 1.6).exp());
        anim.angle = (anim.angle + anim.omega * dt) % TAU;
        let angle = anim.angle;

        let r = rect.expand(2.5 * hover);
        let p = ui.painter();
        let stroke = if selected {
            egui::Stroke::new(1.5_f32, theme::TEXT)
        } else {
            egui::Stroke::new(1.0_f32, if hover > 0.5 { kit::LINE_HOVER } else { theme::LINE })
        };
        p.rect(r, 12.0, theme::CARD, stroke);
        let c = egui::Rect::from_min_max(egui::pos2(r.left() + 20.0, r.top() + 16.0), egui::pos2(r.right() - 20.0, r.bottom() - 18.0));
        p.text(c.left_top(), egui::Align2::LEFT_TOP, kit::ellipsize(&fan.name, 26).to_uppercase(), kit::semibold(10.5), theme::MUTED);
        let pct = fan.percent.map(|x| format!("{x:.0} %")).unwrap_or_default();
        p.text(c.right_top(), egui::Align2::RIGHT_TOP, pct, kit::semibold(10.5), theme::MUTED);

        let status_mid = c.bottom() - 10.0;
        let detail_top = c.bottom() - 20.0 - 8.0 - 17.0;
        let name_top = detail_top - 3.0 - 26.0;
        let art = egui::Rect::from_min_max(egui::pos2(c.left(), c.top() + 20.0), egui::pos2(c.right(), name_top - 10.0));
        self.led_art.fan_plain(ui, art, Some(angle), egui::Color32::WHITE);

        let p = ui.painter();
        let rpm_shown = ui.ctx().animate_value_with_time(resp.id.with("rpm"), rpm, 0.6);
        let num = p.text(egui::pos2(c.left(), name_top), egui::Align2::LEFT_TOP, format!("{rpm_shown:.0}"), kit::semibold(19.0), theme::TEXT);
        p.text(egui::pos2(num.right() + 6.0, num.bottom() - 3.0), egui::Align2::LEFT_BOTTOM, "об/мин", egui::FontId::proportional(12.5), theme::MUTED);
        p.text(egui::pos2(c.left(), detail_top), egui::Align2::LEFT_TOP, kit::ellipsize(mode, 34), egui::FontId::proportional(12.5), theme::MUTED);
        let (who, good) = if fan.controlled { ("Управляет программа", true) } else { ("Управляет плата", false) };
        p.circle_filled(egui::pos2(c.left() + 4.5, status_mid), 4.5, if good { kit::GOOD } else { theme::MUTED });
        p.text(egui::pos2(c.left() + 17.0, status_mid), egui::Align2::LEFT_CENTER, who, egui::FontId::proportional(14.0), theme::TEXT);
        (resp.clicked(), rect)
    }

    /// Плитка неподключённого канала (180×196): вентилятор приглушён и не крутится.
    fn fan_idle_tile(&mut self, ui: &mut egui::Ui, fan: &Fan, selected: bool) -> (bool, egui::Rect) {
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(180.0, 196.0), egui::Sense::click());
        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
        let hover = ui.ctx().animate_bool(resp.id, resp.hovered());
        let p = ui.painter();
        let stroke = if selected {
            egui::Stroke::new(1.5_f32, theme::TEXT)
        } else {
            egui::Stroke::new(1.0_f32, if hover > 0.5 { kit::LINE_HOVER } else { theme::LINE })
        };
        p.rect(rect, 12.0, theme::CARD, stroke);
        let c = rect.shrink2(egui::vec2(16.0, 14.0));
        let art = egui::Rect::from_min_max(c.left_top(), egui::pos2(c.right(), c.bottom() - 44.0));
        let tint = egui::Color32::from_gray(if hover > 0.5 { 120 } else { 80 });
        self.led_art.fan_plain(ui, art, Some(0.35), tint);
        let p = ui.painter();
        p.text(egui::pos2(c.left(), c.bottom() - 38.0), egui::Align2::LEFT_TOP, kit::ellipsize(&fan.name, 18), kit::semibold(14.0), theme::TEXT);
        let sub = if fan.controlled { "Стоит · управляет программа" } else { "Не подключён" };
        p.text(egui::pos2(c.left(), c.bottom() - 16.0), egui::Align2::LEFT_TOP, sub, egui::FontId::proportional(12.0), theme::MUTED);
        (resp.clicked(), rect)
    }

    /// Настройки выбранного канала: режим, своя скорость или кривая.
    fn fan_settings(&mut self, ui: &mut egui::Ui, fan: &Fan, st: &FanState, _time: f64, save: &mut bool) {
        use fan_curve::{FAN_MIN_PERCENT, FAN_MODE_CURVE, FAN_MODE_FIXED};
        let modes = profiles::FAN_MODE_NAMES;
        let id = fan.id.clone();
        let all = self.s.fan_mode_all;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&fan.name).font(kit::semibold(17.0)).color(theme::TEXT));
            let rpm = match fan.rpm {
                Some(r) if r > 50.0 => format!("{r:.0} об/мин"),
                _ => "не крутится".to_string(),
            };
            ui.label(muted_text(rpm));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Закрыть").on_hover_text("Esc или щелчок мимо окошка").clicked() {
                    self.fan_sel = None;
                }
            });
        });
        ui.add_space(8.0);
        let own = self.s.fan_modes.as_ref().and_then(|m| m.get(&id)).copied();
        let mut sel = own.unwrap_or(-1);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
            let as_all = format!("Как для всех ({})", modes.get(all.max(0) as usize).copied().unwrap_or("?"));
            if pill(ui, &as_all, sel < 0).clicked() {
                sel = -1;
            }
            for (i, n) in modes.iter().enumerate() {
                if pill(ui, n, sel == i as i32).clicked() {
                    sel = i as i32;
                }
            }
        });
        if sel != own.unwrap_or(-1) {
            let m = self.s.fan_modes.get_or_insert_with(Default::default);
            if sel < 0 { m.remove(&id); } else { m.insert(id.clone(), sel); }
            if m.is_empty() { self.s.fan_modes = None; }
            *save = true;
        }
        ui.add_space(10.0);
        let effective = if sel < 0 { all } else { sel };
        if effective == FAN_MODE_FIXED {
            let mut pct = self.s.fan_percent.as_ref().and_then(|m| m.get(&id)).copied().unwrap_or(60);
            ui.label(muted_text(format!("Скорость: {pct} %")));
            let r = ui.add(egui::Slider::new(&mut pct, FAN_MIN_PERCENT..=100).show_value(false));
            if r.changed() {
                self.s.fan_percent.get_or_insert_with(Default::default).insert(id.clone(), pct);
            }
            if r.drag_stopped() || (r.changed() && !r.dragged()) {
                *save = true;
            }
        } else if effective == FAN_MODE_CURVE {
            let mut pts = fan_curve::from_flat(self.s.fan_curves.as_ref().and_then(|m| m.get(&id)));
            let before = pts.clone();
            let released = fan_curve::editor(ui, &format!("fan_curve_{id}"), &mut pts, st.cpu_temp, FAN_MIN_PERCENT as f64);
            if pts != before {
                self.s.fan_curves.get_or_insert_with(Default::default).insert(id.clone(), fan_curve::to_flat(&pts));
            }
            if released {
                *save = true;
            }
            kit::cap(ui, "Тяните точки мышью: по горизонтали — температура CPU, по вертикали — скорость. Ниже 20 % вентилятор не опустится.");
            ui.horizontal(|ui| {
                if ui.button("Сбросить кривую").clicked() {
                    self.s.fan_curves.get_or_insert_with(Default::default)
                        .insert(id.clone(), fan_curve::to_flat(&fan_curve::DEFAULT_CURVE));
                    *save = true;
                }
                if ui.button("Скопировать в другие")
                    .on_hover_text("Такая же кривая будет у остальных вентиляторов (включится, когда выберете у них «Своя кривая»).")
                    .clicked()
                {
                    let flat = fan_curve::to_flat(&pts);
                    let curves = self.s.fan_curves.get_or_insert_with(Default::default);
                    for other in &st.fans {
                        if other.id != id {
                            curves.insert(other.id.clone(), flat.clone());
                        }
                    }
                    *save = true;
                }
            });
        } else if (1..=3).contains(&effective) {
            kit::cap(ui, format!("Кривая: {}", preset_text(effective)));
        } else if effective == 0 {
            kit::cap(ui, "Скоростью управляет плата (BIOS).");
        } else {
            kit::cap(ui, "Всегда 100 %.");
        }
    }
}

/// Анимация плитки вентилятора: угол и скорость вращения (рад/с).
struct FanAnim {
    angle: f32,
    omega: f32,
}

/// Кривая готового режима текстом: «35 °C → 30 %, …».
fn preset_text(m: i32) -> String {
    fan_curve::PRESETS
        .get(m.max(0) as usize)
        .map(|pts| pts.iter().map(|(t, p)| format!("{t:.0} °C → {p:.0} %")).collect::<Vec<_>>().join(", "))
        .unwrap_or_default()
}

/// Кнопка-переключатель как вкладки страницы клавиатуры: выбранная — светлая, остальные — тёмные.
fn pill(ui: &mut egui::Ui, text: &str, selected: bool) -> egui::Response {
    let fg = if selected { egui::Color32::from_rgb(0x10, 0x10, 0x10) } else { theme::TEXT };
    let b = egui::Button::new(egui::RichText::new(text).font(kit::semibold(12.5)).color(fg))
        .fill(if selected { theme::TEXT } else { theme::SURFACE2 })
        .stroke(egui::Stroke::new(1.0_f32, if selected { theme::TEXT } else { theme::LINE }))
        .rounding(8.0)
        .min_size(egui::vec2(0.0, 30.0));
    ui.add(b).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Подсказка под выбранным режимом (ArgbModeHint в C#).
fn argb_mode_hint(v: i32) -> &'static str {
    match v {
        7 => "Голубой — холодно (до 40 °C), зелёный — 60 °C, жёлтый — 75 °C, красный — от 90 °C.",
        8 => "Каждый вентилятор повторяет свою часть клавиатуры (слева направо): эффекты, волна от центра и раскраска переходят на вентиляторы.",
        5 => "Обычная плавная волна. Включите «Радуга» для спектра или выключите её и задайте до трёх цветов — они плавно смешаются без тёмных провалов.",
        _ => "",
    }
}

/// Короткое описание режима подсветки для карточки.
fn argb_hint(v: i32) -> &'static str {
    match v {
        0 => "управляет плата",
        1 => "всё погасить",
        2 => "один цвет",
        3 => "плавно гаснет и загорается",
        4 => "вся лента меняет цвет",
        5 => "радуга бежит по кругу",
        6 => "огонёк с хвостом",
        7 => "голубой → красный по нагреву",
        8 => "продолжение клавиатуры",
        _ => "",
    }
}


/// Всплывающее окошко: тёмное матовое стекло с тенью (поверх содержимого текст должен читаться).
fn popup_frame() -> egui::Frame {
    egui::Frame::none()
        .fill(egui::Color32::from_rgba_unmultiplied(20, 20, 24, 242))
        .stroke(egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(40)))
        .rounding(18.0)
        .inner_margin(egui::Margin::same(20.0))
        .shadow(egui::epaint::Shadow {
            offset: egui::vec2(0.0, 10.0),
            blur: 30.0,
            spread: 0.0,
            color: egui::Color32::from_black_alpha(170),
        })
}

/// PaintPalette из MainWindow.KeyPaint.cs: те же 14 цветов у подсветки и у кисти.
const PALETTE: [[u8; 3]; 14] = [
    [0xFF, 0x00, 0x00], [0xFF, 0x00, 0x50], [0xFF, 0x2D, 0x9A], [0xFF, 0x00, 0xFF], [0xC0, 0x00, 0xFF], [0x80, 0x00, 0xFF], [0x00, 0x00, 0xFF],
    [0x00, 0xB4, 0xFF], [0x00, 0xFF, 0xD0], [0x00, 0xFF, 0x40], [0xB4, 0xFF, 0x00], [0xFF, 0xC8, 0x00], [0xFF, 0x64, 0x00], [0xFF, 0xFF, 0xFF],
];

/// Палитра (26×26, скругление 6): щелчок — этот цвет; выбранный — с белой рамкой.
fn swatches(ui: &mut egui::Ui, color: &mut [u8; 3]) -> bool {
    let mut changed = false;
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
        ui.horizontal_wrapped(|ui| {
            for c in PALETTE {
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::click());
                let sel = *color == c;
                let stroke = if sel { egui::Stroke::new(2.0_f32, theme::TEXT) } else { egui::Stroke::new(2.0_f32, theme::LINE) };
                ui.painter().rect(rect, 6.0, egui::Color32::from_rgb(c[0], c[1], c[2]), stroke);
                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(hex(c)).clicked() {
                    *color = c;
                    changed = true;
                }
            }
        });
    });
    changed
}

/// Образец 38×34 и поле #RRGGBB: ввод меняет цвет, а цвет с круга — текст (если поле не в фокусе).
fn hex_row(ui: &mut egui::Ui, id: &str, text: &mut String, color: &mut [u8; 3]) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(38.0, 34.0), egui::Sense::hover());
        ui.painter().rect(rect, 6.0, egui::Color32::from_rgb(color[0], color[1], color[2]), egui::Stroke::new(1.0_f32, theme::LINE));
        let r = ui.add(egui::TextEdit::singleline(text).id_salt(id).desired_width(110.0).char_limit(7));
        if r.changed() {
            let t = text.trim().trim_start_matches('#');
            if t.len() == 3 || t.len() == 6 {
                let c = rgb(text.trim());
                if c != *color {
                    *color = c;
                    changed = true;
                }
            }
        }
        if !r.has_focus() && rgb(text.trim()) != *color {
            *text = hex(*color);
        }
    });
    changed
}

fn muted_text(t: impl Into<String>) -> egui::RichText {
    egui::RichText::new(t).color(theme::MUTED)
}

/// Строка настройки как в PageSettings: заголовок, подсказка и переключатель справа.
fn switch_row(ui: &mut egui::Ui, value: &mut bool, title: &str, hint: &str) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(title).size(14.0).color(theme::TEXT));
            ui.label(egui::RichText::new(hint).size(12.0).color(theme::MUTED));
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            changed = ui.checkbox(&mut *value, "").changed();
        });
    });
    changed
}

impl App {
    /// Батарея, память и меню трея — раз за кадр, дёшево.
    fn tick_extras(&mut self, ctx: &egui::Context) {
        let (min, focused) = ctx.input(|i| (i.viewport().minimized.unwrap_or(false), i.viewport().focused.unwrap_or(true)));
        let now = std::time::Instant::now();
        if !min && now.duration_since(self.ram_at) >= Duration::from_secs(2) {
            self.ram_label = memory::label();
            self.ram_at = now;
        }
        // Свернули: через 3 с отдаём память (в потоке: свёрнутое окно может не перерисовываться).
        if min && !self.was_minimized {
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_secs(3));
                memory::trim();
            });
            self.trim_at = Some(now + Duration::from_secs(300));
        }
        self.was_minimized = min;
        // Не в фокусе: через минуту, дальше раз в 5 минут.
        if focused && !min {
            self.trim_at = None;
        } else if let Some(at) = self.trim_at {
            if now >= at {
                memory::trim();
                self.trim_at = Some(now + Duration::from_secs(300));
            }
        } else {
            self.trim_at = Some(now + Duration::from_secs(60));
        }

        if let Some(p) = self.battery.take_warning() {
            self.msg = format!("Клавиатура садится: заряд {p} % — пора подключить кабель.");
        }

        // «Подобрать» и сообщения потока клавиатуры («Часы выставлены…», ошибки).
        self.tick_demo(ctx);
        if let Some((at, text)) = self.kb.state().event {
            if self.kb_event_at != Some(at) {
                self.kb_event_at = Some(at);
                self.msg = text;
            }
        }
    }

    /// Боковое меню (MainWindow.xaml, колонка 236 px): шапка с логотипом, разделы,
    /// внизу — карточка состояния устройств и «ОЗУ: N МБ». Возвращает выбранную страницу.
    fn ui_sidebar(&mut self, ui: &mut egui::Ui) -> Option<Tab> {
        let mut go = None;
        // Шапка
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            if self.logo.is_none() {
                self.logo = logo_rgba(88).map(|rgba| {
                    let img = egui::ColorImage::from_rgba_unmultiplied([88, 88], &rgba);
                    ui.ctx().load_texture("logo", img, egui::TextureOptions::NEAREST)
                });
            }
            if let Some(tex) = &self.logo {
                ui.add(egui::Image::new(tex).fit_to_exact_size(egui::vec2(44.0, 44.0)).rounding(10.0));
            }
            ui.add_space(6.0);
            ui.vertical(|ui| {
                ui.add_space(4.0);
                ui.label(egui::RichText::new("Rig Control").font(kit::semibold(18.0)).color(theme::TEXT));
                // Коротко, чтобы влезало в одну строку рядом с логотипом.
                ui.add(egui::Label::new(egui::RichText::new(concat!("Rust · ", env!("CARGO_PKG_VERSION"))).size(11.0).color(theme::MUTED)).truncate());
            });
        });
        ui.add_space(18.0);

        // Разделы
        const BOTTOM: f32 = 176.0;
        let nav_h = (ui.available_height() - BOTTOM).max(120.0);
        egui::ScrollArea::vertical().id_salt("nav").max_height(nav_h).auto_shrink([false, true]).show(ui, |ui| {
            for (items, caption) in [(NAV_TOP, None), (NAV_DEVICES, Some("УСТРОЙСТВА")), (NAV_SYSTEM, Some("СИСТЕМА"))] {
                if let Some(c) = caption {
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.add_space(6.0);
                        kit::nav_caption(ui, c);
                    });
                    ui.add_space(4.0);
                }
                for &(t, g, n) in items {
                    if nav_item(ui, self.tab == t, g, n).clicked() && self.tab != t {
                        go = Some(t);
                    }
                    ui.add_space(2.0);
                }
            }
        });
        let rest = ui.available_height() - BOTTOM;
        if rest > 0.0 {
            ui.add_space(rest);
        }

        // Состояние устройств: одна компактная карточка
        let low = rigcontrol_core::battery::LOW_PERCENT;
        let tiles = self.tiles();
        let find = |tab: Tab| tiles.iter().find(|t| t.tab == tab);
        kit::card().inner_margin(egui::Margin { left: 12.0, right: 12.0, top: 10.0, bottom: 2.0 }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            for (tab, title) in [(Tab::Pump, "Помпа"), (Tab::Keyboard, "Клавиатура"), (Tab::Mouse, "Мышь")] {
                let Some(t) = find(tab) else { continue };
                let (text, battery) = match &t.status {
                    TileStatus::Text(s, _) => (s.clone(), None),
                    TileStatus::Link { battery, link, .. } => {
                        let how = match *link {
                            Some("USB") => "по кабелю",
                            Some(_) => "по радио",
                            None => "на связи",
                        };
                        let text = if tab == Tab::Mouse { format!("{} · {how}", t.name) } else { format!("На связи {how}") };
                        (text, *battery)
                    }
                };
                if status_row(ui, title, &text, t.online, battery, low).clicked() {
                    go = Some(tab);
                }
                ui.add_space(8.0);
            }
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.label(egui::RichText::new(self.ram_label.clone()).size(11.5).color(theme::MUTED)).on_hover_text(
                "Сколько оперативной памяти занимает программа (как «Память» в диспетчере задач). Когда окно свёрнуто или в трее, лишнее отдаётся системе.",
            );
        });
        go
    }

    /// Всплывающее сообщение внизу справа: 5 секунд, потом исчезает.
    fn ui_toast(&mut self, ctx: &egui::Context) {
        if !self.msg.is_empty() {
            self.toast = std::mem::take(&mut self.msg);
            self.toast_at = Some(std::time::Instant::now());
        }
        let Some(at) = self.toast_at else { return };
        let shown = Duration::from_secs(5);
        if at.elapsed() >= shown || self.toast.is_empty() {
            self.toast_at = None;
            return;
        }
        egui::Area::new(egui::Id::new("toast"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-24.0, -24.0))
            .show(ctx, |ui| {
                kit::card().fill(theme::SURFACE2).inner_margin(egui::Margin::symmetric(16.0, 12.0)).show(ui, |ui| {
                    ui.set_max_width(420.0);
                    ui.add(egui::Label::new(egui::RichText::new(self.toast.clone()).color(theme::TEXT)).wrap());
                });
            });
        ctx.request_repaint_after(shown.saturating_sub(at.elapsed()));
    }
}

/// «Авто (BIOS)», «Своя кривая» или «свои режимы», если у каналов разные (FanSummary в C#).
fn fan_mode_summary(s: &Settings) -> String {
    let mut values: Vec<i32> = match &s.fan_modes {
        Some(m) if !m.is_empty() => m.values().copied().collect(),
        _ => vec![s.fan_mode_all],
    };
    values.sort_unstable();
    values.dedup();
    if values.len() == 1 {
        let i = values[0].clamp(0, profiles::FAN_MODE_NAMES.len() as i32 - 1) as usize;
        profiles::FAN_MODE_NAMES[i].to_string()
    } else {
        "свои режимы".to_string()
    }
}

/// Шрифт для значка: MDL2 — шрифтом значков (символы из личной области Юникода), остальное — обычным.
fn glyph_font(text: &str, size: f32) -> egui::FontId {
    let private = text.chars().next().is_some_and(|c| ('\u{E000}'..='\u{F8FF}').contains(&c));
    if private { kit::icon_font(size) } else { egui::FontId::proportional(size) }
}

/// Пункт меню (NavButton): значок 16 и подпись 14; выбранный — «стеклянная» капсула.
fn nav_item(ui: &mut egui::Ui, selected: bool, g: char, text: &str) -> egui::Response {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 38.0), egui::Sense::click());
    let painter = ui.painter();
    if selected {
        painter.rect(rect, 10.0, kit::GLASS, egui::Stroke::new(1.0_f32, kit::GLASS_BORDER));
    } else if resp.hovered() {
        painter.rect_filled(rect, 10.0, theme::HOVER);
    }
    let color = if selected || resp.hovered() { theme::TEXT } else { theme::MUTED };
    let icon = kit::icon(g);
    painter.text(rect.left_center() + egui::vec2(12.0, 0.0), egui::Align2::LEFT_CENTER, &icon, glyph_font(&icon, 16.0), color);
    painter.text(rect.left_center() + egui::vec2(38.0, 0.0), egui::Align2::LEFT_CENTER, text, kit::semibold(14.0), color);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Строка карточки состояния в боковой панели: точка, имя, состояние и (если есть) заряд.
fn status_row(ui: &mut egui::Ui, title: &str, text: &str, ok: bool, battery: Option<u8>, low: u8) -> egui::Response {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 34.0), egui::Sense::click());
    let painter = ui.painter();
    kit::status_dot(painter, egui::pos2(rect.left() + 4.0, rect.top() + 9.0), 4.0, ok);
    painter.text(egui::pos2(rect.left() + 17.0, rect.top()), egui::Align2::LEFT_TOP, title, kit::semibold(13.0), theme::TEXT);
    let max = if battery.is_some() { 18 } else { 27 };
    painter.text(
        egui::pos2(rect.left() + 17.0, rect.top() + 18.0),
        egui::Align2::LEFT_TOP,
        kit::ellipsize(text, max),
        egui::FontId::proportional(11.5),
        theme::MUTED,
    );
    if let Some(p) = battery {
        let pct = painter.text(rect.right_top() + egui::vec2(0.0, 1.0), egui::Align2::RIGHT_TOP, format!("{p}%"), kit::semibold(11.5), theme::TEXT);
        kit::battery_horizontal(painter, egui::pos2(pct.left() - 33.0, rect.top() + 3.0), p, low);
    }
    let tip = match title {
        "Помпа" => "Открыть настройки помпы",
        "Клавиатура" => "Открыть настройки клавиатуры",
        _ => "Открыть настройки мыши",
    };
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tip)
}

/// Плитка устройства на главной (DeviceTile, 272×306): подпись раздела, картинка, имя, что настроено, состояние.
fn device_tile(ui: &mut egui::Ui, tex: Option<&egui::TextureHandle>, t: &Tile) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(272.0, 306.0), egui::Sense::click());
    let hover = ui.ctx().animate_bool(resp.id, resp.hovered());
    let r = rect.expand(2.5 * hover);
    let painter = ui.painter();
    let stroke = if hover > 0.5 { kit::LINE_HOVER } else { theme::LINE };
    painter.rect(r, 12.0, theme::CARD, egui::Stroke::new(1.0_f32, stroke));
    let c = egui::Rect::from_min_max(egui::pos2(r.left() + 20.0, r.top() + 16.0), egui::pos2(r.right() - 20.0, r.bottom() - 18.0));

    painter.text(c.left_top(), egui::Align2::LEFT_TOP, t.title.to_uppercase(), kit::semibold(10.5), theme::MUTED);
    let chevron = kit::icon(glyph::CHEVRON);
    painter.text(c.right_top() + egui::vec2(0.0, 1.0), egui::Align2::RIGHT_TOP, &chevron, glyph_font(&chevron, 12.0), theme::MUTED);

    // Снизу вверх: состояние, что настроено, имя, над ними — картинка.
    let status_mid = c.bottom() - 10.0;
    let detail_top = c.bottom() - 20.0 - 8.0 - 17.0;
    let name_top = detail_top - 3.0 - 26.0;
    let art = egui::Rect::from_min_max(egui::pos2(c.left(), c.top() + 18.0), egui::pos2(c.right(), name_top - 8.0));
    match tex {
        Some(tex) => {
            let size = tex.size_vec2();
            let k = (art.width() / size.x).min(art.height().min(164.0) / size.y).min(1.0);
            let img = egui::Rect::from_center_size(art.center(), size * k);
            painter.image(tex.id(), img, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
        }
        None => {
            let radius = (art.height().min(140.0) / 2.0).min(70.0);
            painter.circle_filled(art.center(), radius, kit::ACCENT_DIM);
            let g = kit::icon(t.glyph);
            painter.text(art.center(), egui::Align2::CENTER_CENTER, &g, glyph_font(&g, radius * 0.9), theme::TEXT);
        }
    }
    painter.text(egui::pos2(c.left(), name_top), egui::Align2::LEFT_TOP, kit::ellipsize(&t.name, 20), kit::semibold(19.0), theme::TEXT);
    painter.text(egui::pos2(c.left(), detail_top), egui::Align2::LEFT_TOP, kit::ellipsize(&t.detail, 36), egui::FontId::proportional(12.5), theme::MUTED);

    match &t.status {
        TileStatus::Text(text, ok) => {
            kit::status_dot(painter, egui::pos2(c.left() + 4.5, status_mid), 4.5, *ok);
            painter.text(egui::pos2(c.left() + 17.0, status_mid), egui::Align2::LEFT_CENTER, kit::ellipsize(text, 26), egui::FontId::proportional(14.0), theme::TEXT);
        }
        TileStatus::Link { battery, charging, link } => {
            let mut x = c.left();
            if let Some(p) = battery {
                let shell = egui::Rect::from_min_size(egui::pos2(x, status_mid - 10.0), egui::vec2(12.0, 20.0));
                kit::battery_vertical(painter, shell, *p, rigcontrol_core::battery::LOW_PERCENT);
                x = painter.text(egui::pos2(x + 19.0, status_mid), egui::Align2::LEFT_CENTER, format!("{p}%"), egui::FontId::proportional(14.0), theme::TEXT).right();
                if *charging {
                    let g = kit::icon(glyph::CHARGING);
                    x = painter.text(egui::pos2(x + 4.0, status_mid), egui::Align2::LEFT_CENTER, &g, glyph_font(&g, 15.0), theme::TEXT).right();
                }
                x += 18.0;
            }
            if let Some(l) = link {
                let g = kit::icon(if *l == "USB" { glyph::USB } else { glyph::WIRELESS });
                x = painter.text(egui::pos2(x, status_mid), egui::Align2::LEFT_CENTER, &g, glyph_font(&g, 15.0), theme::TEXT).right();
                painter.text(egui::pos2(x + 7.0, status_mid), egui::Align2::LEFT_CENTER, *l, egui::FontId::proportional(14.0), theme::TEXT);
            }
        }
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Открыть настройки")
}

/// Окно закрывают: отдать устройства и, если включена работа в фоне, передать их фоновой службе.
impl Drop for App {
    fn drop(&mut self) {
        self.sync_colors();
        let _ = self.s.save();
        self.stop_service();
        self.kb.shutdown();
        self.fans.shutdown();
        // Поток вентиляторов отдаёт каналы плате в течение секунды, потом закрываем помощника датчиков.
        std::thread::sleep(Duration::from_millis(1300));
        rigcontrol_core::lhm::shutdown();
        let background = self.s.close_to_tray;
        match self.service.take() {
            // Служба была на паузе: в фоне — отпускаем (закрытие соединения), иначе — просим выйти.
            Some(p) if !background => p.quit(),
            Some(p) => drop(p),
            None if background => {
                let _ = service_link::spawn();
            }
            // Служба запустилась уже при открытом окне (паузу у неё не брали) и ждёт его закрытия:
            // без работы в фоне она тоже должна выйти.
            None => {
                let _ = service_link::quit();
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_restart(ctx);
        self.auto_apply(ctx);
        self.tick_extras(ctx);

        // Боковое меню как в C# (MainWindow.xaml: колонка 236 px, тёмный фон #050505, рамка справа).
        let mut go = None;
        egui::SidePanel::left("nav")
            .exact_width(236.0)
            .resizable(false)
            .frame(
                egui::Frame::none()
                    .fill(theme::SIDEBAR)
                    .stroke(egui::Stroke::new(1.0_f32, theme::LINE))
                    .inner_margin(egui::Margin { left: 16.0, right: 16.0, top: 22.0, bottom: 14.0 }),
            )
            .show(ctx, |ui| {
                go = self.ui_sidebar(ui);
            });

        let minimized = ctx.input(|i| i.viewport().minimized.unwrap_or(false));
        self.sys.set_visible(self.tab == Tab::System && !minimized);
        // Датчики платы нужны и для плитки «Вентиляторы» на главной.
        self.fans.set_page_visible(matches!(self.tab, Tab::Fans | Tab::Devices) && !minimized);
        // Мышь на главной и в боковой панели: найти и следить за зарядом, как DispatcherTimer в C#.
        if !minimized {
            self.mouse.keep_fresh(ctx);
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(theme::WINDOW).inner_margin(egui::Margin { left: 28.0, right: 28.0, top: 22.0, bottom: 20.0 }))
            .show(ctx, |ui| {
                kit::h1(ui, self.tab.title());
                ui.add_space(4.0);
                egui::ScrollArea::vertical().id_salt(("page", self.tab.title())).auto_shrink([false, false]).show(ui, |ui| {
                    if self.tab.own_cards() {
                        // Главная, клавиатура и мышь рисуют свои карточки, без общей.
                        ui.add_space(8.0);
                        match self.tab {
                            Tab::Devices => {
                                if let Some(t) = self.ui_devices(ui) {
                                    go = Some(t);
                                }
                            }
                            Tab::Mouse => self.ui_mouse(ui),
                            Tab::Pump => self.ui_pump(ui),
                            Tab::Fans => self.ui_fans(ui),
                            Tab::Argb => self.ui_argb(ui),
                            Tab::System => {
                                if let Some(m) = self.sys.ui(ui) {
                                    self.msg = m;
                                }
                            }
                            _ => self.ui_keyboard(ui),
                        }
                        return;
                    }
                    ui.add_space(12.0);
                    // Содержимое остальных страниц — в карточке, как CardBrush/CornerRadius в C#.
                    kit::card().inner_margin(egui::Margin::same(18.0)).show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        match self.tab {
                            Tab::Devices | Tab::Keyboard | Tab::Mouse | Tab::Pump | Tab::Fans | Tab::Argb | Tab::System => {}
                            Tab::App => self.ui_app(ui),
                        }
                    });
                });
            });

        if let Some(t) = go {
            // Мышь читается заново при входе во вкладку «Мышь».
            if t == Tab::Mouse {
                self.mouse.refresh_async(ctx);
            }
            self.leave(self.tab, t);
            self.tab = t;
        }
        self.ui_toast(ctx);

        // Статус службы обновляется раз в секунду.
        ctx.request_repaint_after(Duration::from_secs(1));
    }
}

/// Палитра и стиль из C#-версии (Palette.xaml / Styles.xaml): чёрный фон, серые карточки,
/// светлый акцент, скругления 8–10 px.
#[allow(dead_code)]
mod theme {
    use eframe::egui::{self, Color32, Rounding, Stroke};

    pub const WINDOW: Color32 = Color32::from_rgb(0x00, 0x00, 0x00);
    pub const SIDEBAR: Color32 = Color32::from_rgb(0x05, 0x05, 0x05);
    pub const SURFACE: Color32 = Color32::from_rgb(0x0A, 0x0A, 0x0A);
    pub const CARD: Color32 = Color32::from_rgb(0x0E, 0x0E, 0x0E);
    pub const SURFACE2: Color32 = Color32::from_rgb(0x18, 0x18, 0x18);
    pub const HOVER: Color32 = Color32::from_rgb(0x22, 0x22, 0x22);
    pub const PRESSED: Color32 = Color32::from_rgb(0x2E, 0x2E, 0x2E);
    pub const LINE: Color32 = Color32::from_rgb(0x26, 0x26, 0x26);
    pub const NAV_SELECTED: Color32 = Color32::from_rgb(0x1A, 0x1A, 0x1A);
    pub const TEXT: Color32 = Color32::from_rgb(0xF2, 0xF2, 0xF2);
    pub const MUTED: Color32 = Color32::from_rgb(0x9A, 0x9A, 0x9A);
    pub const ACCENT: Color32 = Color32::from_rgb(0xF2, 0xF2, 0xF2);
    pub const BAD: Color32 = Color32::from_rgb(0xFF, 0x8A, 0x96);

    pub fn apply(ctx: &egui::Context) {
        let mut v = egui::Visuals::dark();
        v.panel_fill = WINDOW;
        v.window_fill = CARD;
        v.extreme_bg_color = SURFACE;
        v.faint_bg_color = CARD;
        v.code_bg_color = SURFACE2;
        v.window_stroke = Stroke::new(1.0_f32, LINE);
        v.window_rounding = Rounding::same(10.0);
        v.menu_rounding = Rounding::same(8.0);
        v.error_fg_color = BAD;
        v.hyperlink_color = ACCENT;
        v.selection.bg_fill = NAV_SELECTED;
        v.selection.stroke = Stroke::new(1.0_f32, TEXT);

        let r = Rounding::same(8.0);
        let w = &mut v.widgets;
        w.noninteractive.bg_fill = CARD;
        w.noninteractive.weak_bg_fill = CARD;
        w.noninteractive.bg_stroke = Stroke::new(1.0_f32, LINE);
        w.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT);
        w.noninteractive.rounding = r;
        w.inactive.bg_fill = SURFACE2;
        w.inactive.weak_bg_fill = SURFACE2;
        w.inactive.bg_stroke = Stroke::new(1.0_f32, LINE);
        w.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
        w.inactive.rounding = r;
        w.hovered.bg_fill = HOVER;
        w.hovered.weak_bg_fill = HOVER;
        w.hovered.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(0x3C, 0x3C, 0x3C));
        w.hovered.fg_stroke = Stroke::new(1.5_f32, Color32::WHITE);
        w.hovered.rounding = r;
        w.active.bg_fill = PRESSED;
        w.active.weak_bg_fill = PRESSED;
        w.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
        w.active.fg_stroke = Stroke::new(1.5_f32, Color32::WHITE);
        w.active.rounding = r;
        w.open.bg_fill = SURFACE2;
        w.open.weak_bg_fill = SURFACE2;
        w.open.rounding = r;
        ctx.set_visuals(v);

        ctx.style_mut(|s| {
            s.spacing.item_spacing = egui::vec2(10.0, 8.0);
            s.spacing.button_padding = egui::vec2(14.0, 6.0);
            s.spacing.interact_size.y = 28.0;
            s.spacing.slider_width = 220.0;
            for (ts, size) in [
                (egui::TextStyle::Body, 14.0),
                (egui::TextStyle::Button, 14.0),
                (egui::TextStyle::Heading, 20.0),
                (egui::TextStyle::Small, 11.0),
            ] {
                if let Some(f) = s.text_styles.get_mut(&ts) {
                    f.size = size;
                }
            }
        });
    }
}

/// Имя файла без папки (для подписи рядом с кнопкой «Выбрать…»).
fn file_name(path: &str) -> String {
    if path.trim().is_empty() {
        return "файл не выбран".into();
    }
    std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string())
}

fn main() -> Result<(), eframe::Error> {
    use rigcontrol_core::single_instance::{self, Instance};
    // Старый автозапуск (окно с --tray): окно не нужно — запустить фоновую службу и выйти.
    if std::env::args().any(|a| a == "--tray") {
        if !service_link::running() {
            let _ = service_link::spawn();
        }
        if autostart::status() == autostart::Status::Legacy {
            let _ = autostart::set(true, "");
        }
        return Ok(());
    }
    // Вторая копия не открывается: просим уже запущенную показать окно и выходим.
    let lock = match single_instance::acquire() {
        Instance::First(l) => l,
        Instance::AlreadyRunning => return Ok(()),
    };
    // Фоновая служба отдаёт устройства окну (ждём, пока отдаст, — иначе они достанутся обоим сразу).
    let service = match service_link::pause() {
        Ok(p) => p,
        // Служба есть, но устройства не отдала: просим её выйти совсем (ответ «OK» — когда всё остановлено).
        // При закрытии окна с «работой в фоне» она запустится снова.
        Err(_) if service_link::quit() => None,
        // Ни пауза, ни выход не подтверждены: устройства, возможно, ещё у службы. Свои потоки рядом с её
        // потоками не запускаем (клавиатура, помпа и вентиляторы получали бы команды от двух программ) —
        // говорим, что делать, и выходим.
        Err(e) => {
            rfd::MessageDialog::new()
                .set_level(rfd::MessageLevel::Warning)
                .set_title(WINDOW_TITLE)
                .set_description(format!(
                    "Фоновая служба RigControl не отдала устройства ({e}).\n\n\
                     Закройте её: значок RigControl в трее → «Выход» (если не закрывается — снимите \
                     rigcontrol-service.exe в диспетчере задач) и откройте окно снова."
                ))
                .show();
            return Ok(());
        }
    };
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(WINDOW_TITLE)
        // Меню 236 px + содержимое: меньше этого окно становится тесным, поэтому ниже не даём.
        .with_inner_size([1180.0, 800.0])
        .with_min_inner_size([1020.0, 680.0]);
    // Значок окна и панели задач — тот же логотип, что app.ico в C#-версии.
    if let Some(rgba) = logo_rgba(64) {
        viewport = viewport.with_icon(egui::IconData { rgba, width: 64, height: 64 });
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(
        "RigControl",
        options,
        Box::new(move |cc| {
            kit::install_fonts(&cc.egui_ctx);
            theme::apply(&cc.egui_ctx);
            let ctx = cc.egui_ctx.clone();
            single_instance::listen(lock, move || restore(&ctx));
            Ok(Box::new(App::new(service)))
        }),
    )
}
