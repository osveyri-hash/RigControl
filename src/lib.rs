//! Ядро RigControl на Rust. Портируется по плану rust-anything (см. PLAN.md),
//! по одному модулю: сначала чистая логика без UI и без USB.
//!
//! Готово:
//! - [`protocol`] — Ak820Protocol.cs
//! - [`effects`]  — LightEffects.cs
//! - [`keypaint`] — KeyPaint.cs
//! - [`color`]    — HSV-функции из ColorWheel.cs
//! - [`dongle`]   — Ak820Dongle.cs: клавиатура по радио (часы, подсветка, заряд)
//! - [`keyboard`] — клавиатура по кабелю или по радио (выбор сам)
//! - [`kb_light`] — подсветка клавиатуры в своём потоке: свои эффекты кадрами, в такт гифке, «Выключить», слежение за кабелем
//! - [`gif_light`] — GifLight.cs: гифка с экрана по клавишам («Картинка», «Волна от экрана»), главный цвет кадра
//! - [`device`]   — Ak820Device.cs + HidInterface.cs (через крейт hidapi)
//! - [`cooler`]   — Cooler/CoolerLcd.cs: экран помпы Thermalright
//! - [`mouse`]    — Mouse/McMouse.cs: мыши MCHOSE (DPI, частота, сенсор, сон, профили)
//! - [`argb`]     — Argb/ArgbEngine.cs: эффекты ARGB и синхронизация с клавиатурой
//! - [`aura`]     — Argb/AuraUsb.cs: подсветка ASUS AURA (ARGB-разъёмы платы)
//! - [`mousekeys`] — Mouse/MouseKeys.cs: назначение кнопок мыши
//! - [`screen`]   — ImageFrames.cs: картинки и гифки для экранчика клавиатуры
//! - [`lcdscene`] — Cooler/LcdItem.cs + LcdScene.cs + LcdThemes.cs: сцены с датчиками на экране помпы
//! - [`telemetry`] — Cooler/Telemetry.cs: загрузка, температуры, память, сеть, диск (LHM, sysinfo, nvidia-smi)
//! - [`lhm`]      — LibreHardwareMonitor через помощника sensors\rigcontrol-sensors.exe
//! - [`fans`]     — MainWindow.Fans.cs: скорость вентиляторов платы по режимам и кривым
//! - [`settings`] — Settings.cs: settings.json (совместим с C#-версией)
//! - [`logfile`]  — LogFile.cs: журналы текущего дня
//! - [`autostart`] — Autostart.cs: автозапуск через планировщик
//! - [`service`]  — ARGB и экран помпы по settings.json (потоки)
//! - [`host`], [`service_link`] — фоновая служба rigcontrol-service.exe и её связь с окном
//! - [`timing`]   — TimerResolution.cs: точный таймер и ровный шаг кадров

pub mod argb;
pub mod armoury;
pub mod autostart;
pub mod aura;
pub mod battery;
pub mod color;
#[cfg(feature = "gui")]
pub mod color_wheel;
#[cfg(feature = "gui")]
pub mod device_art;
pub mod cooler;
pub mod device;
pub mod dongle;
pub mod effects;
pub mod fan_curve;
pub mod fans;
pub mod gif_light;
pub mod host;
pub mod kb_light;
pub mod keyboard;
#[cfg(feature = "gui")]
pub mod key_painter;
pub mod keypaint;
pub mod lhm;
pub mod light_mirror;
pub mod lcdscene;
pub mod logfile;
pub mod memory;
pub mod mouse;
#[cfg(feature = "gui")]
pub mod led_art;
#[cfg(feature = "gui")]
pub mod mouse_panel;
pub mod mousekeys;
pub mod profiles;
pub mod protocol;
pub mod pump_art;
#[cfg(feature = "gui")]
pub mod pump_editor;
pub mod screen;
pub mod screen_upload;
pub mod service;
pub mod service_link;
pub mod settings;
pub mod single_instance;
#[cfg(feature = "gui")]
pub mod system_panel;
pub mod telemetry;
pub mod timing;
#[cfg(feature = "gui")]
pub mod ui_kit;

/// Цвет (R, G, B), как кортеж `(byte R, byte G, byte B)` в C#.
pub type Rgb = (u8, u8, u8);
