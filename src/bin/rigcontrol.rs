//! rigcontrol — консольная утилита для AJAZZ AK820 Pro (по кабелю USB-C и по радио) и остального железа.

use chrono::{Datelike, Local, Timelike};
use rigcontrol_core::device::{describe_vendor_interfaces, Ak820Device};
use rigcontrol_core::protocol::{DateTimeParts, EFFECTS};
use rigcontrol_core::cooler::{self, CoolerLcd};
use rigcontrol_core::{aura, effects, keypaint, protocol, screen, timing};
use std::io::Write;
use std::process::ExitCode;
use std::time::{Duration, Instant};

const HELP: &str = "rigcontrol — управление подсветкой AJAZZ AK820 Pro

Настройки и фоновый режим:
  settings show|path|set ИМЯ ЗНАЧЕНИЕ|reset --yes
  daemon [--seconds N] [--no-keyboard] [--no-fans]
        фоновый режим по settings.json: ARGB, экран помпы и вентиляторы платы
        (для вентиляторов и температур — консоль от имени администратора)
  autostart status|on [АРГУМЕНТЫ]|off

Команды:
  rigcontrol list                     список эффектов и наборов раскраски
  rigcontrol info                     найти клавиатуру и показать HID-интерфейсы
  rigcontrol off                      выключить подсветку
  rigcontrol effect <номер|имя> [опции]
        встроенные эффекты 0–19 сохраняются в клавиатуре;
        свои эффекты 100–109 рисуются программой, пока она запущена (Ctrl+C — стоп)
  rigcontrol paint <набор> [--brightness N]   своя раскраска (Сакура, Мята, WASD…)
  rigcontrol time                     синхронизировать часы на экранчике (кабель или донгл — сама выберет)

Мышь MCHOSE (A7 / A7 Pro / A7 Ultra, 5253:xxxx):
  rigcontrol mouse info                     состояние, заряд, прошивка, DPI, частота, сенсор
  rigcontrol mouse dpi 400,800,1600 [--select N]   уровни DPI (1..6 шт.), N — активный с 1
  rigcontrol mouse level N                  выбрать уровень DPI (с 1)
  rigcontrol mouse rate 1000                частота опроса, Гц (125/500/1000/2000/4000/8000)
  rigcontrol mouse lod 0|1|2                высота отрыва: 0,7 / 1 / 2 мм
  rigcontrol mouse mode power|game|super    режим сенсора
  rigcontrol mouse ripple|angle|motion-sync on|off
  rigcontrol mouse rotate -30..30           угол сенсора
  rigcontrol mouse debounce 0..20           антидребезг, мс
  rigcontrol mouse sleep MIN                сон через MIN минут, 0 — не засыпать
  rigcontrol mouse profile 1|2|3            встроенный профиль
  rigcontrol mouse reset-keys               кнопки как с завода
  rigcontrol mouse keys                     что сейчас назначено на кнопки
  rigcontrol mouse actions                  список функций для bind
  rigcontrol mouse bind КНОПКА ФУНКЦИЯ      КНОПКА: left|middle|right|forward|back (или 1..5);
                                            ФУНКЦИЯ: id из actions, default или key:ctrl+shift+f5
  rigcontrol mouse factory-reset --yes      заводские настройки профиля

Подсветка ASUS AURA (ARGB-разъёмы платы, 0B05:19AF):
  rigcontrol aura info                прошивка, число разъёмов и светодиодов платы
  rigcontrol aura color RRGGBB        постоянный цвет (запоминает плата)
  rigcontrol aura effect <имя> [RRGGBB]  встроенный эффект: static, breathing, flash, cycle, rainbow
  rigcontrol aura off                 выключить
  rigcontrol aura rainbow             бегущая радуга от программы (прямой режим), Ctrl+C — стоп
  rigcontrol aura fill RRGGBB         один цвет в прямом режиме (проверка протокола 0x40)

Эффекты ARGB (вентиляторы и ленты на разъёмах платы ASUS):
  rigcontrol argb list                список эффектов
  rigcontrol argb <эффект> [RRGGBB] [опции]   живой эффект, Ctrl+C — стоп
    эффекты: off static breathing spectrum rainbow wave comet temp sync
    --color2 RRGGBB --color3 RRGGBB   дополнительные цвета для wave
    --brightness 0-100  --speed 1-5  --leds 1-120  --header N|all  --board  --seconds S
    --temp C            температура для temp (датчики будут позже)
    sync: --effect 106-109 (спектр, волна от центра, течение, вращение) --fans N
          --kspeed 0-5 --dir 0-3 --keyboard (заодно рисовать эффект на клавиатуре по кабелю)
        --header N|all  разъём ARGB (1, 2, …; по умолчанию all)
        --board         заодно подсветка самой платы и RGB 12 В
        --leds N        светодиодов на ленте (1–120, по умолчанию 60) — для rainbow/fill
        --seconds S     сколько крутить rainbow (по умолчанию бесконечно)

Клавиатура по радио (донгл 2,4 ГГц, 0C45:FDFD):
  rigcontrol dongle info               найти донгл
  rigcontrol dongle battery            заряд клавиатуры, %
  rigcontrol dongle listen [СЕК]      показать всё, что шлёт донгл (диагностика)
  к любой команде dongle можно добавить --debug — печать пакетов
  rigcontrol dongle time               синхронизировать часы
  rigcontrol dongle off                выключить подсветку
  rigcontrol dongle effect <номер|имя> [опции]  встроенные эффекты 0–19 (опции как у effect)
  rigcontrol dongle paint <набор> [--brightness N]  своя раскраска
  Экранчик по радио не работает — только по кабелю.

Экранчик клавиатуры (128×128, сохраняется во флеш клавиатуры):
  rigcontrol screen image <файл>      залить картинку или гифку (png, jpg, gif, bmp)
        --fit  вписать целиком (по умолчанию — заполнить с обрезкой)
        --frames N  сколько кадров гифки оставить (1–140, по умолчанию все до 140)
  rigcontrol screen color RRGGBB     залить экранчик цветом

Экран помпы Thermalright (0416:5302):
  rigcontrol pump info                рукопожатие и диагностика
  rigcontrol pump color RRGGBB       залить экран цветом
  rigcontrol pump image <файл>        картинка или гифка (png, jpg, gif, bmp)
  rigcontrol pump scene [опции]      часы и датчики (CPU/GPU/RAM/сеть), обновление раз в секунду
        --theme ИМЯ        тема (список: pump themes), по умолчанию «Классика»
        --bg ФАЙЛ          свой фон (первый кадр картинки/гифки)
        --brightness 0–100  яркость фона
        --add ТИП:X:Y[:МАСШТАБ[:ЦВЕТ]]  добавить объект, например cputemp:20:180:1.2:FF8800
        --text ТЕКСТ  --clock-seconds  --cpu-temp N  --gpu-temp N  --cpu-power N
        --rotate 0|90|180|270  --seconds N
        --preview out.png   сохранить кадр в файл без помпы
  rigcontrol pump themes             темы и объекты сцены
  rigcontrol pump sensors            показать показания датчиков
        --rotate 0|90|180|270  поворот;  --fit  вписать целиком (по умолчанию — заполнить с обрезкой)
        --seconds N  сколько показывать (по умолчанию бесконечно, Ctrl+C — стоп)
        --chunk N    размер порции USB (по умолчанию 512)

