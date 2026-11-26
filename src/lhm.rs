//! Датчики через LibreHardwareMonitor — как в C#-версии (Cooler/Telemetry.cs): температура, мощность,
//! напряжение и частоты CPU, видеокарта (в том числе AMD), вентиляторы платы и управление ими.
//!
//! LibreHardwareMonitor — библиотека .NET, поэтому она работает в отдельном процессе-помощнике
//! `sensors\rigcontrol-sensors.exe` рядом с программой (исходник — папка `sensors/`, собирает build.bat).
//! Обмен — по строке JSON через stdin/stdout:
//! `{"cmd":"read","board":true}` → снимок, `{"cmd":"fans","targets":{"<id>": 55 | null}}` → `{"ok":true}`.
//!
//! Помощник один на процесс и общий для всех потоков (окно, служба, «О системе»). Свежий снимок
//! (моложе 700 мс) отдаётся из памяти. Программа вышла или упала — у помощника закрывается stdin,
//! и он сам возвращает вентиляторы плате (BIOS).
//!
//! Нужны права администратора (их даёт манифест rigcontrol-gui.exe, см. build.rs) и драйвер PawnIO.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::Deserialize;

/// Снимок моложе этого берётся из памяти: «О системе», помпа и вентиляторы читают раз в секунду.
const FRESH: Duration = Duration::from_millis(700);
/// После ошибки помощник не перезапускаем столько времени.
const RETRY: Duration = Duration::from_secs(30);
/// Сколько ждём ответа на read/fans. Первый read открывает LibreHardwareMonitor (драйвер, шины) — это секунды.
/// Без предела зависший помощник навсегда держал бы общий замок: вентиляторы замирали на последней
/// скорости, а [`shutdown`] не мог его закрыть.
const REPLY_TIMEOUT: Duration = Duration::from_secs(20);
/// zen (ожидание PCI до 5 с) и specs (WMI) отвечают дольше. Их читает отдельный разовый помощник
/// (см. [`one_shot`]), так что вентиляторы эти секунды не ждут.
const SLOW_REPLY_TIMEOUT: Duration = Duration::from_secs(60);
/// Помощник не ответил вовремя: столько ещё ждём, что он дочитает запрос, получит «quit», вернёт
/// вентиляторы плате и выйдет сам. Снимать его сразу нельзя: исходный режим вентиляторов (как было в BIOS)
/// LibreHardwareMonitor помнит только в памяти процесса — снятый помощник унёс бы его с собой.
const RETIRE_GRACE: Duration = Duration::from_secs(30);
/// Не ответивший помощник ещё завершается: нового пока не запускаем (два помощника спорили бы за чип).
static RETIRING: AtomicBool = AtomicBool::new(false);
/// Поток, который его дожидается: [`shutdown`] ждёт его конца — нельзя говорить «устройства отданы»
/// (другой процесс запустил бы своего помощника), пока прежний ещё жив.
static RETIRE_JOB: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);

/// Канал вентилятора платы (FanInfo в C#). `id` — идентификатор управления в LibreHardwareMonitor.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Fan {
    pub id: String,
    pub name: String,
    pub rpm: Option<f64>,
    pub percent: Option<f64>,
    /// Скоростью управляет программа (иначе — плата).
    pub controlled: bool,
}

/// Ответ помощника на read.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Reading {
    pub ok: bool,
    pub error: Option<String>,
    /// Помощник запущен с правами администратора.
    pub admin: bool,
    pub sensors_available: bool,
    /// Плата и чип мониторинга, например «ASUS PRIME B650M-K · Nuvoton NCT6799D».
    pub board: Option<String>,
    pub cpu_name: Option<String>,
    pub gpu_name: Option<String>,
    pub cpu_temp: Option<f64>,
    pub cpu_clock_mhz: Option<f64>,
    pub cpu_clock_max_mhz: Option<f64>,
    pub cpu_power_w: Option<f64>,
    pub cpu_vcore: Option<f64>,
    /// Откуда Vcore: «SVI», «плата» или «VID».
    pub cpu_vcore_source: Option<String>,
    pub gpu_load: Option<f64>,
    pub gpu_temp: Option<f64>,
    pub gpu_hot_spot: Option<f64>,
    pub gpu_mem_temp: Option<f64>,
    pub gpu_power_w: Option<f64>,
    pub gpu_core_volt: Option<f64>,
    pub gpu_core_clock_mhz: Option<f64>,
    pub gpu_mem_clock_mhz: Option<f64>,
    pub gpu_fan_rpm: Option<f64>,
    pub vram_used_gb: Option<f64>,
    pub vram_total_gb: Option<f64>,
    /// Вентиляторы платы; None — плату не опрашивали.
    pub fans: Option<Vec<Fan>>,
}

