//! Замена Cooler/Telemetry.cs. Источники:
//! - LibreHardwareMonitor через помощника ([`crate::lhm`]), как в C#: температура, мощность, частоты и
//!   напряжение CPU, видеокарта (NVIDIA, AMD, Intel); нужны права администратора и драйвер PawnIO;
//! - крейт sysinfo: загрузка и частота CPU, память, сеть, диск C:, время работы, температуры (если ОС их отдаёт);
//! - nvidia-smi (если есть видеокарта NVIDIA): загрузка, температура, мощность и память GPU;
//! - ручные значения из командной строки (--cpu-temp и т. п.) перекрывают всё остальное.

use std::process::Command;
use std::time::{Duration, Instant};
use sysinfo::{Components, Disks, Networks, System};

/// Снимок датчиков (поля как у Snapshot в Telemetry.cs).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub cpu_load: f64,
    pub cpu_temp: Option<f64>,
    pub cpu_clock_mhz: Option<f64>,
    pub cpu_power_w: Option<f64>,
    pub gpu_load: Option<f64>,
    pub gpu_temp: Option<f64>,
    pub gpu_power_w: Option<f64>,
    pub vram_used_gb: Option<f64>,
    pub vram_total_gb: Option<f64>,
    pub ram_load: f64,
    pub ram_used_gb: f64,
    pub ram_total_gb: f64,
    pub net_down_bps: f64,
    pub net_up_bps: f64,
    pub disk_used_pct: f64,
    pub disk_free_gb: f64,
    pub uptime_secs: u64,
    /// Напряжение ядер CPU, В (SVI, датчик платы или VID).
    pub cpu_vcore: Option<f64>,
    /// Частота лучшего ядра, МГц.
    pub cpu_clock_max_mhz: Option<f64>,
    pub gpu_hot_spot: Option<f64>,
    pub gpu_core_volt: Option<f64>,
    pub gpu_core_clock_mhz: Option<f64>,
    pub gpu_mem_clock_mhz: Option<f64>,
    /// Имена из LibreHardwareMonitor и откуда Vcore («SVI», «плата», «VID»).
    pub cpu_name: Option<String>,
    pub gpu_name: Option<String>,
    pub cpu_vcore_source: Option<String>,
    pub gpu_mem_temp: Option<f64>,
    pub gpu_fan_rpm: Option<f64>,
    /// Почему нет датчиков LibreHardwareMonitor (нет помощника, нет прав, нет драйвера); None — всё читается.
    pub sensors_note: Option<String>,
}

/// Ручные значения: Some — подставить вместо датчика.
#[derive(Clone, Debug, Default)]
pub struct Overrides {
    pub cpu_temp: Option<f64>,
    pub gpu_temp: Option<f64>,
    pub cpu_power_w: Option<f64>,
}

const GB: f64 = 1024.0 * 1024.0 * 1024.0;