Опции эффекта:
  --color RRGGBB   цвет (по умолчанию #FFFFFF)
  --rainbow         радужный режим (цвет игнорируется)
  --brightness 0-5  яркость (по умолчанию 5)
  --speed 0-5       скорость (по умолчанию 3)
  --dir 0-3         направление (по умолчанию 0)
  --seconds N       для своих эффектов: сколько секунд крутить (по умолчанию бесконечно)

Примеры:
  rigcontrol effect 8 --rainbow
  rigcontrol effect \"Дыхание\" --color #FF0050 --speed 2
  rigcontrol effect 106 --seconds 30
  rigcontrol paint Сакура
  rigcontrol screen image C:\\Users\\me\\cat.gif --frames 60
  rigcontrol pump image C:\\Users\\me\\cat.gif --seconds 60";

struct Opts {
    color: String,
    rainbow: bool,
    brightness: i32,
    speed: i32,
    dir: i32,
    seconds: Option<f64>,
}

fn parse_opts(args: &[String]) -> Result<Opts, String> {
    let mut o = Opts { color: "#FFFFFF".into(), rainbow: false, brightness: 5, speed: 3, dir: 0, seconds: None };
    let mut i = 0;
    let num = |v: Option<&String>, name: &str| -> Result<f64, String> {
        v.ok_or(format!("{name}: нужно значение"))?.parse::<f64>().map_err(|_| format!("{name}: не число"))
    };
    while i < args.len() {
        match args[i].as_str() {
            "--color" => { o.color = args.get(i + 1).ok_or("--color: нужно значение")?.clone(); i += 1; }
            "--rainbow" => o.rainbow = true,
            "--brightness" => { o.brightness = num(args.get(i + 1), "--brightness")? as i32; i += 1; }
            "--speed" => { o.speed = num(args.get(i + 1), "--speed")? as i32; i += 1; }
            "--dir" => { o.dir = num(args.get(i + 1), "--dir")? as i32; i += 1; }
            "--seconds" => { o.seconds = Some(num(args.get(i + 1), "--seconds")?); i += 1; }
            other => return Err(format!("Неизвестная опция: {other}")),
        }
        i += 1;
    }
    Ok(o)
}

fn resolve_effect(name: &str) -> Option<(&'static str, i32)> {
    if let Ok(n) = name.parse::<i32>() {
        return EFFECTS.iter().copied().find(|(_, m)| *m == n);
    }
    let wanted = name.to_lowercase();
    EFFECTS.iter().copied().find(|(n, _)| n.to_lowercase() == wanted)
}

fn open() -> Result<(hidapi::HidApi, Ak820Device), String> {
    let api = hidapi::HidApi::new().map_err(|e| format!("hidapi: {e}"))?;
    let dev = Ak820Device::open(&api).map_err(|e| {
        if rigcontrol_core::dongle::Ak820Dongle::present(&api) {
            format!("{e}\nПодсказка: донгл найден — по радио работают команды rigcontrol dongle … (time, off, effect, paint, battery).")
        } else {
            e
        }
    })?;
    Ok((api, dev))
}

fn run_live(dev: &mut Ak820Device, mode: i32, o: &Opts) -> Result<(), String> {
    if mode == effects::GIF_FLOW || mode == effects::PAINT {
        return Err("Этот эффект пока не поддерживается в консольной версии (нужны гифка или раскраска).".into());
    }
    let color = protocol::parse_color(Some(&o.color), (255, 255, 255));
    let start = Instant::now();
    let frame_dt = Duration::from_millis(effects::FRAME_INTERVAL_MS);
    let _timer = timing::HighResTimer::begin();
    let mut pacer = timing::Pacer::new(frame_dt);
    let mut skipped = 0u32;
    println!("Рисую эффект… Ctrl+C — остановить (клавиатура вернётся к сохранённому эффекту).");
    loop {
        let t = start.elapsed().as_secs_f64();
        if let Some(limit) = o.seconds {
            if t >= limit { break; }
        }
        let mut bank = effects::build(mode, t, o.speed, o.dir, color);
        if o.brightness < 5 {
            effects::dim(&mut bank, o.brightness.clamp(0, 5) as f64 / 5.0);
        }
        let live = protocol::live_frame(&bank);
        if !dev.send_live_frame(&live)? {
            skipped += 1;
        }
        pacer.wait();
    }
    if skipped > 0 {
        println!("Про��ущено кадров (клавиатура занята): {skipped}");
    }
    Ok(())
}

struct PumpOpts {
    rotate: u32,
    fit: bool,
    seconds: Option<f64>,
    chunk: Option<usize>,
}

fn parse_pump_opts(args: &[String]) -> Result<PumpOpts, String> {
    let mut o = PumpOpts { rotate: 0, fit: false, seconds: None, chunk: None };
    let mut i = 0;
    while i < args.len() {
        let value = || args.get(i + 1).ok_or(format!("{}: нужно значение", args[i]));
        match args[i].as_str() {
            "--rotate" => { o.rotate = value()?.parse().map_err(|_| "--rotate: 0, 90, 180 или 270")?; i += 1; }
            "--fit" => o.fit = true,
            "--seconds" => { o.seconds = Some(value()?.parse().map_err(|_| "--seconds: не число")?); i += 1; }
            "--chunk" => { o.chunk = Some(value()?.parse().map_err(|_| "--chunk: не число")?); i += 1; }
            other => return Err(format!("Неизвестная опция: {other}")),
        }
        i += 1;
    }
    if !matches!(o.rotate, 0 | 90 | 180 | 270) {
        return Err("--rotate: 0, 90, 180 или 270".into());
    }
    Ok(o)
}

/// Крутит кадры на экране помпы. Одиночную картинку повторяем раз в секунду — иначе помпа уходит на логотип.
fn play_pump(lcd: &mut CoolerLcd, frames: &[cooler::LcdFrame], seconds: Option<f64>) -> Result<(), String> {
    let start = Instant::now();
    println!("Показываю на экране помпы… Ctrl+C — остановить.");
    let mut i = 0usize;
    loop {
        if let Some(limit) = seconds {
            if start.elapsed().as_secs_f64() >= limit { break; }
        }
        let tick = Instant::now();
        let frame = &frames[i % frames.len()];
        lcd.send_image(&frame.image)?;
        if let Some(rest) = Duration::from_millis(frame.delay_ms as u64).checked_sub(tick.elapsed()) {
            std::thread::sleep(rest);
        }
        i += 1;
    }
    println!("Кадров отправлено: {}", lcd.frames_sent);
    Ok(())
}

fn run_pump(args: &[String]) -> Result<(), String> {
    let sub = args.first().map(|s| s.as_str()).unwrap_or("info");
    let api = hidapi::HidApi::new().map_err(|e| format!("hidapi: {e}"))?;
    match sub {
        "info" => {
            let o = parse_pump_opts(&args[1.min(args.len())..])?;
            let lcd = CoolerLcd::open(&api, o.chunk)?;
            println!("{}", lcd.description());
            println!("Разрешение: {}×{}, формат: {}", lcd.resolution.0, lcd.resolution.1, if lcd.use_rgb565() { "RGB565" } else { "JPEG" });
            print!("{}", lcd.diagnostics());
        }
        "color" => {
            let hex = args.get(1).ok_or("Укажите цвет: rigcontrol pump color RRGGBB")?;
            let o = parse_pump_opts(&args[2..])?;
            let mut lcd = CoolerLcd::open(&api, o.chunk)?;
            if !lcd.handshake_ok { eprint!("Внимание: {}", lcd.diagnostics()); }
            let (w, h) = lcd.resolution;
            let frame = cooler::LcdFrame { image: cooler::solid(w, h, protocol::parse_color(Some(hex), (0, 0, 0))), delay_ms: 1000 };
            play_pump(&mut lcd, std::slice::from_ref(&frame), o.seconds.or(Some(10.0)))?;
        }
        "image" => {
            let path = args.get(1).ok_or("Укажите файл: rigcontrol pump image <файл>")?;
            let o = parse_pump_opts(&args[2..])?;
            let mut lcd = CoolerLcd::open(&api, o.chunk)?;
            if !lcd.handshake_ok { eprint!("Внимание: {}", lcd.diagnostics()); }
            let (w, h) = lcd.resolution;
            let frames = cooler::load_image(std::path::Path::new(path), w, h, o.rotate, !o.fit)?;
            println!("{} · {}×{} · кадров: {}", lcd.description(), w, h, frames.len());
            play_pump(&mut lcd, &frames, o.seconds)?;
        }
        "themes" => {
            use rigcontrol_core::lcdscene;
            println!("Темы экрана помпы (rigcontrol pump scene --theme ИМЯ):");
            for t in lcdscene::all_themes() {
                let bg = t.image.map(|i| format!(" · фон в оригинале: {i} (свой — --bg)")).unwrap_or_default();
                println!("  {:<12} {} → {}{}", t.name, t.top, t.bottom, bg);
            }
            println!("\nОбъекты для --add ТИП:X:Y[:МАСШТАБ[:ЦВЕТ]]:");
            for (n, _, d) in lcdscene::KINDS {
                println!("  {n:<9} {d}");
            }
        }
        "sensors" => {
            let mut tel = rigcontrol_core::telemetry::Telemetry::new();
            std::thread::sleep(Duration::from_millis(1000));
            let s = tel.sample();
            let o = |v: Option<f64>, unit: &str| v.map(|v| format!("{v:.1} {unit}")).unwrap_or_else(|| "нет данных".into());
            println!("CPU: {:.0}% · {} · {} · {}", s.cpu_load, o(s.cpu_temp, "°C"), o(s.cpu_clock_mhz, "МГц"), o(s.cpu_power_w, "Вт"));
            println!("GPU: {} · {} · {} · VRAM {} / {}", o(s.gpu_load, "%"), o(s.gpu_temp, "°C"), o(s.gpu_power_w, "Вт"), o(s.vram_used_gb, "ГБ"), o(s.vram_total_gb, "ГБ"));
            println!("RAM: {:.1} / {:.1} ГБ ({:.0}%)", s.ram_used_gb, s.ram_total_gb, s.ram_load);
            println!("Сеть: ↓ {} ↑ {}", rigcontrol_core::lcdscene::speed(s.net_down_bps), rigcontrol_core::lcdscene::speed(s.net_up_bps));
            println!("Диск C: занято {:.0}%, свободно {:.0} ГБ · аптайм {} мин", s.disk_used_pct, s.disk_free_gb, s.uptime_secs / 60);
            if s.cpu_temp.is_none() {
                println!("Температуру CPU Windows не отдала — задайте её в сцене через --cpu-temp или запустите от администратора.");
            }
        }
        "scene" => {
            use rigcontrol_core::lcdscene::{self, Background, Fonts, LcdItem, LcdKind};
            use rigcontrol_core::telemetry::Telemetry;
            let mut theme_name = String::from("Классика");
            let mut bg_path: Option<String> = None;
            let mut brightness: Option<f64> = None;
            let mut preview: Option<String> = None;
            let mut adds: Vec<LcdItem> = Vec::new();
            let mut seconds_on_time = false;
            let mut text: Option<String> = None;
            let mut tel = Telemetry::new();
            let mut rest: Vec<String> = Vec::new();
            let mut i = 1;
            let num = |v: Option<&String>, what: &str| -> Result<f64, String> {
                v.and_then(|v| v.replace(',', ".").parse::<f64>().ok()).ok_or_else(|| format!("{what}: нужно число"))
            };
            while i < args.len() {
                let a = args[i].as_str();
                let v = args.get(i + 1);
                match a {
                    "--theme" => { theme_name = v.ok_or("--theme: укажите имя")?.clone(); i += 1; }
                    "--bg" => { bg_path = Some(v.ok_or("--bg: укажите файл")?.clone()); i += 1; }
                    "--brightness" => { brightness = Some(num(v, a)?.clamp(0.0, 100.0)); i += 1; }
                    "--preview" => { preview = Some(v.ok_or("--preview: укажите файл .png")?.clone()); i += 1; }
                    "--cpu-temp" => { tel.overrides.cpu_temp = Some(num(v, a)?); i += 1; }
                    "--gpu-temp" => { tel.overrides.gpu_temp = Some(num(v, a)?); i += 1; }
                    "--cpu-power" => { tel.overrides.cpu_power_w = Some(num(v, a)?); i += 1; }
                    "--clock-seconds" => seconds_on_time = true,
                    "--text" => { text = Some(v.ok_or("--text: укажите текст")?.clone()); i += 1; }
                    "--add" => {
                        let spec = v.ok_or("--add ТИП:X:Y[:МАСШТАБ[:ЦВЕТ]]")?;
                        let p: Vec<&str> = spec.split(':').collect();
                        let kind = LcdKind::parse(p[0]).ok_or_else(|| format!("Нет такого объекта: {}. Список: rigcontrol pump themes", p[0]))?;
                        let f = |k: usize, d: f64| p.get(k).and_then(|s| s.parse::<f64>().ok()).unwrap_or(d);
                        let mut item = LcdItem::create(kind);
                        item.x = f(1, 16.0);
                        item.y = f(2, 16.0);
                        item.scale = f(3, 1.0);
                        if let Some(c) = p.get(4) { item.color = format!("#{}", c.trim_start_matches('#')); }
                        adds.push(item);
                        i += 1;
                    }
                    _ => rest.push(args[i].clone()),
                }
                i += 1;
            }
            let o = parse_pump_opts(&rest)?;
            let theme = lcdscene::find_theme(&theme_name)
                .ok_or_else(|| format!("Нет темы «{theme_name}». Список: rigcontrol pump themes"))?;

            // Без помпы (--preview) рисуем 320×240.
            let mut lcd = if preview.is_some() { None } else { Some(CoolerLcd::open(&api, o.chunk)?) };
            if let Some(l) = &lcd {
                if !l.handshake_ok { eprint!("Внимание: {}", l.diagnostics()); }
            }
            let (pw, ph) = lcd.as_ref().map(|l| l.resolution).unwrap_or((320, 240));
            let swap = matches!(o.rotate, 90 | 270);
            let (w, h) = if swap { (ph, pw) } else { (pw, ph) };

            let mut items = lcdscene::layout_for(&theme, w, h);
            for it in items.iter_mut() {
                if it.kind == LcdKind::Time && seconds_on_time { it.seconds = true; }
                if it.kind == LcdKind::Text { if let Some(t) = &text { it.text = t.clone(); } }
            }
            for mut it in adds {
                if it.kind == LcdKind::Text { if let Some(t) = &text { it.text = t.clone(); } }
                items.push(it);
            }

            let bg_image = match &bg_path {
                Some(p) => Some(cooler::load_image(std::path::Path::new(p), w, h, 0, true)?.remove(0).image),
                None => None,
            };
            let k = brightness.or(theme.brightness.map(|b| b as f64)).unwrap_or(100.0) / 100.0;
            let top = lcdscene::parse_color(theme.top).unwrap_or((0x10, 0x12, 0x1A));
            let bottom = lcdscene::parse_color(theme.bottom).unwrap_or((5, 6, 10));
            let mut fonts = Fonts::default();

            let draw = |snap: &rigcontrol_core::telemetry::Snapshot, now: &chrono::NaiveDateTime, fonts: &mut Fonts| {
                let bg = match &bg_image { Some(img) => Background::Image(img), None => Background::Gradient(top, bottom) };
                let img = lcdscene::render(w, h, &items, snap, now, bg, k, fonts);
                match o.rotate {
                    90 => image::imageops::rotate90(&img),
                    180 => image::imageops::rotate180(&img),
                    270 => image::imageops::rotate270(&img),
                    _ => img,
                }
            };

            // Первый замер загрузки CPU и сети нужен как точка отсчёта.
            std::thread::sleep(Duration::from_millis(500));

            if let Some(path) = preview {
                let snap = tel.sample();
                let img = draw(&snap, &lcdscene::now(), &mut fonts);
                img.save(&path).map_err(|e| format!("{path}: {e}"))?;
                println!("Кадр {}×{} сохранён в {path}", img.width(), img.height());
                println!("Датчики: {snap:?}");
                if !fonts.fallback_used.is_empty() {
                    println!("Нет шрифтов {:?} — взят встроенный M PLUS Rounded.", fonts.fallback_used);
                }
                return Ok(());
            }

            let lcd = lcd.as_mut().expect("помпа открыта");
            println!("{} · сцена «{}» {}×{} · объектов: {}. Ctrl+C — остановить.", lcd.description(), theme.name, w, h, items.len());
            let start = Instant::now();
            let mut last_sig = String::new();
            let mut last_frame: Option<image::RgbImage> = None;
            let mut last_sent = Instant::now();
            loop {
                if let Some(limit) = o.seconds {
                    if start.elapsed().as_secs_f64() >= limit { break; }
                }
                let tick = Instant::now();
                let snap = tel.sample();
                let now = lcdscene::now();
                // Как в C#: перерисовываем только если поменялся отпечаток сцены, но кадр повторяем
                // раз в секунду — иначе помпа уходит на логотип.
                let sig = lcdscene::signature(&items, &snap, &now);
                if sig != last_sig || last_frame.is_none() {
                    last_frame = Some(draw(&snap, &now, &mut fonts));
                    last_sig = sig;
                }
                if let Some(frame) = &last_frame {
                    if let Err(e) = lcd.send_image(frame) {
                        eprintln!("Ошибка отправки: {e}. Пробую снова рукопожатие…");
                        lcd.handshake(2000);
                    }
                    last_sent = Instant::now();
                }
                let _ = last_sent;
                if let Some(rest) = Duration::from_millis(1000).checked_sub(tick.elapsed()) {
                    std::thread::sleep(rest);
                }
            }
            println!("Кадров отправлено: {}", lcd.frames_sent);
        }
        other => return Err(format!("Неизвестная команда pump: {other}. Есть: info, color, image, scene, themes, sensors")),
    }
    Ok(())
}

fn upload_frames(frames: &[protocol::ScreenFrame]) -> Result<(), String> {
    let blob = protocol::screen_blob(frames).map_err(|e| e.to_string())?;
    let (_api, dev) = open()?;
    println!("Кадров: {}, размер: {} КиБ. Не отключайте клавиатуру...", frames.len().min(protocol::MAX_FRAMES), blob.len() / 1024);
    let stats = dev.upload_screen(&blob, |done, total| {
        print!("\r  {done}/{total} порций ({}%)", done * 100 / total.max(1));
        let _ = std::io::stdout().flush();
    })?;
    println!();
    println!(
        "Готово за {:.1} с: порций {}, ACK {}, запись {:.1} с, ожидание ACK {:.1} с.",
        stats.total_seconds, stats.chunks, stats.acks, stats.write_seconds, stats.ack_seconds
    );
    if stats.acks == 0 {
        println!("Внимание: клавиатура не прислала ни одного ACK — если картинка не сменилась, пришлите этот вывод.");
    }
    Ok(())
}

fn run_screen(args: &[String]) -> Result<(), String> {
    match args.first().map(|s| s.as_str()) {
        Some("image") => {
            let path = args.get(1).ok_or("Укажите файл: rigcontrol screen image <файл>")?;
            let mut crop = true;
            let mut max = 0usize;
            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--fit" => crop = false,
                    "--crop" => crop = true,
                    "--frames" => {
                        i += 1;
                        max = args.get(i).and_then(|v| v.parse().ok()).ok_or("--frames: нужно число 1–140")?;
                    }
                    other => return Err(format!("Неизвестная опция: {other}")),
                }
                i += 1;
            }
            let frames = screen::load(std::path::Path::new(path), crop, max)?;
            upload_frames(&frames)
        }
        Some("color") => {
            let hex = args.get(1).ok_or("Укажите цвет: rigcontrol screen color RRGGBB")?;
            let color = protocol::parse_color(Some(hex), (0, 0, 0));
            upload_frames(&[screen::solid(color)])
        }
        _ => Err("Подкоманды: screen image <файл> | screen color RRGGBB".into()),
    }
}