/// Почему нет температуры CPU — для подсказки в окне. None — всё читается.
pub fn note(r: &Reading) -> Option<String> {
    if r.cpu_temp.is_some() {
        return None;
    }
    Some(if r.admin {
        "LibreHardwareMonitor не отдаёт температуру CPU: нужен драйвер PawnIO (pawnio.eu)".to_string()
    } else {
        "нет прав администратора: запустите RigControl от имени администратора".to_string()
    })
}

struct Helper {
    child: Child,
    stdin: ChildStdin,
    /// Строки ответов: stdout читает отдельный поток, чтобы ждать ответ не дольше заданного.
    replies: Receiver<std::io::Result<String>>,
    /// Последний вызов кончился тайм-аутом: помощник, возможно, жив и просто занят.
    timed_out: bool,
}

fn helper_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("RIGCONTROL_SENSORS") {
        return Some(PathBuf::from(p));
    }
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.join("sensors").join("rigcontrol-sensors.exe"))
}

impl Helper {
    fn spawn() -> Result<Self, String> {
        let path = helper_path().ok_or("не найдена папка программы")?;
        Self::spawn_at(&path)
    }

    fn spawn_at(path: &std::path::Path) -> Result<Self, String> {
        if !path.exists() {
            return Err(format!(
                "нет помощника датчиков {} — соберите build.bat (нужен .NET 8 SDK)",
                path.display()
            ));
        }
        Self::spawn_command(Command::new(path), &path.display().to_string())
    }

    fn spawn_command(mut cmd: Command, name: &str) -> Result<Self, String> {
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW — без окна консоли
        }
        let mut child = cmd.spawn().map_err(|e| format!("{name}: {e}"))?;
        let stdin = child.stdin.take().ok_or("нет stdin помощника датчиков")?;
        let stdout = child.stdout.take().ok_or("нет stdout помощника датчиков")?;
        let (tx, replies) = mpsc::channel();
        let reader = std::thread::Builder::new().name("sensors-reply".into()).spawn(move || {
            let mut stdout = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                let r = stdout.read_line(&mut line);
                let end = !matches!(r, Ok(n) if n > 0);
                // Ok(пустая строка) — помощник закрыл stdout.
                if tx.send(r.map(|_| line)).is_err() || end {
                    return;
                }
            }
        });
        if let Err(e) = reader {
            let _ = child.kill();
            return Err(format!("помощник датчиков: {e}"));
        }
        Ok(Self { child, stdin, replies, timed_out: false })
    }

    fn call(&mut self, request: &str, timeout: Duration) -> Result<String, String> {
        let send = |w: &mut ChildStdin| -> std::io::Result<()> {
            w.write_all(request.as_bytes())?;
            w.write_all(b"\n")?;
            w.flush()
        };
        send(&mut self.stdin).map_err(|e| format!("помощник датчиков: {e}"))?;
        match self.replies.recv_timeout(timeout) {
            Ok(Ok(line)) if !line.is_empty() => Ok(line),
            Ok(Ok(_)) | Err(RecvTimeoutError::Disconnected) => Err("помощник датчиков закрылся".to_string()),
            Ok(Err(e)) => Err(format!("помощник датчиков: {e}")),
            // Вызывающий код отправит помощника на покой (см. `call` ниже): ответ мог бы прийти позже и сбить протокол.
            Err(RecvTimeoutError::Timeout) => {
                self.timed_out = true;
                Err(format!("помощник датчиков не ответил за {} с", timeout.as_secs()))
            }
        }
    }

    /// Вежливо закрыть: «quit» (помощник вернёт вентиляторы плате и выйдет), ждать до grace, потом снять.
    /// true — вышел сам.
    fn close(&mut self, grace: Duration) -> bool {
        let _ = self.stdin.write_all(b"{\"cmd\":\"quit\"}\n");
        let _ = self.stdin.flush();
        let until = Instant::now() + grace;
        loop {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return true;
            }
            if Instant::now() >= until {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        false
    }
}

