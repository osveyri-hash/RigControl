// Помощник датчиков для rigcontrol-rs: LibreHardwareMonitor в отдельном процессе.
// Логика — из Cooler/Telemetry.cs RigControl 2.17.3 (C#): те же датчики, те же правила выбора, то же управление.
//
// Обмен — по одной строке JSON на запрос и на ответ через stdin/stdout:
//   {"cmd":"read","board":true}                        → снимок датчиков; вентиляторы платы — при board
//                                                          (и пока программа ими управляет)
//   {"cmd":"fans","targets":{"<id>":55,"<id2>":null}}  → {"ok":true}; число — % (0–100), null — вернуть плате (BIOS)
//   {"cmd":"zen"}                                      → {"ok":true,"zen":{…}} — Curve Optimizer и тайминги ОЗУ (ZenTuning.cs)
//   {"cmd":"specs"}                                    → {"ok":true,"specs":{…}} — характеристики ПК и паспорт (SystemInfo.cs)
//   {"cmd":"quit"}                                     → вернуть вентиляторы плате и выйти
// Закрылся stdin (программа вышла, упала или её сняли в диспетчере задач) — тоже возвращаем вентиляторы плате.
// Права администратора помощник получает от rigcontrol-gui.exe, который его запускает.
using System.Security.Principal;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using LibreHardwareMonitor.Hardware;

namespace RigControl.Sensors;

internal static class Program
{
    private static readonly JsonSerializerOptions Json = new() { PropertyNamingPolicy = JsonNamingPolicy.CamelCase };

    private static int Main()
    {
        var utf8 = new UTF8Encoding(false);
        using var input = new StreamReader(Console.OpenStandardInput(), utf8);
        using var output = new StreamWriter(Console.OpenStandardOutput(), utf8) { AutoFlush = true, NewLine = "\n" };
        // stdout — только для ответов: если библиотека что-то напишет в консоль, протокол не собьётся.
        Console.SetOut(TextWriter.Null);
        Console.SetError(TextWriter.Null);

        using var sensors = new Sensors();
        AppDomain.CurrentDomain.ProcessExit += (_, _) => sensors.RestoreFans();

        while (input.ReadLine() is { } line)
        {
            if (string.IsNullOrWhiteSpace(line)) continue;

            string reply;
            var quit = false;
            try
            {
                var request = JsonNode.Parse(line)?.AsObject() ?? throw new InvalidDataException("пустой запрос");
                var cmd = (string?)request["cmd"];
                switch (cmd)
                {
                    case "read":
                        reply = JsonSerializer.Serialize(sensors.Read((bool?)request["board"] ?? false), Json);
                        break;
                    case "fans":
                        sensors.ApplyFans(Targets(request["targets"] as JsonObject));
                        reply = """{"ok":true}""";
                        break;
                    case "zen":
                        reply = JsonSerializer.Serialize(new { ok = true, zen = ZenTuning.Read() }, Json);
                        break;
                    case "specs":
                        reply = JsonSerializer.Serialize(new { ok = true, specs = SystemInfo.Read() }, Json);
                        break;
                    case "quit":
                        sensors.RestoreFans();
                        reply = """{"ok":true}""";
                        quit = true;
                        break;
                    default:
                        throw new InvalidDataException($"неизвестная команда: {cmd}");
                }
            }
            catch (Exception ex)
            {
                reply = JsonSerializer.Serialize(new { ok = false, error = ex.Message }, Json);
            }

            output.WriteLine(reply);
            if (quit) return 0;
            HelperMemory.TrimSometimes();
        }

        return 0; // stdin закрыт: Dispose вернёт вентиляторы плате
    }

    private static Dictionary<string, float?> Targets(JsonObject? targets)
    {
        var result = new Dictionary<string, float?>();
        if (targets is null) return result;
        foreach (var (id, value) in targets)
            result[id] = value is null ? null : (float)value.GetValue<double>();
        return result;
    }
}

/// <summary>
/// Помощник работает сутками: раз в минуту собираем мусор с возвратом памяти системе и отдаём
/// неиспользуемые страницы (то же, что делает основная программа), чтобы он не держал лишние десятки МБ.
/// </summary>
internal static class HelperMemory
{
    private static long _next = Environment.TickCount64 + 20_000;