struct AuraOpts {
    header: Option<usize>,
    board: bool,
    leds: usize,
    seconds: Option<f64>,
}

fn parse_aura_opts(args: &[String]) -> Result<AuraOpts, String> {
    let mut o = AuraOpts { header: None, board: false, leds: 60, seconds: None };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--board" => o.board = true,
            "--header" => {
                i += 1;
                let v = args.get(i).ok_or("--header: нужен номер разъёма или all")?;
                o.header = if v == "all" { None } else { Some(v.parse::<usize>().ok().filter(|&n| n >= 1).ok_or("--header: номер с 1")?) };
            }
            "--leds" => {
                i += 1;
                o.leds = args.get(i).and_then(|v| v.parse().ok()).ok_or("--leds: нужно число 1–120")?;
                o.leds = o.leds.clamp(1, aura::MAX_LEDS);
            }
            "--seconds" => {
                i += 1;
                o.seconds = Some(args.get(i).and_then(|v| v.parse().ok()).ok_or("--seconds: нужно число")?);
            }
            other => return Err(format!("Неизвестная опция: {other}")),
        }
        i += 1;
    }
    Ok(o)
}

/// Разъёмы прямого режима (0-based) и каналы эффектов (0 — плата, разъём + 1), как в ArgbEngine.
fn aura_channels(a: &aura::AuraUsb, o: &AuraOpts) -> Result<(Vec<u8>, Vec<u8>), String> {
    let headers: Vec<u8> = match o.header {
        Some(n) if n > a.argb_headers => return Err(format!("На плате только {} ARGB-разъём(а)", a.argb_headers)),
        Some(n) => vec![(n - 1) as u8],
        None => (0..a.argb_headers as u8).collect(),
    };
    let mut effect: Vec<u8> = Vec::new();
    if o.board {
        effect.push(0);
    }
    effect.extend(headers.iter().map(|h| h + 1));
    Ok((headers, effect))
}

