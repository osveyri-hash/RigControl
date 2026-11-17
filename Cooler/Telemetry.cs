using System.IO;
using System.Net.NetworkInformation;
using System.Runtime.InteropServices;
using LibreHardwareMonitor.Hardware;

namespace RigControl;

/// <summary>Показания для экрана помпы. null — датчика нет или нет прав.</summary>
public readonly record struct Snapshot(
    double CpuLoad, double? CpuTemp, double RamLoad, double RamUsedGb, double RamTotalGb,
    double? GpuLoad, double? GpuTemp)
{
    public double? CpuClockMhz { get; init; }
    public double? CpuPowerW { get; init; }
    public double? GpuPowerW { get; init; }
    public double? VramUsedGb { get; init; }
    public double? VramTotalGb { get; init; }
    public double NetDownBps { get; init; }
    public double NetUpBps { get; init; }
    public double DiskUsedPct { get; init; }
    public double DiskFreeGb { get; init; }
    public TimeSpan Uptime { get; init; }

    /// <summary>Вентиляторы платы (если их опрашивали), null — не опрашивали.</summary>
    public IReadOnlyList<FanInfo>? Fans { get; init; }

    /// <summary>Плата и чип мониторинга, например «ASUS PRIME B650M-K · Nuvoton NCT6799D».</summary>
    public string? Board { get; init; }

    /// <summary>Показания для страницы «О системе» (андервольтинг); null — их не запрашивали.</summary>
    public UvReadings? Uv { get; init; }
}

/// <summary>Напряжения, частоты и температуры CPU/GPU для оценки андервольтинга. null — датчика нет.</summary>
public sealed record UvReadings
{
    public string? CpuName { get; init; }
    public string? GpuName { get; init; }

    /// <summary>Напряжение ядер CPU, В, и откуда оно: SVI (телеметрия CPU), плата (Super I/O) или VID (запрошенное).</summary>
    public double? CpuVcore { get; init; }
    public string? CpuVcoreSource { get; init; }
    public double? CpuClockMaxMhz { get; init; }

    /// <summary>Напряжение ядра GPU, В.</summary>
    public double? GpuCoreVolt { get; init; }
    public double? GpuCoreClockMhz { get; init; }
    public double? GpuMemClockMhz { get; init; }
    public double? GpuHotSpot { get; init; }
    public double? GpuMemTemp { get; init; }
    public double? GpuFanRpm { get; init; }
}

/// <summary>Канал вентилятора платы: Id — идентификатор управления в LibreHardwareMonitor.</summary>
public sealed record FanInfo(string Id, string Name, double? Rpm, double? Percent, bool Controlled);

/// <summary>
/// Загрузка CPU, память, сеть, диск и время работы — средствами Windows (без прав администратора);
/// температуры, видеокарта, частота и мощность — через LibreHardwareMonitor (нужен запуск от администратора).
/// LibreHardwareMonitor занимает заметно памяти, поэтому открывается, только когда такие показания есть на экране.
/// </summary>
public sealed class Telemetry : IDisposable
{
    private Computer? _computer;
    private bool _sensorsFailed;
    private long _sensorsUnusedSince;
    private long _lastIdle, _lastKernel, _lastUser;
    private bool _hasLast;
    private long _netRx, _netTx, _netAt;
    private long _diskAt;
    private double _diskUsed, _diskFree;

    public bool SensorsAvailable { get; private set; }

    private readonly Dictionary<string, ISensor> _controls = new();
    private readonly Dictionary<string, float> _applied = new();
    private readonly object _fanLock = new();
    private bool _markerWritten;
    private bool _restoreFailed;

    /// <summary>Действующий экземпляр — чтобы при сбое или завершении процесса вернуть вентиляторы плате.</summary>
    private static Telemetry? _current;