    public static void TrimSometimes()
    {
        var now = Environment.TickCount64;
        if (now < _next) return;
        _next = now + 60_000;
        try
        {
            GC.Collect(GC.MaxGeneration, GCCollectionMode.Aggressive, blocking: true, compacting: true);
            GC.WaitForPendingFinalizers();
            EmptyWorkingSet(new IntPtr(-1));   // -1 — псевдодескриптор своего процесса
        }
        catch
        {
            // не страшно
        }
    }

    [System.Runtime.InteropServices.DllImport("psapi.dll")]
    private static extern bool EmptyWorkingSet(IntPtr process);
}

/// <summary>Ответ на read. Имена полей — camelCase, как ждёт src/lhm.rs.</summary>
internal sealed class Reply
{
    public bool Ok { get; set; } = true;
    public string? Error { get; set; }
    public bool Admin { get; set; }
    public bool SensorsAvailable { get; set; }
    public string? Board { get; set; }
    public string? CpuName { get; set; }
    public string? GpuName { get; set; }
    public double? CpuTemp { get; set; }
    public double? CpuClockMhz { get; set; }
    public double? CpuClockMaxMhz { get; set; }
    public double? CpuPowerW { get; set; }
    public double? CpuVcore { get; set; }
    public string? CpuVcoreSource { get; set; }
    public double? GpuLoad { get; set; }
    public double? GpuTemp { get; set; }
    public double? GpuHotSpot { get; set; }
    public double? GpuMemTemp { get; set; }
    public double? GpuPowerW { get; set; }
    public double? GpuCoreVolt { get; set; }
    public double? GpuCoreClockMhz { get; set; }
    public double? GpuMemClockMhz { get; set; }
    public double? GpuFanRpm { get; set; }
    public double? VramUsedGb { get; set; }
    public double? VramTotalGb { get; set; }
    public List<FanReply>? Fans { get; set; }
}

/// <summary>Канал вентилятора платы: Id — идентификатор управления в LibreHardwareMonitor (FanInfo в C#).</summary>
internal sealed record FanReply(string Id, string Name, double? Rpm, double? Percent, bool Controlled);

/// <summary>Нужная часть Cooler/Telemetry.cs: процессор, видеокарта, плата и вентиляторы.</summary>
internal sealed class Sensors : IDisposable
{
    private static readonly bool Admin = IsAdmin();

    private Computer? _computer;
    private bool _openFailed;
    private string? _openError;

    /// <summary>Плату держим открытой ещё минуту после последнего запроса с board: её открытие дорогое.</summary>
    private long _boardUntil;

    private readonly Dictionary<string, ISensor> _controls = new();
    private readonly Dictionary<string, float> _applied = new();
    private readonly object _fanLock = new();