fn on_off(v: Option<&String>) -> Result<u8, String> {
    match v.map(|s| s.to_lowercase()).as_deref() {
        Some("on") | Some("1") | Some("вкл") => Ok(1),
        Some("off") | Some("0") | Some("выкл") => Ok(2),
        _ => Err("Укажите on или off".into()),
    }
}

fn run_mouse(args: &[String]) -> Result<(), String> {
    use rigcontrol_core::mouse::McMouse;
    use rigcontrol_core::mousekeys;
    if args.first().map(|s| s.as_str()) == Some("actions") {
        let mut group = "";
        for x in mousekeys::ACTIONS {
            if x.group != group { group = x.group; println!("{group}:"); }
            println!("  {:<16} {}{}", x.id, x.name, x.hint.map(|h| format!(" ({h})")).unwrap_or_default());
        }
        println!("Также: default — родная функция, key:СОЧЕТАНИЕ — любая клавиша (например key:ctrl+shift+f5)");
        return Ok(());
    }
    let api = hidapi::HidApi::new().map_err(|e| format!("HID: {e}"))?;
    let (found, log) = McMouse::find(&api);
    let sub = args.first().map(|s| s.as_str()).unwrap_or("info");
    let Some((m, status)) = found else {
        return Err(format!("Мышь MCHOSE не найдена.\n{log}"));
    };
    let num = |i: usize, what: &str| -> Result<i64, String> {
        args.get(i).and_then(|v| v.parse::<i64>().ok()).ok_or_else(|| format!("Укажите {what}"))
    };
    let need_cfg = || m.read_config().filter(|c| c.valid).ok_or_else(|| "Не удалось прочитать настройки мыши (0x12 0x67)".to_string());
    let lod_now = || m.read_config().map(|c| c.lod()).unwrap_or(0);

    match sub {
        "info" => {
            let status = m.read_status_fresh().or(status.clone());
            println!("{} · {:04X}:{:04X} 0x{:04X} · {}", m.model(status.as_ref()), m.vendor_id, m.product_id, m.usage_page,
                if m.via_dongle() { "через донгл" } else { "по кабелю" });
            match &status {
                Some(s) => println!("Мышь: {:04X}:{:04X}, связь {}, заряд {}%{} (raw {})", s.vid, s.pid,
                    if s.online { "есть" } else { "нет" }, s.battery, if s.charging() { ", заряжается" } else { "" }, s.raw),
                None => println!("Состояние: нет ответа"),
            }
            if !status.as_ref().map(|s| s.has_mouse()).unwrap_or(false) {
                println!("Подсказка: донгл не получил ответ от мыши. Закройте вкладку M HUB в браузере, пошевелите мышью и повторите.");
                println!("Подробный лог: $env:RIGCONTROL_DEBUG=1; target\\release\\rigcontrol.exe mouse info");
            }
            if let Some(v) = m.read_version(false) { println!("Прошивка мыши: {v}"); }
            if m.via_dongle() { if let Some(v) = m.read_version(true) { println!("Прошивка приёмника: {v}"); } }
            match m.read_config() {
                Some(c) => {
                    let rates = m.rates();
                    let rate = if m.via_dongle() { c.g_rate } else { c.usb_rate } as usize;
                    let level = if m.via_dongle() { c.g_dpi } else { c.usb_dpi } as usize;
                    // У «странного» ответа число уровней может быть больше шести — массивы DPI короче.
                    let levels: Vec<String> = (0..(c.count as usize).min(c.dpi.len())).map(|i| {
                        let mark = if i == level { "*" } else { "" };
                        if c.dpi_y[i] != c.dpi[i] { format!("{mark}{}x{}", c.dpi[i], c.dpi_y[i]) } else { format!("{mark}{}", c.dpi[i]) }
                    }).collect();
                    println!("Профиль {} · DPI: {} (макс. {})", c.profile + 1, levels.join(", "), m.max_dpi(status.as_ref()));
                    println!("Частота: {} Гц", rates.get(rate).map(|r| r.to_string()).unwrap_or_else(|| format!("индекс {rate}")));
                    let lod = ["0,7", "1", "2"][c.lod() as usize];
                    let mode = ["мощность", "игровой", "супер"][c.mode() as usize];
                    let yn = |b: bool| if b { "вкл" } else { "выкл" };
                    println!("Сенсор: отрыв {lod} мм, режим {mode}, ripple {}, angle snap {}, motion sync {}, поворот {}°",
                        yn(c.ripple()), yn(c.angle_snap()), yn(c.motion_sync()), c.rotate);
                    println!("Антидребезг {} мс · сон {}", c.debounce, if c.sleep == 0 { "выкл".to_string() } else { format!("через {} мин", c.sleep) });
                    if !c.valid { println!("Внимание: настройки выглядят странно (raw {})", c.raw); }
                }
                None => println!("Настройки: нет ответа"),
            }
            println!("Интерфейсы:\n{log}");
        }
        "dpi" => {
            let cfg = need_cfg()?;
            let list: Vec<u16> = args.get(1).ok_or("Укажите уровни: mouse dpi 400,800,1600")?
                .split(',').map(|v| v.trim().parse::<u16>().map_err(|_| format!("Плохое DPI: {v}"))).collect::<Result<_, _>>()?;
            if list.is_empty() || list.len() > 6 { return Err("Нужно от 1 до 6 уровней".into()); }
            let max = m.max_dpi(status.as_ref());
            let mut dpi = cfg.dpi;
            for (i, v) in list.iter().enumerate() { dpi[i] = (*v).clamp(50, max); }
            let sel = match args.iter().position(|a| a == "--select") {
                Some(p) => args.get(p + 1).and_then(|v| v.parse::<usize>().ok()).ok_or("--select N")?.saturating_sub(1),
                None => (if m.via_dongle() { cfg.g_dpi } else { cfg.usb_dpi } as usize).min(list.len() - 1),
            };
            m.write_dpi(&cfg, &dpi, &dpi, list.len(), sel)?;
            println!("Готово: DPI {:?}, активный уровень {}", &dpi[..list.len()], sel + 1);
        }
        "level" => {
            let cfg = need_cfg()?;
            let n = num(1, "уровень: mouse level 2")?;
            if n < 1 || n > cfg.count as i64 { return Err(format!("Уровень 1..{}", cfg.count)); }
            m.write_dpi(&cfg, &cfg.dpi, &cfg.dpi_y, cfg.count as usize, n as usize - 1)?;
            println!("Готово: уровень {n} — {} DPI", cfg.dpi[n as usize - 1]);
        }
        "rate" => {
            let hz = num(1, "частоту: mouse rate 1000")? as u32;
            let rates = m.rates();
            let idx = rates.iter().position(|&r| r == hz).ok_or_else(|| format!("Доступно: {rates:?}"))?;
            m.write_rate(idx as u8, m.via_dongle())?;
            println!("Готово: {hz} Гц ({})", if m.via_dongle() { "радио" } else { "кабель" });
        }
        "lod" => {
            let v = num(1, "высоту: mouse lod 0|1|2")?;
            if !(0..=2).contains(&v) { return Err("lod 0, 1 или 2".into()); }
            m.write_sensor(v as u8, 0, 0, 0, 0, None)?;
            println!("Готово: высота отрыва {} мм", ["0,7", "1", "2"][v as usize]);
        }
        "mode" => {
            let mode = match args.get(1).map(|s| s.as_str()) {
                Some("power") => 1, Some("game") => 2, Some("super") => 3,
                _ => return Err("Режим: power, game или super".into()),
            };
            m.write_sensor(lod_now(), 0, 0, 0, mode, None)?;
            println!("Готово: режим {}", args[1]);
        }
        "ripple" | "angle" | "motion-sync" => {
            let v = on_off(args.get(1))?;
            let lod = lod_now();
            match sub {
                "ripple" => m.write_sensor(lod, v, 0, 0, 0, None)?,
                "angle" => m.write_sensor(lod, 0, v, 0, 0, None)?,
                _ => m.write_sensor(lod, 0, 0, v, 0, None)?,
            }
            println!("Готово: {sub} {}", if v == 1 { "вкл" } else { "выкл" });
        }
        "rotate" => {
            let a = num(1, "угол: mouse rotate -30..30")?.clamp(-30, 30) as i8;
            m.write_sensor(lod_now(), 0, 0, 0, 0, Some(a))?;
            println!("Готово: поворот сенсора {a}°");
        }
        "debounce" => {
            let ms = num(1, "мс: mouse debounce 0..20")?.clamp(0, 20) as u8;
            m.write_debounce(ms)?;
            println!("Готово: антидребезг {ms} мс");
        }
        "sleep" => {
            let min = num(1, "минуты: mouse sleep 5 (0 — не засыпать)")?.clamp(0, 255) as u8;
            m.write_sleep(min)?;
            println!("Готово: {}", if min == 0 { "мышь не будет засыпать".to_string() } else { format!("сон через {min} мин") });
        }
        "profile" => {
            let p = num(1, "профиль: mouse profile 1|2|3")?;
            if !(1..=3).contains(&p) { return Err("Профиль 1, 2 или 3".into()); }
            m.switch_profile(p as u8 - 1)?;
            std::thread::sleep(std::time::Duration::from_millis(300));
            println!("Готово: профиль {p}");
        }
        "reset-keys" => {
            m.reset_keys()?;
            println!("Готово: кнопки как с завода");
        }
        "keys" => {
            let cfg = need_cfg()?;
            println!("Профиль {}:", cfg.profile + 1);
            for b in 0..mousekeys::COUNT {
                println!("  {:<8} {:<18} → {}", mousekeys::BUTTON_IDS[b], mousekeys::TITLES[b], mousekeys::describe(b, cfg.keys[b]));
            }
        }
        "bind" => {
            let button = args.get(1).and_then(|b| mousekeys::button_from_name(b))
                .ok_or("Укажите кнопку: left|middle|right|forward|back")?;
            let key = mousekeys::parse_action(args.get(2).ok_or("Укажите функцию: mouse bind back key:ctrl+c")?)?;
            let key = mousekeys::normalize(button, key);
            // Защита из MainWindow.MouseKeys.cs: нельзя остаться без левого клика. Настройки не прочитались —
            // проверить нечем, поэтому тоже отказываем (а не пишем вслепую): в C# без настроек кнопки не меняются.
            if !args.iter().any(|a| a == "--force") {
                let cfg = need_cfg().map_err(|e| format!("{e}. Без них нельзя проверить, что останется левый клик; добавьте --force, если уверены"))?;
                let any_left = (0..mousekeys::COUNT).any(|b| mousekeys::is_left_click(b, if b == button { key } else { cfg.keys[b] }));
                if !any_left {
                    return Err("После этого ни одна кнопка не будет левым кликом. Добавьте --force, если уверены".into());
                }
            }
            m.write_key(button as u8, key)?;
            std::thread::sleep(std::time::Duration::from_millis(150));
            println!("Готово: {} → {}", mousekeys::TITLES[button], mousekeys::describe(button, key));
        }
        "factory-reset" => {
            if !args.iter().any(|a| a == "--yes") {
                return Err("Это сбросит DPI, частоту, сенсор и кнопки текущего профиля. Повторите с --yes".into());
            }
            m.factory_reset()?;
            std::thread::sleep(std::time::Duration::from_millis(500));
            println!("Готово: профиль сброшен к заводским настройкам");
        }
        other => return Err(format!("Неизвестная команда mouse {other}. См. rigcontrol help")),
    }
    Ok(())
}