impl Drop for Helper {
    fn drop(&mut self) {
        // Вежливо: помощник вернёт вентиляторы плате и выйдет. Не вышел за 2 с — снимаем.
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            self.close(Duration::from_secs(2));
        }
    }
}

struct Shared {
    helper: Option<Helper>,
    retry_at: Option<Instant>,
    error: Option<String>,
    last: Option<(Instant, bool, Reading)>,
}

static SHARED: Mutex<Shared> = Mutex::new(Shared { helper: None, retry_at: None, error: None, last: None });

fn lock() -> MutexGuard<'static, Shared> {
    SHARED.lock().unwrap_or_else(|e| e.into_inner())
}

fn call(s: &mut Shared, request: &str, timeout: Duration) -> Result<String, String> {
    if s.helper.is_none() {
        if RETIRING.load(Ordering::SeqCst) {
            return Err("помощник датчиков не ответил вовремя и завершается — новый запустится после него".to_string());
        }
        if s.retry_at.is_some_and(|t| Instant::now() < t) {
            return Err(s.error.clone().unwrap_or_else(|| "помощник датчиков недоступен".to_string()));
        }
        match Helper::spawn() {
            Ok(h) => s.helper = Some(h),
            Err(e) => {
                s.retry_at = Some(Instant::now() + RETRY);
                s.error = Some(e.clone());
                return Err(e);
            }
        }
    }
    let result = match s.helper.as_mut() {
        Some(h) => h.call(request, timeout),
        None => Err("помощник датчиков недоступен".to_string()),
    };
    if let Err(e) = &result {
        match s.helper.take() {
            // Не ответил вовремя — даём дочитать и вернуть вентиляторы плате (в фоне: замок не держим).
            Some(h) if h.timed_out => retire(h),
            // Закрылся или сломался канал — обычное закрытие.
            other => drop(other),
        }
        s.last = None;
        s.retry_at = Some(Instant::now() + RETRY);
        s.error = Some(e.clone());
    }
    result
}

/// Отправить не ответившего помощника на покой в фоновом потоке (см. [`RETIRE_GRACE`]).
fn retire(mut helper: Helper) {
    RETIRING.store(true, Ordering::SeqCst);
    let spawned = std::thread::Builder::new().name("sensors-retire".into()).spawn(move || {
        helper.close(RETIRE_GRACE);
        RETIRING.store(false, Ordering::SeqCst);
    });
    match spawned {
        Ok(job) => *RETIRE_JOB.lock().unwrap_or_else(|e| e.into_inner()) = Some(job),
        // Поток не создался: помощник уже закрыт обычным образом (Drop замыкания).
        Err(_) => RETIRING.store(false, Ordering::SeqCst),
    }
}

/// Дождаться, пока не ответивший помощник выйдет или будет снят (не дольше [`RETIRE_GRACE`] и пары секунд).
fn wait_retired() {
    let job = RETIRE_JOB.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(job) = job {
        let _ = job.join();
    }
}

/// Разовый помощник для долгих справочных запросов (zen, specs): отдельный процесс, который не открывает
/// LibreHardwareMonitor и не трогает вентиляторы. Основной помощник и общий замок при этом свободны —
/// вентиляторы продолжают регулироваться, пока читаются тайминги и WMI (в C# это тоже отдельная задача).
fn one_shot(request: &str) -> Result<String, String> {
    let mut helper = Helper::spawn()?;
    helper.call(request, SLOW_REPLY_TIMEOUT)
}