    public Reply Read(bool board)
    {
        var reply = new Reply { Admin = Admin };
        Open();
        if (_computer is null)
        {
            reply.Ok = false;
            reply.Error = _openError ?? "LibreHardwareMonitor не открылся";
            return reply;
        }

        reply.SensorsAvailable = true;
        var now = Environment.TickCount64;
        if (board) _boardUntil = now + 60_000;

        bool wantBoard;
        lock (_fanLock) wantBoard = board || _applied.Count > 0 || now < _boardUntil;

        double? cpuSvi = null, cpuVid = null, boardVcore = null;
        try
        {
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
                        reply.Board = hardware.Name;
                        var chip = hardware.SubHardware.FirstOrDefault(h => h.HardwareType == HardwareType.SuperIO);
                        if (chip is not null) reply.Board += " · " + chip.Name;
                        reply.Fans = ReadFans(sensors);
                        boardVcore ??= BoardVcore(sensors);
                        break;

                    case HardwareType.Cpu:
                        reply.CpuName ??= hardware.Name;
                        reply.CpuTemp ??= PickTemperature(sensors, "package", "tctl", "tdie", "cpu");
                        reply.CpuClockMhz ??= Average(sensors, SensorType.Clock, "core #");
                        reply.CpuPowerW ??= Pick(sensors, SensorType.Power, "package", "cpu");
                        cpuSvi ??= Volts(Pick(sensors, SensorType.Voltage, "core (svi"));
                        cpuVid ??= Volts(Max(sensors, SensorType.Voltage, n => n.Contains("vid", StringComparison.OrdinalIgnoreCase)));
                        reply.CpuClockMaxMhz ??= Max(sensors, SensorType.Clock, n => n.StartsWith("core #", StringComparison.OrdinalIgnoreCase)
                                                                                    && !n.Contains("effective", StringComparison.OrdinalIgnoreCase));
                        break;

                    case HardwareType.GpuNvidia:
                    case HardwareType.GpuAmd:
                    case HardwareType.GpuIntel:
                        reply.GpuTemp ??= PickTemperature(sensors, "gpu core", "core", "gpu");
                        reply.GpuLoad ??= sensors
                            .Where(s => s.SensorType == SensorType.Load && Has(s)
                                        && s.Name.Contains("GPU Core", StringComparison.OrdinalIgnoreCase))
                            .Select(s => (double?)s.Value!.Value)
                            .FirstOrDefault();
                        reply.GpuPowerW ??= Pick(sensors, SensorType.Power, "package", "gpu", "board", "total");
                        reply.VramUsedGb ??= Pick(sensors, SensorType.SmallData, "gpu memory used", "d3d dedicated memory used") / 1024.0;
                        reply.VramTotalGb ??= Pick(sensors, SensorType.SmallData, "gpu memory total") / 1024.0;
                        if (reply.GpuName is null)
                        {
                            // Имена датчиков LibreHardwareMonitor: «GPU Core», «GPU Memory», «GPU Hot Spot», «GPU Fan».
                            reply.GpuName = hardware.Name;
                            reply.GpuCoreVolt = Volts(Exact(sensors, SensorType.Voltage, "GPU Core") ?? Pick(sensors, SensorType.Voltage, "core"));
                            reply.GpuCoreClockMhz = Exact(sensors, SensorType.Clock, "GPU Core") ?? Pick(sensors, SensorType.Clock, "core");
                            reply.GpuMemClockMhz = Exact(sensors, SensorType.Clock, "GPU Memory") ?? Pick(sensors, SensorType.Clock, "memory");
                            reply.GpuHotSpot = Pick(sensors, SensorType.Temperature, "hot spot", "junction");
                            reply.GpuMemTemp = Exact(sensors, SensorType.Temperature, "GPU Memory") ?? Pick(sensors, SensorType.Temperature, "memory junction");
                            reply.GpuFanRpm = Pick(sensors, SensorType.Fan, "fan");
                        }
                        break;
                }
            }
        }
        catch (Exception ex)
        {
            // Один сбойный датчик не должен ронять остальное.
            reply.Error = ex.Message;
        }

        // SVI — то, что отдаёт регулятор; датчик платы — почти то же; VID — лишь запрос CPU (выше реального).
        reply.CpuVcore = cpuSvi ?? boardVcore ?? cpuVid;
        reply.CpuVcoreSource = cpuSvi is not null ? "SVI" : boardVcore is not null ? "плата" : cpuVid is not null ? "VID" : null;
        if (wantBoard) reply.Fans ??= new List<FanReply>();
        return reply;
    }

    /// <summary>Скорость вентиляторов: id → % (0–100) или null — вернуть плате (BIOS). Звать после Read с board.</summary>
    public void ApplyFans(IReadOnlyDictionary<string, float?> targets)
    {
        // Каналы, которые не удалось вернуть плате: вентилятор мог остаться на заданной (низкой) скорости.
        List<string>? notRestored = null;
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
                        control.SetSoftware(value);
                        _applied[id] = value;
                    }
                    else if (_applied.ContainsKey(id))
                    {
                        // Забываем канал только после удачного возврата: иначе следующий запрос «вернуть плате»
                        // ничего бы не сделал, и RestoreFans при выходе его бы пропустил.
                        control.SetDefault();
                        _applied.Remove(id);
                    }
                }
                catch (Exception ex)
                {
                    // Канал не управляется — пропускаем. Но о неудачном возврате плате молчать нельзя
                    // (в C#-версии это _restoreFailed и запись в журнал).
                    if (target is null) (notRestored ??= new List<string>()).Add($"{id}: {ex.Message}");
                }
            }
        }

        if (notRestored is not null)
            throw new IOException("не удалось вернуть вентилятор плате (" + string.Join("; ", notRestored) + ")");
    }

    /// <summary>Все каналы, которыми управляли, — обратно плате.</summary>
    public void RestoreFans()
    {
        if (!System.Threading.Monitor.TryEnter(_fanLock, 2000)) return;
        try
        {
            foreach (var id in _applied.Keys.ToList())
            {
                try
                {
                    if (_controls.TryGetValue(id, out var sensor)) sensor.Control?.SetDefault();
                    _applied.Remove(id);
                }
                catch
                {
                    // Процесс и так завершается; канал остаётся в списке — Dispose попробует ещё раз.
                }
            }
        }
        finally
        {
            System.Threading.Monitor.Exit(_fanLock);
        }
    }

    public void Dispose()
    {
        RestoreFans();
        try { _computer?.Close(); } catch { /* закрываем молча */ }
        _computer = null;
    }

    private void Open()
    {
        if (_computer is not null || _openFailed) return;

        var computer = new Computer { IsCpuEnabled = true, IsGpuEnabled = true };
        try
        {
            computer.Open();
            _computer = computer;
        }
        catch (Exception ex)
        {
            // Нет драйвера или прав. Повторно не пробуем, как в C#.
            _openFailed = true;
            _openError = ex.Message;
            try { computer.Close(); } catch { /* молча */ }
        }
    }

    /// <summary>Каналы управления (Control, %) и обороты (Fan) с тем же номером.</summary>
    private List<FanReply> ReadFans(List<ISensor> sensors)
    {
        var list = new List<FanReply>();
        var rpm = sensors.Where(s => s.SensorType == SensorType.Fan).ToList();

        lock (_fanLock)
        {
            foreach (var control in sensors.Where(s => s.SensorType == SensorType.Control && s.Control is not null))
            {
                var id = control.Identifier.ToString();
                _controls[id] = control;

                var speed = rpm.FirstOrDefault(f => f.Index == control.Index);
                var name = speed?.Name ?? control.Name;
                list.Add(new FanReply(id, name, Value(speed), Value(control), _applied.ContainsKey(id)));
            }
        }

        return list;
    }

    private static bool IsAdmin()
    {
        try
        {
            using var identity = WindowsIdentity.GetCurrent();
            return new WindowsPrincipal(identity).IsInRole(WindowsBuiltInRole.Administrator);
        }
        catch
        {
            return false;
        }
    }

    /// <summary>Показание есть и это число (NaN и бесконечности — «нет данных»: в JSON их не передать).</summary>
    private static bool Has(ISensor s) => s.Value is { } v && float.IsFinite(v);

    private static double? Value(ISensor? s) => s is not null && Has(s) ? s.Value!.Value : null;

    /// <summary>Напряжение в вольтах; часть драйверов отдаёт милливольты. 0 и мусор — «нет данных».</summary>
    private static double? Volts(double? value) => value switch
    {
        > 10 => value / 1000.0,
        > 0.2 => value,
        _ => null
    };

    /// <summary>Vcore с чипа мониторинга платы («Vcore», «CPU Core»), без напряжения SoC.</summary>
    private static double? BoardVcore(List<ISensor> sensors) => Volts(sensors
        .Where(s => s.SensorType == SensorType.Voltage && Has(s)
                    && (s.Name.Contains("vcore", StringComparison.OrdinalIgnoreCase)
                        || s.Name.Equals("CPU Core", StringComparison.OrdinalIgnoreCase))
                    && !s.Name.Contains("soc", StringComparison.OrdinalIgnoreCase))
        .Select(s => (double?)s.Value!.Value)
        .FirstOrDefault());

    private static double? Exact(List<ISensor> sensors, SensorType type, string name) => sensors
        .Where(s => s.SensorType == type && Has(s) && s.Name.Equals(name, StringComparison.OrdinalIgnoreCase))
        .Select(s => (double?)s.Value!.Value)
        .FirstOrDefault();

    private static double? Max(List<ISensor> sensors, SensorType type, Func<string, bool> match)
    {
        double? max = null;
        foreach (var s in sensors)
        {
            if (s.SensorType != type || !Has(s) || !match(s.Name)) continue;
            if (max is null || s.Value!.Value > max) max = s.Value!.Value;
        }
        return max;
    }

    private static double? Pick(List<ISensor> sensors, SensorType type, params string[] preferred)
    {
        var list = sensors.Where(s => s.SensorType == type && Has(s)).ToList();
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
            if (s.SensorType != type || !Has(s)) continue;
            if (!s.Name.Contains(key, StringComparison.OrdinalIgnoreCase)) continue;
            sum += s.Value!.Value;
            count++;
        }
        return count > 0 ? sum / count : null;
    }

    private static double? PickTemperature(List<ISensor> sensors, params string[] preferred)
    {
        var temps = sensors.Where(s => s.SensorType == SensorType.Temperature && Has(s)).ToList();

        foreach (var key in preferred)
        {
            var hit = temps.FirstOrDefault(s => s.Name.Contains(key, StringComparison.OrdinalIgnoreCase));
            if (hit is not null) return hit.Value!.Value;
        }

        return temps.Count > 0 ? temps.Max(s => s.Value!.Value) : null;
    }
}