fn run_aura(args: &[String]) -> Result<(), String> {
    let sub = args.first().map(|s| s.as_str()).unwrap_or("info");
    let api = hidapi::HidApi::new().map_err(|e| format!("hidapi: {e}"))?;
    let a = aura::AuraUsb::find(&api)?;
    match sub {
        "info" => println!("{}", a.description()),
        "color" | "effect" | "off" => {
            let (mode, color, rest) = match sub {
                "off" => (0u8, (0, 0, 0), &args[1.min(args.len())..]),
                "color" => {
                    let hex = args.get(1).ok_or("Укажите цвет: rigcontrol aura color FF0000")?;
                    (1u8, protocol::parse_color(Some(hex), (255, 255, 255)), &args[2..])
                }
                _ => {
                    let name = args.get(1).ok_or("Укажите эффект: static, breathing, flash, cycle, rainbow")?;
                    let mode = aura::EFFECTS.iter().find(|(n, _)| n == name).map(|(_, m)| *m)
                        .or_else(|| name.parse().ok())
                        .ok_or_else(|| format!("Неизвестный эффект {name}. Есть: static, breathing, flash, cycle, rainbow"))?;
                    let (color, skip) = match args.get(2) {
                        Some(c) if !c.starts_with("--") => (protocol::parse_color(Some(c), (255, 255, 255)), 3),
                        _ => ((255, 255, 255), 2),
                    };
                    (mode, color, &args[skip.min(args.len())..])
                }
            };
            let o = parse_aura_opts(rest)?;
            let (_, effect) = aura_channels(&a, &o)?;
            a.set_effect(&effect, mode, color)?;
            println!("Готово: эффект {mode}, цвет {:02X}{:02X}{:02X}, каналы эффектов {:?}", color.0, color.1, color.2, effect);
        }
        "fill" | "rainbow" => {
            let (color, rest) = if sub == "fill" {
                let hex = args.get(1).ok_or("Укажите цвет: rigcontrol aura fill FF0000")?;
                (protocol::parse_color(Some(hex), (255, 255, 255)), &args[2..])
            } else {
                ((0, 0, 0), &args[1.min(args.len())..])
            };
            let o = parse_aura_opts(rest)?;
            let (headers, effect) = aura_channels(&a, &o)?;
            a.enter_direct(&effect)?;
            let leds = o.leds;
            let mut rgb = vec![0u8; leds * 3];
            let send = |rgb: &[u8]| -> Result<(), String> {
                for &h in &headers {
                    a.send_direct(h, rgb, leds)?;
                }
                if o.board && a.mainboard_leds > 0 {
                    a.send_direct(aura::FIXED_DIRECT_CHANNEL, rgb, leds.min(a.mainboard_leds))?;
                }
                Ok(())
            };
            if sub == "fill" {
                for px in rgb.chunks_mut(3) {
                    px.copy_from_slice(&[color.0, color.1, color.2]);
                }
                send(&rgb)?;
                println!("Готово: {leds} светодиодов, разъёмы {:?}. Прямой режим держится до смены эффекта или перезагрузки.", headers.iter().map(|h| h + 1).collect::<Vec<_>>());
            } else {
                println!("Радуга на {leds} светодиодах, разъёмы {:?}. Ctrl+C — стоп.", headers.iter().map(|h| h + 1).collect::<Vec<_>>());
                let start = std::time::Instant::now();
                let mut frames = 0u64;
                loop {
                    let t = start.elapsed().as_secs_f64();
                    if o.seconds.map(|s| t >= s).unwrap_or(false) {
                        break;
                    }
                    for (i, px) in rgb.chunks_mut(3).enumerate() {
                        let c = aura::hue(i as f64 / leds as f64 + t * 0.25);
                        px.copy_from_slice(&[c.0, c.1, c.2]);
                    }
                    send(&rgb)?;
                    frames += 1;
                    std::thread::sleep(std::time::Duration::from_millis(33));
                }
                println!("Кадров: {frames} за {:.1} с.", start.elapsed().as_secs_f64());
            }
        }
        other => return Err(format!("Неизвестная команда aura: {other}. Есть: info, color, effect, off, fill, rainbow")),
    }
    Ok(())
}