/// Закрыть помощника (он сам вернёт вентиляторы плате) и пару секунд не запускать его снова —
/// чтобы окно и служба не держали двух помощников сразу. Ждёт, пока помощник выйдет (до 2 с;
/// если он перед этим не ответил вовремя и ещё завершается — пока не завершится).
pub fn shutdown() {
    let helper = {
        let mut s = lock();
        s.last = None;
        s.retry_at = Some(Instant::now() + Duration::from_secs(3));
        s.error = Some("помощник датчиков остановлен".to_string());
        s.helper.take()
    };
    drop(helper);
    wait_retired();
}

/// Снова можно запускать помощника (после [`shutdown`]).
pub fn reset() {
    let mut s = lock();
    if s.helper.is_none() {
        s.retry_at = None;
        s.error = None;
    }
}

/// Снимок датчиков. board = true — с вентиляторами платы (их опрос дороже).
pub fn read(board: bool) -> Result<Reading, String> {
    let mut s = lock();
    if let Some((at, with_board, r)) = &s.last {
        if at.elapsed() < FRESH && (*with_board || !board) {
            return Ok(r.clone());
        }
    }
    let request = if board { r#"{"cmd":"read","board":true}"# } else { r#"{"cmd":"read","board":false}"# };
    let line = call(&mut s, request, REPLY_TIMEOUT)?;
    let r: Reading = serde_json::from_str(line.trim()).map_err(|e| format!("помощник датчиков: {e}"))?;
    if !r.ok {
        let e = r.error.unwrap_or_else(|| "LibreHardwareMonitor не открылся".to_string());
        s.error = Some(e.clone());
        return Err(e);
    }
    s.error = None;
    s.last = Some((Instant::now(), board, r.clone()));
    Ok(r)
}

/// Тайминги памяти одного канала (DramTimings в ZenTuning.cs), в тактах MEMCLK.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct DramTimings {
    pub data_rate: i32,
    pub mem_clk_mhz: f64,
    #[serde(rename = "cmd2T")]
    pub cmd2t: bool,
    pub gdm: bool,
    pub power_down: bool,
    pub cl: u32,
    pub rcd_rd: u32,
    pub rcd_wr: u32,
    pub rp: u32,
    pub ras: u32,
    pub rc: u32,
    pub rrds: u32,
    pub rrdl: u32,
    pub faw: u32,
    pub wtrs: u32,
    pub wtrl: u32,
    pub wr: u32,
    pub rtp: u32,
    pub cwl: u32,
    pub rfc: u32,
    pub rfc2: u32,
    pub rfc_sb: u32,
    pub refi: u32,
}

impl DramTimings {
    /// Такты → наносекунды: у DDR5 MEMCLK вдвое ниже скорости обмена.
    pub fn ns(&self, clocks: u32) -> f64 {
        if self.data_rate > 0 { f64::from(clocks) * 2000.0 / f64::from(self.data_rate) } else { 0.0 }
    }
}

/// Curve Optimizer по ядрам (в порядке ядер Windows) и тайминги (ZenReport). None — не прочиталось, см. заметку.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ZenReport {
    pub curve_optimizer: Option<Vec<i32>>,
    pub co_note: Option<String>,
    pub timings: Option<DramTimings>,
    pub timings_note: Option<String>,
}

/// Строка «Характеристики»: название, значение, подробности мелким шрифтом (SpecRow).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct SpecRow {
    pub label: String,
    pub value: String,
    pub detail: Option<String>,
}

/// Паспорт процессора (CpuSpec в SystemInfo.cs).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct CpuSpec {
    pub summary: String,
    pub base_mhz: f64,
    pub boost_mhz: f64,
    pub ppt_w: f64,
    pub tj_max_c: f64,
    pub tips: String,
}

/// Паспорт видеокарты (GpuSpec): max_volt_mv — верхняя точка стоковой кривой напряжения.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct GpuSpec {
    pub summary: String,
    pub boost_mhz: f64,
    pub mem_clock_mhz: f64,
    pub max_volt_mv: f64,
    pub hot_spot_limit_c: f64,
    pub board_power_w: f64,
    pub tips: String,
}

/// Характеристики ПК (SystemSpecs).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct SystemSpecs {
    pub rows: Vec<SpecRow>,
    pub cpu_name: Option<String>,
    pub gpu_name: Option<String>,
    pub cpu: Option<CpuSpec>,
    pub gpu: Option<GpuSpec>,
}