    /// <summary>
    /// Пока хоть один вентилятор под управлением программы, лежит этот файл (в нём — время старта Windows).
    /// Если программу закрыли аварийно («Снять задачу», сбой), файл остаётся — так следующий запуск узнаёт об этом.
    /// </summary>
    private static readonly string ManualMarker = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "RigControl", "fans-manual.flag");

    /// <summary>
    /// Прошлый запуск в этом же сеансе Windows закрылся аварийно, пока программа управляла вентиляторами:
    /// чип платы мог остаться на последней заданной скорости. Вернуть управление BIOS после такого может только
    /// перезагрузка — LibreHardwareMonitor помнит исходный режим вентиляторов лишь в том процессе, который его менял.
    /// </summary>
    public static bool LeftoverFromCrash { get; } = CheckLeftover();

    public Telemetry() => _current = this;

    /// <summary>Показание берётся из LibreHardwareMonitor (остальные Windows отдаёт сама).</summary>
    public static bool NeedsSensors(LcdKind kind) => kind is LcdKind.CpuTemp or LcdKind.GpuLoad or LcdKind.GpuTemp
        or LcdKind.CpuClock or LcdKind.CpuPower or LcdKind.GpuPower or LcdKind.Vram or LcdKind.GpuBar;

    /// <param name="useSensors">На экране есть температуры/видеокарта/частота/мощность — нужны датчики.</param>
    /// <param name="board">Нужны вентиляторы платы (страница «Вентиляторы» или управление скоростью).</param>
    /// <param name="details">Нужны показания для андервольтинга (<see cref="Snapshot.Uv"/>).</param>
    public Snapshot Read(bool useSensors = true, bool board = false, bool details = false)
    {
        useSensors |= board || details || _applied.Count > 0;
        List<FanInfo>? fans = null;
        string? boardName = null;
        var cpuLoad = ReadCpuLoad();
        var (ramLoad, usedGb, totalGb) = ReadMemory();

        double? cpuTemp = null, gpuTemp = null, gpuLoad = null;
        double? cpuClock = null, cpuPower = null, gpuPower = null, vramUsed = null, vramTotal = null;

        string? cpuName = null, gpuName = null;
        double? cpuSvi = null, cpuVid = null, boardVcore = null, cpuClockMax = null;
        double? gpuVolt = null, gpuClock = null, gpuMemClock = null, gpuHotSpot = null, gpuMemTemp = null, gpuFan = null;

        if (useSensors) OpenSensors();
        else CloseUnusedSensors();

        if (useSensors && _computer is not null)
        {
            try
            {
                var wantBoard = board || _applied.Count > 0;
                if (_computer.IsMotherboardEnabled != wantBoard)
                {
                    _computer.IsMotherboardEnabled = wantBoard;
                    if (!wantBoard)
                        lock (_fanLock) _controls.Clear();
                }

                foreach (var hardware in _computer.Hardware)
                {
                    hardware.Update();
                    foreach (var sub in hardware.SubHardware) sub.Update();

                    var sensors = hardware.Sensors.Concat(hardware.SubHardware.SelectMany(h => h.Sensors)).ToList();

                    switch (hardware.HardwareType)
                    {
                        case HardwareType.Motherboard:
                            boardName = hardware.Name;
                            var chip = hardware.SubHardware.FirstOrDefault(h => h.HardwareType == HardwareType.SuperIO);
                            if (chip is not null) boardName += " · " + chip.Name;
                            fans = ReadFans(sensors);
                            if (details) boardVcore ??= BoardVcore(sensors);
                            break;

                        case HardwareType.Cpu:
                            cpuTemp ??= PickTemperature(sensors, "package", "tctl", "tdie", "cpu");
                            cpuClock ??= Average(sensors, SensorType.Clock, "core #");
                            cpuPower ??= Pick(sensors, SensorType.Power, "package", "cpu");
                            if (details)
                            {
                                cpuName ??= hardware.Name;
                                cpuSvi ??= Volts(Pick(sensors, SensorType.Voltage, "core (svi"));
                                cpuVid ??= Volts(Max(sensors, SensorType.Voltage, n => n.Contains("vid", StringComparison.OrdinalIgnoreCase)));
                                cpuClockMax ??= Max(sensors, SensorType.Clock, n => n.StartsWith("core #", StringComparison.OrdinalIgnoreCase)
                                                                                    && !n.Contains("effective", StringComparison.OrdinalIgnoreCase));
                            }
                            break;

                        case HardwareType.GpuNvidia:
                        case HardwareType.GpuAmd:
                        case HardwareType.GpuIntel:
                            gpuTemp ??= PickTemperature(sensors, "gpu core", "core", "gpu");
                            gpuLoad ??= sensors
                                .Where(s => s.SensorType == SensorType.Load && s.Value.HasValue
                                            && s.Name.Contains("GPU Core", StringComparison.OrdinalIgnoreCase))
                                .Select(s => (double?)s.Value!.Value)
                                .FirstOrDefault();
                            gpuPower ??= Pick(sensors, SensorType.Power, "package", "gpu", "board", "total");
                            vramUsed ??= Pick(sensors, SensorType.SmallData, "gpu memory used", "d3d dedicated memory used") / 1024.0;
                            vramTotal ??= Pick(sensors, SensorType.SmallData, "gpu memory total") / 1024.0;
                            if (details && gpuName is null)
                            {
                                // Имена датчиков LibreHardwareMonitor: «GPU Core», «GPU Memory», «GPU Hot Spot», «GPU Fan».
                                gpuName = hardware.Name;
                                gpuVolt = Volts(Exact(sensors, SensorType.Voltage, "GPU Core") ?? Pick(sensors, SensorType.Voltage, "core"));
                                gpuClock = Exact(sensors, SensorType.Clock, "GPU Core") ?? Pick(sensors, SensorType.Clock, "core");
                                gpuMemClock = Exact(sensors, SensorType.Clock, "GPU Memory") ?? Pick(sensors, SensorType.Clock, "memory");
                                gpuHotSpot = Pick(sensors, SensorType.Temperature, "hot spot", "junction");
                                gpuMemTemp = Exact(sensors, SensorType.Temperature, "GPU Memory") ?? Pick(sensors, SensorType.Temperature, "memory junction");
                                gpuFan = Pick(sensors, SensorType.Fan, "fan");
                            }
                            break;
                    }
                }
            }
            catch (Exception ex)
            {
                // Один сбойный датчик не должен ронять экран.
                QuietLog.Write(ex);
            }
        }

        var (down, up) = ReadNetwork();
        ReadDisk();

        return new Snapshot(cpuLoad, cpuTemp, ramLoad, usedGb, totalGb, gpuLoad, gpuTemp)
        {
            CpuClockMhz = cpuClock,
            CpuPowerW = cpuPower,
            GpuPowerW = gpuPower,
            VramUsedGb = vramUsed,
            VramTotalGb = vramTotal,
            NetDownBps = down,
            NetUpBps = up,
            DiskUsedPct = _diskUsed,
            DiskFreeGb = _diskFree,
            Uptime = TimeSpan.FromMilliseconds(Environment.TickCount64),
            Fans = fans ?? (board ? new List<FanInfo>() : null),
            Board = boardName,
            Uv = !details ? null : new UvReadings
            {
                CpuName = cpuName,
                GpuName = gpuName,
                // SVI — то, что отдаёт регулятор; датчик платы — почти то же; VID — лишь запрос CPU (выше реального).
                CpuVcore = cpuSvi ?? boardVcore ?? cpuVid,
                CpuVcoreSource = cpuSvi is not null ? "SVI" : boardVcore is not null ? "плата" : cpuVid is not null ? "VID" : null,
                CpuClockMaxMhz = cpuClockMax,
                GpuCoreVolt = gpuVolt,
                GpuCoreClockMhz = gpuClock,
                GpuMemClockMhz = gpuMemClock,
                GpuHotSpot = gpuHotSpot,
                GpuMemTemp = gpuMemTemp,
                GpuFanRpm = gpuFan
            }
        };
    }

    /// <summary>Напряжение в вольтах; часть драйверов отдаёт милливольты. 0 и мусор — «нет данных».</summary>
    private static double? Volts(double? value) => value switch
    {
        > 10 => value / 1000.0,
        > 0.2 => value,
        _ => null
    };

    /// <summary>Vcore с чипа мониторинга платы («Vcore», «CPU Core»), без напряжения SoC.</summary>
    private static double? BoardVcore(List<ISensor> sensors) => Volts(sensors
        .Where(s => s.SensorType == SensorType.Voltage && s.Value.HasValue
                    && (s.Name.Contains("vcore", StringComparison.OrdinalIgnoreCase)
                        || s.Name.Equals("CPU Core", StringComparison.OrdinalIgnoreCase))
                    && !s.Name.Contains("soc", StringComparison.OrdinalIgnoreCase))
        .Select(s => (double?)s.Value!.Value)
        .FirstOrDefault());

    private static double? Exact(List<ISensor> sensors, SensorType type, string name) => sensors
        .Where(s => s.SensorType == type && s.Value.HasValue && s.Name.Equals(name, StringComparison.OrdinalIgnoreCase))
        .Select(s => (double?)s.Value!.Value)
        .FirstOrDefault();

    private static double? Max(List<ISensor> sensors, SensorType type, Func<string, bool> match)
    {
        double? max = null;
        foreach (var s in sensors)
        {
            if (s.SensorType != type || !s.Value.HasValue || !match(s.Name)) continue;
            if (max is null || s.Value.Value > max) max = s.Value.Value;
        }
        return max;
    }

    /// <summary>Каналы управления (Control, %) и обороты (Fan) с тем же номером.</summary>
    private List<FanInfo> ReadFans(List<ISensor> sensors)
    {
        var list = new List<FanInfo>();
        var rpm = sensors.Where(s => s.SensorType == SensorType.Fan).ToList();

        lock (_fanLock)
        {
            foreach (var control in sensors.Where(s => s.SensorType == SensorType.Control && s.Control is not null))
            {
                var id = control.Identifier.ToString();
                _controls[id] = control;

                var speed = rpm.FirstOrDefault(f => f.Index == control.Index);
                var name = speed?.Name ?? control.Name;
                list.Add(new FanInfo(id, name, speed?.Value, control.Value, _applied.ContainsKey(id)));
            }
        }

        return list;
    }

    /// <summary>
    /// Скорость вентиляторов: id → % (20–100) или null — вернуть плате (BIOS).
    /// Вызывать после Read в том же потоке.
    /// </summary>
    public void ApplyFans(IReadOnlyDictionary<string, float?> targets)
    {
        lock (_fanLock)
        {
            foreach (var (id, target) in targets)
            {
                if (!_controls.TryGetValue(id, out var sensor) || sensor.Control is not { } control) continue;

                try
                {
                    if (target is { } value)
                    {
                        value = Math.Clamp(value, 0, 100);
                        if (_applied.TryGetValue(id, out var old) && Math.Abs(old - value) < 0.5f) continue;
                        WriteMarker();
                        control.SetSoftware(value);
                        _applied[id] = value;
                    }
                    else if (_applied.Remove(id))
                    {
                        control.SetDefault();
                    }
                }
                catch (Exception ex)
                {
                    // Канал не управляется — пропускаем (но запоминаем, если не удалось вернуть его плате).
                    if (target is null) _restoreFailed = true;
                    QuietLog.Write(ex);
                }
            }

            if (_applied.Count == 0 && !_restoreFailed) DeleteMarker();
        }
    }

    private void RestoreFansLocked()
    {
        foreach (var id in _applied.Keys.ToList())
        {
            try
            {
                if (_controls.TryGetValue(id, out var sensor)) sensor.Control?.SetDefault();
            }
            catch (Exception ex)
            {
                _restoreFailed = true;
                QuietLog.Write(ex);
            }
        }

        _applied.Clear();
        if (!_restoreFailed) DeleteMarker();
    }

    /// <summary>
    /// Аварийный путь: процесс падает или завершается, а вентиляторы ещё не отданы плате — отдаём.
    /// Ждём не дольше 2 секунд, чтобы завершение процесса не зависло.
    /// </summary>
    public static void RestoreFansNow()
    {
        var telemetry = _current;
        if (telemetry is null || !System.Threading.Monitor.TryEnter(telemetry._fanLock, 2000)) return;

        try { telemetry.RestoreFansLocked(); }
        catch { /* процесс и так завершается */ }
        finally { System.Threading.Monitor.Exit(telemetry._fanLock); }
    }

    /// <summary>Отмечает, что вентиляторы под управлением программы (см. <see cref="LeftoverFromCrash"/>).</summary>
    private void WriteMarker()
    {
        if (_markerWritten) return;
        _markerWritten = true;

        try
        {
            Directory.CreateDirectory(Path.GetDirectoryName(ManualMarker)!);
            File.WriteAllText(ManualMarker, BootTime().Ticks.ToString(System.Globalization.CultureInfo.InvariantCulture));
        }
        catch (Exception ex)
        {
            QuietLog.Write(ex);
        }
    }

    private void DeleteMarker()
    {
        if (!_markerWritten) return;
        _markerWritten = false;

        try
        {
            File.Delete(ManualMarker);
        }
        catch (Exception ex)
        {
            QuietLog.Write(ex);
        }
    }

    private static bool CheckLeftover()
    {
        try
        {
            if (!File.Exists(ManualMarker)) return false;

            var text = File.ReadAllText(ManualMarker).Trim();
            File.Delete(ManualMarker);

            // Windows с тех пор перезапускали — после перезагрузки вентиляторами снова управляет BIOS.
            if (!long.TryParse(text, out var bootTicks)) return true;
            return Math.Abs(BootTime().Ticks - bootTicks) < TimeSpan.FromMinutes(5).Ticks;
        }
        catch (Exception ex)
        {
            QuietLog.Write(ex);
            return false;
        }
    }

    /// <summary>Время старта Windows (UTC); сон и гибернация входят в счётчик, так что оно не «плывёт».</summary>
    private static DateTime BootTime() => DateTime.UtcNow - TimeSpan.FromMilliseconds(Environment.TickCount64);

    private void OpenSensors()
    {
        _sensorsUnusedSince = 0;
        if (_computer is not null || _sensorsFailed) return;

        var computer = new Computer { IsCpuEnabled = true, IsGpuEnabled = true };
        try
        {
            computer.Open();
            _computer = computer;
            SensorsAvailable = true;
        }
        catch (Exception ex)
        {
            // Нет драйвера или прав — остаются показания Windows. Повторно не пробуем.
            QuietLog.Write(ex);
            try { computer.Close(); } catch { /* молча */ }
            _sensorsFailed = true;
            SensorsAvailable = false;
        }
    }

    /// <summary>Датчики не нужны уже минуту (их убрали с экрана) — закрываем LibreHardwareMonitor.</summary>
    private void CloseUnusedSensors()
    {
        if (_computer is null || _applied.Count > 0) return;

        var now = Environment.TickCount64;
        if (_sensorsUnusedSince == 0) _sensorsUnusedSince = now;
        if (now - _sensorsUnusedSince >= 60_000) CloseSensors();
    }

    private void CloseSensors()
    {
        lock (_fanLock)
        {
            RestoreFansLocked();
            _controls.Clear();
        }

        try { _computer?.Close(); } catch { /* закрываем молча */ }
        _computer = null;
        _sensorsUnusedSince = 0;
    }

    private static double? Pick(List<ISensor> sensors, SensorType type, params string[] preferred)
    {
        var list = sensors.Where(s => s.SensorType == type && s.Value.HasValue).ToList();
        foreach (var key in preferred)
        {
            var hit = list.FirstOrDefault(s => s.Name.Contains(key, StringComparison.OrdinalIgnoreCase));
            if (hit is not null) return hit.Value!.Value;
        }
        return null;
    }

    private static double? Average(List<ISensor> sensors, SensorType type, string key)
    {
        double sum = 0;
        var count = 0;
        foreach (var s in sensors)
        {
            if (s.SensorType != type || !s.Value.HasValue) continue;
            if (!s.Name.Contains(key, StringComparison.OrdinalIgnoreCase)) continue;
            sum += s.Value.Value;
            count++;
        }
        return count > 0 ? sum / count : null;
    }

    /// <summary>Скорость сети по всем активным адаптерам, байт/с.</summary>
    private (double Down, double Up) ReadNetwork()
    {
        try
        {
            long rx = 0, tx = 0;
            foreach (var nic in NetworkInterface.GetAllNetworkInterfaces())
            {
                if (nic.OperationalStatus != OperationalStatus.Up) continue;
                if (nic.NetworkInterfaceType is NetworkInterfaceType.Loopback or NetworkInterfaceType.Tunnel) continue;
                var stats = nic.GetIPStatistics();
                rx += stats.BytesReceived;
                tx += stats.BytesSent;
            }

            var now = Environment.TickCount64;
            double down = 0, up = 0;
            if (_netAt > 0 && now > _netAt)
            {
                var seconds = (now - _netAt) / 1000.0;
                down = Math.Max(0, rx - _netRx) / seconds;
                up = Math.Max(0, tx - _netTx) / seconds;
            }

            _netRx = rx;
            _netTx = tx;
            _netAt = now;
            return (down, up);
        }
        catch
        {
            return (0, 0);
        }
    }

    /// <summary>Заполненность системного диска — раз в 30 секунд, чаще незачем.</summary>
    private void ReadDisk()
    {
        var now = Environment.TickCount64;
        if (_diskAt > 0 && now - _diskAt < 30_000) return;
        _diskAt = now;

        try
        {
            var root = Path.GetPathRoot(Environment.SystemDirectory) ?? "C:\\";
            var drive = new DriveInfo(root);
            if (!drive.IsReady || drive.TotalSize <= 0) return;

            _diskFree = drive.AvailableFreeSpace / (1024.0 * 1024 * 1024);
            _diskUsed = 100.0 * (drive.TotalSize - drive.AvailableFreeSpace) / drive.TotalSize;
        }
        catch (Exception ex)
        {
            // нет доступа — оставляем прошлое значение
            QuietLog.Write(ex);
        }
    }

    private static double? PickTemperature(List<ISensor> sensors, params string[] preferred)
    {
        var temps = sensors
            .Where(s => s.SensorType == SensorType.Temperature && s.Value.HasValue)
            .ToList();

        foreach (var key in preferred)
        {
            var hit = temps.FirstOrDefault(s => s.Name.Contains(key, StringComparison.OrdinalIgnoreCase));
            if (hit is not null) return hit.Value!.Value;
        }

        return temps.Count > 0 ? temps.Max(s => s.Value!.Value) : null;
    }

    private double ReadCpuLoad()
    {
        if (!GetSystemTimes(out var idle, out var kernel, out var user)) return 0;

        double load = 0;
        if (_hasLast)
        {
            var idleDelta = idle - _lastIdle;
            var total = (kernel - _lastKernel) + (user - _lastUser); // kernel уже включает idle

            if (total > 0) load = Math.Clamp(100.0 * (1.0 - (double)idleDelta / total), 0, 100);
        }

        _lastIdle = idle;
        _lastKernel = kernel;
        _lastUser = user;
        _hasLast = true;
        return load;
    }

    private static (double Load, double UsedGb, double TotalGb) ReadMemory()
    {
        var status = new MemoryStatusEx { Length = (uint)Marshal.SizeOf<MemoryStatusEx>() };
        if (!GlobalMemoryStatusEx(ref status) || status.TotalPhys == 0) return (0, 0, 0);

        const double gb = 1024.0 * 1024 * 1024;
        var total = status.TotalPhys / gb;
        var used = (status.TotalPhys - status.AvailPhys) / gb;
        return (status.MemoryLoad, used, total);
    }

    public void Dispose()
    {
        CloseSensors();
        if (ReferenceEquals(_current, this)) _current = null;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct MemoryStatusEx
    {
        public uint Length;
        public uint MemoryLoad;
        public ulong TotalPhys;
        public ulong AvailPhys;
        public ulong TotalPageFile;
        public ulong AvailPageFile;
        public ulong TotalVirtual;
        public ulong AvailVirtual;
        public ulong AvailExtendedVirtual;
    }

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetSystemTimes(out long idle, out long kernel, out long user);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GlobalMemoryStatusEx(ref MemoryStatusEx buffer);
}