fn run_argb(args: &[String]) -> Result<(), String> {
    use rigcontrol_core::argb::{self, ArgbConfig, ArgbMode, Renderer};
    let name = args.first().map(|s| s.as_str()).unwrap_or("list");
    if name == "list" {
        println!("Эффекты ARGB:");
        for (n, _, d) in argb::MODES {
            println!("  {n:<10} {d}");
        }
        return Ok(());
    }
    let mode = ArgbMode::parse(name).ok_or_else(|| format!("Нет такого эффекта: {name}. Список: rigcontrol argb list"))?;
    let mut c = ArgbConfig { mode, ..Default::default() };
    let mut rest = &args[1..];
    if let Some(first) = rest.first() {
        if !first.starts_with("--") {
            c.color = protocol::parse_color(Some(first), c.color);
            rest = &rest[1..];
        }
    }
    // Свои опции разбираем здесь, общие для AURA (--header, --board, --leds, --seconds) — в parse_aura_opts.
    let mut temp = f64::NAN;
    let mut keyboard = false;
    let mut aura_args: Vec<String> = Vec::new();
    let mut i = 0;
    let num = |v: Option<&String>, what: &str| -> Result<f64, String> {
        v.and_then(|v| v.parse::<f64>().ok()).ok_or_else(|| format!("{what}: нужно число"))
    };
    while i < rest.len() {
        let a = rest[i].as_str();
        let v = rest.get(i + 1);
        match a {
            "--color2" => { c.color2 = Some(protocol::parse_color(v.map(|s| s.as_str()), c.color)); i += 1; }
            "--color3" => { c.color3 = Some(protocol::parse_color(v.map(|s| s.as_str()), c.color)); i += 1; }
            "--brightness" => { c.brightness = num(v, a)?.clamp(0.0, 100.0) as i32; i += 1; }
            "--speed" => { c.speed = num(v, a)?.clamp(1.0, 5.0) as i32; i += 1; }
            "--fans" => { c.fans = num(v, a)?.clamp(1.0, 6.0) as usize; i += 1; }
            "--effect" => { c.keyboard_effect = num(v, a)? as i32; i += 1; }
            "--kspeed" => { c.keyboard_speed = num(v, a)?.clamp(0.0, 5.0) as i32; i += 1; }
            "--dir" => { c.keyboard_dir = num(v, a)?.clamp(0.0, 3.0) as i32; i += 1; }
            "--temp" => { temp = num(v, a)?; i += 1; }
            "--keyboard" => keyboard = true,
            "--header" | "--leds" | "--seconds" => {
                aura_args.push(a.to_string());
                if let Some(v) = v { aura_args.push(v.clone()); }
                i += 1;
            }
            _ => aura_args.push(a.to_string()),
        }
        i += 1;
    }
    if mode == ArgbMode::Keyboard && !effects::is_rainbow_flow(c.keyboard_effect) {
        return Err("sync: --effect должен быть 106 (спектр), 107 (волна от центра), 108 (течение) или 109 (вращение)".into());
    }
    if keyboard && mode != ArgbMode::Keyboard {
        return Err("--keyboard работает только с эффектом sync".into());
    }
    let o = parse_aura_opts(&aura_args)?;
    c.leds = o.leds;

    let api = hidapi::HidApi::new().map_err(|e| format!("hidapi: {e}"))?;
    let a = aura::AuraUsb::find(&api)?;
    let (headers, effect) = aura_channels(&a, &o)?;
    // Клавиатура по кабелю — только для sync --keyboard.
    let mut kb = if keyboard { Some(Ak820Device::open(&api)?) } else { None };
    a.enter_direct(&effect)?;

    let leds = c.leds;
    let mut frame = vec![0u8; leds * 3];
    let mut send = vec![0u8; leds * 3];
    let mut last = vec![1u8; leds * 3];
    let mut r = Renderer::new();
    let start = Instant::now();
    let mut last_sent = Instant::now();
    let mut frames = 0u64;
    let mut kb_skipped = 0u32;
    let _timer = timing::HighResTimer::begin();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    // Клавиатура — тем же эффектом и по тем же часам `start`, 20 кадров/с, в своём потоке:
    // отправка её кадра занимает ~45 мс и раньше тормозила цикл ARGB (12 мс).
    let kb_thread = kb.take().map(|mut dev| {
        let stop = stop.clone();
        let (effect, speed, dir, color) = (c.keyboard_effect, c.keyboard_speed, c.keyboard_dir, c.color);
        std::thread::spawn(move || -> Result<u32, String> {
            let mut pacer = timing::Pacer::new(Duration::from_millis(effects::FRAME_INTERVAL_MS));
            let mut skipped = 0u32;
            loop {
                let t = start.elapsed().as_secs_f64();
                let bank = effects::build(effect, t, speed, dir, color);
                if !dev.send_live_frame(&protocol::live_frame(&bank))? {
                    skipped += 1;
                }
                if stop.load(std::sync::atomic::Ordering::Relaxed) {
                    return Ok(skipped);
                }
                pacer.wait();
            }
        })
    });
    let mut pacer = timing::Pacer::new(Duration::from_millis(argb::FRAME_MS));
    let ports: Vec<u8> = headers.iter().map(|h| h + 1).collect();
    if mode.animated() {
        println!("ARGB: {name} на {leds} светодиодах, разъёмы {ports:?}{}. Ctrl+C — стоп.",
            if kb_thread.is_some() { " + клавиатура" } else { "" });
    }
    loop {
        let t = start.elapsed().as_secs_f64();
        if o.seconds.map(|s| t >= s).unwrap_or(false) {
            break;
        }
        r.render(&c, t, temp, &mut frame);
        argb::output(&frame, c.brightness, &mut send);
        // Неизменный кадр повторяем раз в секунду: если плату перехватили (сон, Armoury Crate) — вернётся.
        if send != last || last_sent.elapsed() > Duration::from_secs(1) || frames == 0 {
            for &h in &headers {
                a.send_direct(h, &send, leds)?;
            }
            if o.board && a.mainboard_leds > 0 {
                a.send_direct(aura::FIXED_DIRECT_CHANNEL, &send, leds.min(a.mainboard_leds))?;
            }
            last.copy_from_slice(&send);
            last_sent = Instant::now();
            frames += 1;
        }
        if !mode.animated() {
            break;
        }
        pacer.wait();
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    if let Some(h) = kb_thread {
        kb_skipped = h.join().map_err(|_| "Поток клавиатуры упал".to_string())??;
    }
    if mode.animated() {
        println!("Кадров: {frames} за {:.1} с.", start.elapsed().as_secs_f64());
        if kb_skipped > 0 {
            println!("Пропущено кадров клавиатуры (занята): {kb_skipped}");
        }
    } else {
        println!("Готово: {name}, {leds} светодиодов, разъёмы {ports:?}. Прямой режим держится до смены эффекта или перезагрузки.");
    }
    Ok(())
}

fn now_parts() -> DateTimeParts {
    let now = Local::now();
    DateTimeParts {
        year: now.year(),
        month: now.month() as u8,
        day: now.day() as u8,
        hour: now.hour() as u8,
        minute: now.minute() as u8,
        second: now.second() as u8,
        weekday: now.weekday().num_days_from_sunday() as u8,
    }
}

fn run_dongle(args: &[String]) -> Result<(), String> {
    use rigcontrol_core::dongle::Ak820Dongle;
    let debug = args.iter().any(|a| a == "--debug");
    // --iface N — какой из двух каналов 0xFF60 донгла брать (по умолчанию первый)
    let mut iface: Option<i32> = None;
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--debug" => {}
            "--iface" => {
                iface = Some(args.get(i + 1).and_then(|v| v.parse().ok()).ok_or("--iface: нужен номер интерфейса (см. dongle info)")?);
                i += 1;
            }
            _ => rest.push(args[i].clone()),
        }
        i += 1;
    }
    let args = &rest[..];
    let sub = args.first().map(|s| s.as_str()).unwrap_or("info");
    let api = hidapi::HidApi::new().map_err(|e| format!("hidapi: {e}"))?;
    let mut d = Ak820Dongle::open_interface(&api, iface)?;
    d.debug = debug;
    match sub {
        "info" => {
            println!("Найден: {}", d.description);
            for i in api.device_list().filter(|i| i.vendor_id() == 0x0C45) {
                println!("  интерфейс {:04X}:{:04X} usage page 0x{:04X} usage 0x{:04X} #{}",
                    i.vendor_id(), i.product_id(), i.usage_page(), i.usage(), i.interface_number());
            }
        }
        "listen" => {
            let secs = args.get(1).and_then(|v| v.parse().ok()).unwrap_or(10);
            println!("Слушаю донгл {secs} с — понажимайте клавиши и крутите ручку…");
            let n = d.listen(secs);
            println!("Получено отчётов: {n}");
        }
        "battery" => {
            println!("Спрашиваю заряд ({}), до 6 с…", d.description);
            let level = d.read_battery()?;
            let level = level;
            match level {
                Some(level) => println!("Заряд клавиатуры: {level} % ({})", d.description),
                None => println!("Донгл отвечает, но заряда в ответе нет. Если клавиатура заряжается по кабелю — отключите его; иначе пришлите вывод с --debug."),
            }
        }
        "status" => match d.status()? {
            Some(r) => {
                println!("Ответ донгла на 0x02: {}", rigcontrol_core::dongle::hex(&r));
                if let Some(st) = rigcontrol_core::dongle::parse_status(&r) {
                    println!("  сопряжена клавиатура: {:04X}:{:04X}", st.vendor_id, st.product_id);
                    println!("  неизвестные байты 3–4: 0x{:02X} 0x{:02X} ({} и {}), 11–12: 0x{:02X} 0x{:02X}",
                        st.raw[3], st.raw[4], st.raw[3], st.raw[4], st.raw[11], st.raw[12]);
                }
            }
            None => println!("Донгл не ответил на 0x02."),
        },
        "time" => {
            d.sync_time(now_parts())?;
            println!("Часы синхронизированы по радио: {}", Local::now().format("%d.%m.%Y %H:%M:%S"));
        }
        "off" => {
            d.send_light(0, 0, 0, 0, false, 0, 0, 0)?;
            println!("Подсветка выключена.");
        }
        "effect" => {
            let name = args.get(1).ok_or("Укажите эффект: rigcontrol dongle effect <номер|имя>")?;
            let (title, mode) = resolve_effect(name).ok_or(format!("Нет такого эффекта: {name}. Список: rigcontrol list"))?;
            if effects::is_custom(mode) {
                return Err("Свои эффекты 100–109 рисуются покадрово — по радио это слишком медленно, нужен кабель.".into());
            }
            let o = parse_opts(&args[2..])?;
            let (r, g, b) = protocol::parse_color(Some(&o.color), (255, 255, 255));
            d.send_light(mode, r, g, b, o.rainbow, o.brightness, o.speed, o.dir)?;
            println!("Готово по радио: {title} ({mode}).");
        }
        "paint" => {
            let name = args.get(1).ok_or("Укажите набор: rigcontrol dongle paint <набор>")?;
            let wanted = name.to_lowercase();
            let (title, color_of) = keypaint::PRESETS
                .iter()
                .find(|(n, _)| n.to_lowercase() == wanted)
                .ok_or(format!("Нет такого набора: {name}"))?;
            let o = parse_opts(&args[2..])?;
            let paint: keypaint::Paint = keypaint::KEYS.iter().map(|k| (k.code.to_string(), color_of(k))).collect();
            let frame = keypaint::build_frame(Some(&paint), o.brightness);
            d.set_user_light(&frame)?;
            println!("Раскраска «{title}» записана по радио.");
        }
        other => return Err(format!("Неизвестная команда dongle: {other}. Справка: rigcontrol help")),
    }
    Ok(())
}