/// Ответ вида {"ok":true,"<key>":{…}} → значение поля key.
fn payload<T: serde::de::DeserializeOwned>(line: &str, key: &str) -> Result<T, String> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).map_err(|e| format!("помощник датчиков: {e}"))?;
    if v.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
        return Err(v.get("error").and_then(serde_json::Value::as_str).unwrap_or("помощник датчиков не ответил").to_string());
    }
    serde_json::from_value(v.get(key).cloned().unwrap_or(serde_json::Value::Null)).map_err(|e| format!("помощник датчиков: {e}"))
}

/// Curve Optimizer и тайминги ОЗУ (только чтение, через PawnIO). Меняются только из BIOS — читать редко.
pub fn zen() -> Result<ZenReport, String> {
    payload(&one_shot(r#"{"cmd":"zen"}"#)?, "zen")
}

/// Характеристики ПК из реестра и WMI и паспорт известной модели (это сотни миллисекунд — читать один раз).
pub fn specs() -> Result<SystemSpecs, String> {
    payload(&one_shot(r#"{"cmd":"specs"}"#)?, "specs")
}

/// Скорость вентиляторов: id → % (20–100) или None — вернуть канал плате.
pub fn apply_fans(targets: &BTreeMap<String, Option<f64>>) -> Result<(), String> {
    let request = serde_json::json!({ "cmd": "fans", "targets": targets }).to_string();
    let mut s = lock();
    let line = call(&mut s, &request, REPLY_TIMEOUT)?;
    let v: serde_json::Value = serde_json::from_str(line.trim()).map_err(|e| format!("помощник датчиков: {e}"))?;
    if v.get("ok").and_then(serde_json::Value::as_bool) == Some(true) {
        Ok(())
    } else {
        Err(v.get("error").and_then(serde_json::Value::as_str).unwrap_or("ошибка управления").to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_helper_reply() {
        let line = r#"{"ok":true,"error":null,"admin":true,"sensorsAvailable":true,"board":"ASUS PRIME · Nuvoton NCT6799D",
            "cpuTemp":61.5,"cpuPowerW":48.2,"cpuVcore":1.1,"cpuVcoreSource":"SVI","gpuHotSpot":70,"vramUsedGb":null,
            "fans":[{"id":"/lpc/nct6799d/0/control/1","name":"Fan #2","rpm":980,"percent":42.5,"controlled":true}]}"#;
        let r: Reading = serde_json::from_str(line).unwrap();
        assert!(r.ok && r.admin);
        assert_eq!(r.cpu_temp, Some(61.5));
        assert_eq!(r.gpu_hot_spot, Some(70.0));
        assert_eq!(r.vram_used_gb, None);
        let fans = r.fans.unwrap();
        assert_eq!(fans[0].id, "/lpc/nct6799d/0/control/1");
        assert_eq!(fans[0].rpm, Some(980.0));
        assert!(fans[0].controlled);
    }

    #[test]
    fn parses_zen_and_specs() {
        let z: ZenReport = payload(r#"{"ok":true,"zen":{"curveOptimizer":[-20,-20,-15,-20,-25,-20],"coNote":null,
            "timings":{"dataRate":6000,"memClkMhz":3000,"cmd2T":false,"gdm":true,"powerDown":false,"cl":30,"rcdRd":36,
            "rcdWr":36,"rp":36,"ras":76,"rc":112,"rfc":884,"refi":11700},"timingsNote":null}}"#, "zen").unwrap();
        assert_eq!(z.curve_optimizer.as_deref(), Some(&[-20, -20, -15, -20, -25, -20][..]));
        let t = z.timings.unwrap();
        assert_eq!((t.data_rate, t.cl, t.rfc), (6000, 30, 884));
        assert!(t.gdm && !t.cmd2t);
        assert!((t.ns(t.rfc) - 294.67).abs() < 0.01);
        let s: SystemSpecs = payload(r#"{"ok":true,"specs":{"rows":[{"label":"Процессор","value":"AMD Ryzen 5 7500F","detail":null}],
            "cpuName":"AMD Ryzen 5 7500F","gpuName":null,"cpu":{"summary":"Zen 4","baseMhz":3700,"boostMhz":5000,"pptW":88,"tjMaxC":95,"tips":""},"gpu":null}}"#, "specs").unwrap();
        assert_eq!(s.rows.len(), 1);
        assert_eq!(s.cpu.unwrap().tj_max_c, 95.0);
        assert!(payload::<SystemSpecs>(r#"{"ok":false,"error":"нет"}"#, "specs").is_err());
    }

    /// Поддельный помощник: команда оболочки (только Unix — на Windows его заменил бы .exe).
    #[cfg(unix)]
    fn fake_helper(body: &str) -> Helper {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(body);
        Helper::spawn_command(cmd, "sh").unwrap()
    }

    #[cfg(unix)]
    #[test]
    fn helper_answers_line_by_line() {
        let mut h = fake_helper(r#"while read line; do echo "{\"ok\":true}"; done"#);
        for _ in 0..3 {
            assert_eq!(h.call(r#"{"cmd":"read"}"#, Duration::from_secs(5)).unwrap().trim(), r#"{"ok":true}"#);
        }
    }

    #[cfg(unix)]
    #[test]
    fn silent_helper_times_out_instead_of_hanging() {
        // Читает запрос и молчит: раньше вызов ждал ответа вечно, держа общий замок.
        let mut h = fake_helper("read line; sleep 30");
        let started = Instant::now();
        let e = h.call(r#"{"cmd":"read"}"#, Duration::from_millis(300)).unwrap_err();
        assert!(e.contains("не ответил"), "{e}");
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[cfg(unix)]
    #[test]
    fn slow_helper_is_allowed_to_finish_and_quit_by_itself() {
        // Занят 1 с, потом читает следующую строку («quit») и выходит сам — как настоящий помощник,
        // который после долгого чтения возвращает вентиляторы плате. Снимать его сразу нельзя.
        let mut h = fake_helper("read first; sleep 1; read second; [ \"$second\" = '{\"cmd\":\"quit\"}' ] && exit 0; exit 3");
        let e = h.call(r#"{"cmd":"read"}"#, Duration::from_millis(200)).unwrap_err();
        assert!(h.timed_out && e.contains("не ответил"), "{e}");
        assert!(h.close(Duration::from_secs(10)), "помощник должен выйти сам, а не быть снятым");
        assert_eq!(h.child.wait().unwrap().code(), Some(0));
    }

    #[cfg(unix)]
    #[test]
    fn shutdown_waits_for_a_retiring_helper() {
        // Единственный тест с общим состоянием модуля (RETIRING / RETIRE_JOB).
        let mut h = fake_helper("read first; sleep 1; read second; exit 0");
        assert!(h.call(r#"{"cmd":"read"}"#, Duration::from_millis(100)).is_err());
        retire(h);
        assert!(RETIRING.load(Ordering::SeqCst));
        wait_retired();
        // Пока помощник не вышел, wait_retired не возвращается — значит, уже вышел.
        assert!(!RETIRING.load(Ordering::SeqCst));
    }

    #[cfg(unix)]
    #[test]
    fn hung_helper_is_killed_after_the_grace_period() {
        let mut h = fake_helper("read first; sleep 30");
        assert!(h.call(r#"{"cmd":"read"}"#, Duration::from_millis(100)).is_err());
        let started = Instant::now();
        assert!(!h.close(Duration::from_millis(300)));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[cfg(unix)]
    #[test]
    fn closed_helper_is_an_error() {
        let mut h = fake_helper("exit 0");
        let e = h.call(r#"{"cmd":"read"}"#, Duration::from_secs(5)).unwrap_err();
        assert!(e.contains("закрылся") || e.contains("помощник датчиков"), "{e}");
    }

    #[test]
    fn notes() {
        let mut r = Reading { admin: false, ..Default::default() };
        assert!(note(&r).unwrap().contains("администратора"));
        r.admin = true;
        assert!(note(&r).unwrap().contains("PawnIO"));
        r.cpu_temp = Some(50.0);
        assert_eq!(note(&r), None);
    }
}