pub struct Telemetry {
    sys: System,
    networks: Networks,
    disks: Disks,
    components: Components,
    last_net: Instant,
    last_net_list: Instant,
    last_disk: Instant,
    last_gpu: Option<Instant>,
    gpu_cache: Option<GpuStats>,
    nvidia_smi: bool,
    pub overrides: Overrides,
    /// Спрашивать LibreHardwareMonitor (через помощника). По умолчанию — да.
    pub use_lhm: bool,
    /// Опрашивать и плату: на её чипе мониторинга — Vcore (нужно странице «О системе», как в C#:
    /// telemetry.Read(sensors, fans || system, system)). Без этого у CPU без SVI показывался бы VID.
    pub read_board: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GpuStats {
    pub load: f64,
    pub temp: f64,
    pub power_w: Option<f64>,
    pub vram_used_gb: f64,
    pub vram_total_gb: f64,
}

/// Разбор строки `nvidia-smi --query-gpu=utilization.gpu,temperature.gpu,power.draw,memory.used,memory.total
/// --format=csv,noheader,nounits` (берётся первая видеокарта). Память в МиБ.
pub fn parse_nvidia_smi(out: &str) -> Option<GpuStats> {
    let line = out.lines().find(|l| !l.trim().is_empty())?;
    let parts: Vec<&str> = line.split(',').map(|p| p.trim()).collect();
    if parts.len() < 5 {
        return None;
    }
    let num = |s: &str| s.parse::<f64>().ok();
    Some(GpuStats {
        load: num(parts[0])?,
        temp: num(parts[1])?,
        power_w: num(parts[2]),
        vram_used_gb: num(parts[3])? / 1024.0,
        vram_total_gb: num(parts[4])? / 1024.0,
    })
}

/// Том Windows («C:»): как Environment.SystemDirectory в C# — Windows стоит не обязательно на C:.
fn system_drive() -> String {
    system_drive_from(std::env::var("SystemDrive").ok().as_deref())
}

fn system_drive_from(var: Option<&str>) -> String {
    let d = var.unwrap_or("").trim().to_uppercase();
    let mut chars = d.chars();
    match (chars.next(), chars.next(), chars.next()) {
        (Some(letter), Some(':'), None) if letter.is_ascii_alphabetic() => d,
        _ => "C:".to_string(),
    }
}

fn query_nvidia() -> Option<GpuStats> {
    let mut cmd = Command::new("nvidia-smi");
    cmd.args([
        "--query-gpu=utilization.gpu,temperature.gpu,power.draw,memory.used,memory.total",
        "--format=csv,noheader,nounits",
    ]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW — без мигающего окна консоли
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    parse_nvidia_smi(&String::from_utf8_lossy(&out.stdout))
}

/// Температура CPU из списка датчиков ОС: Package / Tctl / Tdie / CPU.
fn cpu_temp(components: &Components) -> Option<f64> {
    let keys = ["package", "tctl", "tdie", "cpu", "core"];
    for key in keys {
        for c in components.iter() {
            let label = c.label().to_lowercase();
            let t = c.temperature() as f64;
            if label.contains(key) && t.is_finite() && t > 0.0 && t < 130.0 {
                return Some(t);
            }
        }
    }
    None
}

impl Default for Telemetry {
    fn default() -> Self {
        Self::new()
    }
}

impl Telemetry {
    pub fn new() -> Self {
        let mut sys = System::new();
        sys.refresh_cpu_all();
        sys.refresh_memory();
        Telemetry {
            sys,
            networks: Networks::new_with_refreshed_list(),
            disks: Disks::new_with_refreshed_list(),
            components: Components::new_with_refreshed_list(),
            last_net: Instant::now(),
            last_net_list: Instant::now(),
            last_disk: Instant::now(),
            last_gpu: None,
            gpu_cache: None,
            nvidia_smi: true,
            overrides: Overrides::default(),
            use_lhm: true,
            read_board: false,
        }
    }

    /// Обновить датчики и вернуть снимок. Звать раз в секунду (как таймер в C#):
    /// загрузка CPU и скорость сети считаются по разнице с прошлым вызовом.
    pub fn sample(&mut self) -> Snapshot {
        self.sys.refresh_cpu_all();
        self.sys.refresh_memory();
        self.components.refresh();

        let elapsed = self.last_net.elapsed().as_secs_f64().max(0.001);
        // Список адаптеров перечитываем: Wi-Fi или кабель, подключённые после запуска, иначе
        // не попали бы в счёт (в C# адаптеры перечисляются на каждом чтении).
        if self.last_net_list.elapsed() >= Duration::from_secs(10) {
            self.networks.refresh_list();
            self.last_net_list = Instant::now();
        } else {
            self.networks.refresh();
        }
        self.last_net = Instant::now();
        let (mut down, mut up) = (0u64, 0u64);
        for (_, data) in self.networks.iter() {
            down += data.received();
            up += data.transmitted();
        }

        if self.last_disk.elapsed() > Duration::from_secs(30) {
            self.disks.refresh_list();
            self.last_disk = Instant::now();
        }
        let (mut disk_pct, mut disk_free) = (0.0, 0.0);
        let system_drive = system_drive();
        for d in self.disks.iter() {
            let mount = d.mount_point().to_string_lossy().to_uppercase();
            if mount.starts_with(&system_drive) || (cfg!(not(windows)) && mount == "/") {
                let total = d.total_space() as f64;
                let free = d.available_space() as f64;
                if total > 0.0 {
                    disk_pct = (total - free) / total * 100.0;
                    disk_free = free / GB;
                }
                break;
            }
        }

        // nvidia-smi — отдельный процесс, поэтому не чаще раза в 2 с.
        if self.nvidia_smi && self.last_gpu.map_or(true, |t| t.elapsed() >= Duration::from_secs(2)) {
            self.last_gpu = Some(Instant::now());
            self.gpu_cache = query_nvidia();
            if self.gpu_cache.is_none() {
                self.nvidia_smi = false; // нет NVIDIA — больше не пробуем
            }
        }
        let gpu = self.gpu_cache.clone();

        let (hw, sensors_note) = if self.use_lhm {
            match crate::lhm::read(self.read_board) {
                Ok(r) => {
                    let note = crate::lhm::note(&r);
                    (Some(r), note)
                }
                Err(e) => (None, Some(e)),
            }
        } else {
            (None, None)
        };
        let h = hw.as_ref();

        let total = self.sys.total_memory() as f64;
        let used = self.sys.used_memory() as f64;
        let clock = self.sys.cpus().iter().map(|c| c.frequency() as f64).fold(0.0, f64::max);

        Snapshot {
            cpu_load: self.sys.global_cpu_usage() as f64,
            cpu_temp: self.overrides.cpu_temp
                .or_else(|| h.and_then(|h| h.cpu_temp))
                .or_else(|| cpu_temp(&self.components)),
            cpu_clock_mhz: h.and_then(|h| h.cpu_clock_mhz).or(if clock > 0.0 { Some(clock) } else { None }),
            cpu_power_w: self.overrides.cpu_power_w.or_else(|| h.and_then(|h| h.cpu_power_w)),
            gpu_load: h.and_then(|h| h.gpu_load).or(gpu.as_ref().map(|g| g.load)),
            gpu_temp: self.overrides.gpu_temp
                .or_else(|| h.and_then(|h| h.gpu_temp))
                .or(gpu.as_ref().map(|g| g.temp)),
            gpu_power_w: h.and_then(|h| h.gpu_power_w).or(gpu.as_ref().and_then(|g| g.power_w)),
            vram_used_gb: h.and_then(|h| h.vram_used_gb).or(gpu.as_ref().map(|g| g.vram_used_gb)),
            vram_total_gb: h.and_then(|h| h.vram_total_gb).or(gpu.as_ref().map(|g| g.vram_total_gb)),
            ram_load: if total > 0.0 { used / total * 100.0 } else { 0.0 },
            ram_used_gb: used / GB,
            ram_total_gb: total / GB,
            net_down_bps: down as f64 / elapsed,
            net_up_bps: up as f64 / elapsed,
            disk_used_pct: disk_pct,
            disk_free_gb: disk_free,
            uptime_secs: System::uptime(),
            cpu_vcore: h.and_then(|h| h.cpu_vcore),
            cpu_clock_max_mhz: h.and_then(|h| h.cpu_clock_max_mhz),
            gpu_hot_spot: h.and_then(|h| h.gpu_hot_spot),
            gpu_core_volt: h.and_then(|h| h.gpu_core_volt),
            gpu_core_clock_mhz: h.and_then(|h| h.gpu_core_clock_mhz),
            gpu_mem_clock_mhz: h.and_then(|h| h.gpu_mem_clock_mhz),
            cpu_name: h.and_then(|h| h.cpu_name.clone()),
            gpu_name: h.and_then(|h| h.gpu_name.clone()),
            cpu_vcore_source: h.and_then(|h| h.cpu_vcore_source.clone()),
            gpu_mem_temp: h.and_then(|h| h.gpu_mem_temp),
            gpu_fan_rpm: h.and_then(|h| h.gpu_fan_rpm),
            sensors_note,
        }
    }
}

// ── Живые датчики для окна ───────────────────────────────────────────────────

struct Live {
    snap: Option<Snapshot>,
    at: Option<Instant>,
    wanted: Option<Instant>,
    running: bool,
}

static LIVE: std::sync::Mutex<Live> = std::sync::Mutex::new(Live { snap: None, at: None, wanted: None, running: false });

fn live_lock() -> std::sync::MutexGuard<'static, Live> {
    LIVE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Свежий снимок из потока помпы — им же пользуется предпросмотр в окне.
pub fn publish(s: &Snapshot) {
    let mut l = live_lock();
    l.snap = Some(s.clone());
    l.at = Some(Instant::now());
}

/// Последние показания датчиков для предпросмотра (как на помпе). Если экран помпы сейчас ничего не
/// публикует, сам запускает фоновый опрос раз в секунду; опрос гаснет через 5 с после последнего вызова.
/// None — первое чтение ещё не готово.
pub fn live() -> Option<Snapshot> {
    let mut l = live_lock();
    l.wanted = Some(Instant::now());
    let fresh = l.at.map(|t| t.elapsed() < Duration::from_millis(2500)).unwrap_or(false);
    if !fresh && !l.running {
        l.running = true;
        std::thread::spawn(|| {
            let mut tel = Telemetry::new();
            loop {
                let snap = tel.sample();
                {
                    let mut l = live_lock();
                    // Помпа публикует сама — свой опрос не нужен.
                    let pump_fresh = l.at.map(|t| t.elapsed() < Duration::from_millis(1500)).unwrap_or(false);
                    if !pump_fresh {
                        l.snap = Some(snap);
                        l.at = Some(Instant::now());
                    }
                    if l.wanted.map(|t| t.elapsed() > Duration::from_secs(5)).unwrap_or(true) || pump_fresh {
                        l.running = false;
                        return;
                    }
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        });
    }
    l.snap.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_drive_falls_back_to_c() {
        assert_eq!(system_drive_from(Some("d:")), "D:");
        assert_eq!(system_drive_from(Some("C:")), "C:");
        assert_eq!(system_drive_from(None), "C:");
        assert_eq!(system_drive_from(Some("")), "C:");
        assert_eq!(system_drive_from(Some("мусор")), "C:");
    }

    #[test]
    fn nvidia_smi_line() {
        let g = parse_nvidia_smi("37, 54, 112.45, 3072, 12288\n").unwrap();
        assert_eq!(g.load, 37.0);
        assert_eq!(g.temp, 54.0);
        assert_eq!(g.power_w, Some(112.45));
        assert_eq!(g.vram_used_gb, 3.0);
        assert_eq!(g.vram_total_gb, 12.0);
        // У некоторых карт мощность «[N/A]» — остальное всё равно берём.
        assert_eq!(parse_nvidia_smi("5, 40, [N/A], 512, 4096").unwrap().power_w, None);
        assert!(parse_nvidia_smi("").is_none());
    }
}