fn run(args: Vec<String>) -> Result<(), String> {
    let Some(cmd) = args.first().map(|s| s.as_str()) else {
        println!("{HELP}");
        return Ok(());
    };
    match cmd {
        "help" | "-h" | "--help" => println!("{HELP}"),
        "list" => {
            println!("Эффекты:");
            for (name, mode) in EFFECTS {
                println!("  {mode:>3}  {name}");
            }
            println!("\nНаборы раскраски:");
            for (name, _) in keypaint::PRESETS {
                println!("  {name}");
            }
        }
        "info" => {
            let api = hidapi::HidApi::new().map_err(|e| format!("hidapi: {e}"))?;
            println!("Вендорские HID-интерфейсы:");
            for s in describe_vendor_interfaces(&api) {
                println!("  {s}");
            }
            let dev = Ak820Device::open(&api)?;
            println!("Найдена: {}", dev.description);
        }
        "off" => {
            let (_api, dev) = open()?;
            dev.set_effect(0, "#000000", false, 0, 0, 0)?;
            println!("Подсветка выключена.");
        }
        "effect" => {
            let name = args.get(1).ok_or("Укажите эффект: rigcontrol effect <номер|имя>. Список: rigcontrol list")?;
            let (title, mode) = resolve_effect(name).ok_or(format!("Нет такого эффекта: {name}. Список: rigcontrol list"))?;
            let o = parse_opts(&args[2..])?;
            let (_api, mut dev) = open()?;
            if effects::is_custom(mode) {
                println!("{title} ({mode})");
                run_live(&mut dev, mode, &o)?;
            } else {
                dev.set_effect(mode, &o.color, o.rainbow, o.brightness, o.speed, o.dir)?;
                println!("Готово: {title} ({mode}).");
            }
        }
        "paint" => {
            let name = args.get(1).ok_or("Укажите набор: rigcontrol paint <набор>. Список: rigcontrol list")?;
            let wanted = name.to_lowercase();
            let (title, color_of) = keypaint::PRESETS
                .iter()
                .find(|(n, _)| n.to_lowercase() == wanted)
                .ok_or(format!("Нет такого набора: {name}"))?;
            let o = parse_opts(&args[2..])?;
            let paint: keypaint::Paint = keypaint::KEYS.iter().map(|k| (k.code.to_string(), color_of(k))).collect();
            let frame = keypaint::build_frame(Some(&paint), o.brightness);
            let (_api, dev) = open()?;
            dev.set_user_light(&frame)?;
            println!("Раскраска «{title}» записана в клавиатуру.");
        }
        "time" => {
            let now = Local::now();
            let parts = DateTimeParts {
                year: now.year(),
                month: now.month() as u8,
                day: now.day() as u8,
                hour: now.hour() as u8,
                minute: now.minute() as u8,
                second: now.second() as u8,
                weekday: now.weekday().num_days_from_sunday() as u8,
            };
            // По кабелю, а если кабеля нет — сами через донгл 2,4 ГГц.
            let api = hidapi::HidApi::new().map_err(|e| format!("hidapi: {e}"))?;
            match Ak820Device::open(&api) {
                Ok(dev) => {
                    dev.sync_time(parts)?;
                    println!("Часы синхронизированы по кабелю: {}", now.format("%d.%m.%Y %H:%M:%S"));
                }
                Err(e) if rigcontrol_core::dongle::Ak820Dongle::present(&api) => {
                    let _ = e;
                    let mut d = rigcontrol_core::dongle::Ak820Dongle::open(&api)?;
                    d.sync_time(parts)?;
                    println!("Часы синхронизированы по радио: {}", now.format("%d.%m.%Y %H:%M:%S"));
                }
                Err(e) => return Err(e),
            }
        }
        "pump" => run_pump(&args[1..])?,
        "settings" => run_settings(&args[1..])?,
        "daemon" => run_daemon(&args[1..])?,
        "autostart" => run_autostart(&args[1..])?,
        "screen" => run_screen(&args[1..])?,
        "aura" => run_aura(&args[1..])?,
        "argb" => run_argb(&args[1..])?,
        "mouse" => run_mouse(&args[1..])?,
        "dongle" => run_dongle(&args[1..])?,
        other => return Err(format!("Неизвестная команда: {other}. Справка: rigcontrol help")),
    }
    Ok(())
}

// ---- v12: настройки, профили, автозапуск ----

fn run_settings(args: &[String]) -> Result<(), String> {
    use rigcontrol_core::settings::{self, Settings};
    match args.first().map(|s| s.as_str()).unwrap_or("show") {
        "show" => println!("{}", Settings::load().to_json()),
        "path" => println!("{}", settings::file_path().display()),
        "set" => {
            let key = args.get(1).ok_or("Использование: rigcontrol settings set ИМЯ ЗНАЧЕНИЕ")?;
            let val = args.get(2).ok_or("Использование: rigcontrol settings set ИМЯ ЗНАЧЕНИЕ")?;
            let mut s = Settings::load();
            s.set_field(key, val)?;
            s.save()?;
            println!("Сохранено: {key} = {val}");
        }
        "reset" => {
            if !args.iter().any(|a| a == "--yes") {
                return Err("Сброс удалит все настройки и профили. Повторите с --yes".into());
            }
            Settings::default().save()?;
            println!("Настройки сброшены.");
        }
        other => return Err(format!("Неизвестная команда settings {other}. Есть: show, path, set, reset")),
    }
    Ok(())
}

fn run_daemon(args: &[String]) -> Result<(), String> {
    use rigcontrol_core::{fans::FanControl, kb_light::KbLight, logfile, service::Service, settings::{self, Settings}};
    let seconds: Option<f64> = args.iter().position(|a| a == "--seconds")
        .and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok());
    let keyboard = !args.iter().any(|a| a == "--no-keyboard");
    let with_fans = !args.iter().any(|a| a == "--no-fans");
    logfile::cleanup_on_startup();
    let s = Settings::load();
    // Как окно и фоновая служба (host::Devices::start): подгонка скорости эффектов из настроек —
    // иначе вентиляторы «как клавиатура» шли бы не в такт — и переключатель «Автовывод на помпу».
    rigcontrol_core::light_mirror::set_rates(s.effect_rates.as_ref());
    // Клавиатура — своим потоком: свои эффекты кадрами по кабелю, слежение за кабелем.
    let svc = Service::start_with(&s, false, s.auto_output);
    let kb = keyboard.then(|| KbLight::start(&s));
    // Вентиляторы — как в окне: режимы и кривые из settings.json. Нужны права администратора;
    // процесс завершился (Ctrl+C, сбой) — помощник датчиков сам отдаёт вентиляторы плате.
    let fans = with_fans.then(|| FanControl::start(&s));
    println!("Фоновый режим по {}. Ctrl+C — стоп.", settings::file_path().display());
    let start = Instant::now();
    let mut last: Vec<(String, String)> = Vec::new();
    loop {
        let mut st = svc.status();
        // Диагностика помпы (кадр/с) меняется постоянно — в консоль её не печатаем.
        st.retain(|x| !x.0.contains('·'));
        if let Some(k) = &kb {
            let ks = k.state();
            let text = match &ks.link {
                None => "поиск…".to_string(),
                Some(Ok(how)) => {
                    let mut t = format!("«{}» ({how})", rigcontrol_core::protocol::effect_name(s.effect));
                    if ks.live {
                        t += " · кадры";
                    }
                    if let Some(n) = &ks.note {
                        t += &format!(" · {n}");
                    }
                    t
                }
                Some(Err(e)) => format!("не найдена: {e}"),
            };
            st.insert(0, ("Клавиатура".to_string(), text));
        }
        if let Some(f) = &fans {
            st.push(("Вентиляторы".to_string(), f.state().summary()));
        }
        if st != last {
            for (k, v) in &st {
                if !last.contains(&(k.clone(), v.clone())) {
                    println!("{k}: {v}");
                }
            }
            last = st;
        }
        if seconds.map(|x| start.elapsed().as_secs_f64() >= x).unwrap_or(false) { break; }
        std::thread::sleep(Duration::from_millis(500));
    }
    drop(fans);
    drop(kb);
    // Поток вентиляторов отдаёт каналы плате за секунду после остановки.
    std::thread::sleep(Duration::from_millis(1500));
    svc.stop();
    Ok(())
}

fn run_autostart(args: &[String]) -> Result<(), String> {
    use rigcontrol_core::autostart;
    match args.first().map(|s| s.as_str()).unwrap_or("status") {
        "status" => println!("Автозапуск: {}", if autostart::is_enabled() { "включён" } else { "выключен" }),
        "on" => println!("{}", autostart::set(true, &args[1..].join(" "))?),
        "off" => println!("{}", autostart::set(false, "")?),
        other => return Err(format!("Неизвестная команда autostart {other}. Есть: status, on [АРГУМЕНТЫ], off")),
    }
    Ok(())
}

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Ошибка: {e}");
            ExitCode::FAILURE
        }
    }
}
