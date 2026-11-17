using System.Globalization;
using System.IO;
using System.Management;
using System.Text.RegularExpressions;
using Microsoft.Win32;

namespace RigControl;

/// <summary>Строка карточки «Характеристики»: название, значение и подробности мелким шрифтом.</summary>
public sealed record SpecRow(string Label, string Value, string? Detail);

/// <summary>Паспорт процессора — для сравнения с тем, что показывают датчики.</summary>
public sealed record CpuSpec(string Summary, double BaseMhz, double BoostMhz, double PptW, double TjMaxC, string Tips);

/// <summary>Паспорт видеокарты; MaxVoltMv — верхняя точка стоковой кривой напряжения.</summary>
public sealed record GpuSpec(string Summary, double BoostMhz, double MemClockMhz, double MaxVoltMv, double HotSpotLimitC,
    double BoardPowerW, string Tips);

public sealed record SystemSpecs(IReadOnlyList<SpecRow> Rows, string? CpuName, string? GpuName, CpuSpec? Cpu, GpuSpec? Gpu);

/// <summary>Характеристики ПК из реестра и WMI (без LibreHardwareMonitor). Читаются один раз — это сотни миллисекунд.</summary>
public static class SystemInfo
{
    private static readonly (string Match, CpuSpec Spec)[] KnownCpus =
    {
        ("7500F", new CpuSpec(
            "Zen 4 (Raphael), AM5 · 6 ядер / 12 потоков · 3,7–5,0 ГГц · L3 32 МБ · TDP 65 Вт (PPT 88 Вт)",
            BaseMhz: 3700, BoostMhz: 5000, PptW: 88, TjMaxC: 95,
            "Процессор: андервольт Ryzen 7000 делается в BIOS — Precision Boost Overdrive → Curve Optimizer → All Cores → Negative. " +
            "Начните с −10…−15 и проверяйте OCCT, Cinebench R23 (10 минут) и CoreCycler; многие Ryzen 7000 держат −20…−30, но у каждого чипа свой предел. " +
            "Перезагрузки, вылеты в простое или ошибки WHEA в журнале Windows — уменьшите шаг. " +
            "Удачный андервольт: частота под нагрузкой та же или выше, а напряжение, температура и ватты — ниже."))
    };

    private static readonly (string Match, GpuSpec Spec)[] KnownGpus =
    {
        ("5700 XT", new GpuSpec(
            "RDNA 1 (Navi 10) · 2560 потоковых процессоров · 1605 / 1755 / 1905 МГц (база / игровая / буст) · 8 ГБ GDDR6, 256 бит, 14 Гбит/с · TBP 225 Вт",
            BoostMhz: 1905, MemClockMhz: 1750, MaxVoltMv: 1200, HotSpotLimitC: 110, BoardPowerW: 225,
            "Видеокарта: AMD Software (Adrenalin) → Производительность → Настройка → Ручная настройка → Настройка GPU. " +
            "В стоке RX 5700 XT поднимает до 1200 мВ; многим картам хватает 1050–1100 мВ при 1950–2000 МГц, но у каждого чипа свой предел. " +
            "Снижайте по 10–25 мВ и проверяйте 15–20 минут в игре или 3DMark. " +
            "Сброс драйвера, чёрный экран или вылет игры — верните 10–25 мВ. Хотспот до 110 °C AMD считает нормой, но чем ниже — тем тише и стабильнее."))
    };

    public static SystemSpecs Read()
    {
        var rows = new List<SpecRow>();

        var cpuName = CleanCpuName(Reg(@"HARDWARE\DESCRIPTION\System\CentralProcessor\0", "ProcessorNameString"));
        var cpuSpec = Find(KnownCpus, cpuName);
        rows.Add(new SpecRow("Процессор", cpuName ?? "Не определён", cpuSpec?.Summary ?? CpuDetail()));

        var gpu = ReadGpu();
        var gpuSpec = Find(KnownGpus, gpu?.Name);
        if (gpu is not null)
        {
            var parts = new List<string>();
            if (gpuSpec is not null) parts.Add(gpuSpec.Summary);
            else if (gpu.MemoryBytes > 0) parts.Add($"{gpu.MemoryBytes / 1073741824.0:0.#} ГБ видеопамяти");
            if (gpu.Adrenalin is not null) parts.Add("AMD Software " + gpu.Adrenalin);
            if (gpu.Driver is not null) parts.Add("драйвер " + gpu.Driver);
            rows.Add(new SpecRow("Видеокарта", gpu.Name, string.Join(" · ", parts)));
        }
        else rows.Add(new SpecRow("Видеокарта", "Не определена", null));

        rows.Add(ReadMemory());

        var board = string.Join(" ", new[]
        {
            ShortVendor(Reg(@"HARDWARE\DESCRIPTION\System\BIOS", "BaseBoardManufacturer")),
            Reg(@"HARDWARE\DESCRIPTION\System\BIOS", "BaseBoardProduct")
        }.Where(s => !string.IsNullOrWhiteSpace(s)));
        var bios = Reg(@"HARDWARE\DESCRIPTION\System\BIOS", "BIOSVersion");
        var biosDate = Reg(@"HARDWARE\DESCRIPTION\System\BIOS", "BIOSReleaseDate");
        if (DateTime.TryParseExact(biosDate, "MM/dd/yyyy", CultureInfo.InvariantCulture, DateTimeStyles.None, out var date))
            biosDate = date.ToString("dd.MM.yyyy", CultureInfo.InvariantCulture);
        rows.Add(new SpecRow("Материнская плата", board.Length > 0 ? board : "Не определена",
            bios is null ? null : $"BIOS {bios}" + (biosDate is null ? "" : $" от {biosDate}")));

        rows.Add(ReadWindows());

        try
        {
            var drive = new DriveInfo(Path.GetPathRoot(Environment.SystemDirectory) ?? "C:\\");
            if (drive.IsReady)
                rows.Add(new SpecRow($"Системный диск {drive.Name.TrimEnd('\\')}", $"{drive.TotalSize / 1e9:0} ГБ",
                    $"свободно {drive.AvailableFreeSpace / 1e9:0} ГБ"));
        }
        catch (Exception ex) { QuietLog.Write(ex); }

        return new SystemSpecs(rows, cpuName, gpu?.Name, cpuSpec, gpuSpec);
    }

    /// <summary>Паспорт известной модели по названию (например, «7500F» в «AMD Ryzen 5 7500F»).</summary>
    public static T? Find<T>((string Match, T Spec)[] known, string? name) where T : class =>
        name is null ? null : known.FirstOrDefault(k => name.Contains(k.Match, StringComparison.OrdinalIgnoreCase)).Spec;

    public static CpuSpec? FindCpu(string? name) => Find(KnownCpus, name);

    public static GpuSpec? FindGpu(string? name) => Find(KnownGpus, name);

    /// <summary>«AMD Ryzen 5 7500F 6-Core Processor   » → «AMD Ryzen 5 7500F».</summary>
    private static string? CleanCpuName(string? name)
    {
        if (string.IsNullOrWhiteSpace(name)) return null;
        name = Regex.Replace(name.Trim(), @"\s+\d+-Core Processor$", "", RegexOptions.IgnoreCase);
        return Regex.Replace(name, @"\s{2,}", " ");
    }

    private static string? CpuDetail()
    {
        var cpu = Query("SELECT NumberOfCores, NumberOfLogicalProcessors, MaxClockSpeed FROM Win32_Processor",
            "NumberOfCores", "NumberOfLogicalProcessors", "MaxClockSpeed");
        if (cpu.Count == 0) return $"потоков: {Environment.ProcessorCount}";

        var cores = cpu.Sum(c => ToLong(c["NumberOfCores"]));
        var threads = cpu.Sum(c => ToLong(c["NumberOfLogicalProcessors"]));
        var clock = ToLong(cpu[0]["MaxClockSpeed"]);
        return $"ядер: {cores}, потоков: {threads}" + (clock > 0 ? $" · {clock / 1000.0:0.0} ГГц" : "");
    }

    private sealed record GpuInfo(string Name, long MemoryBytes, string? Driver, string? Adrenalin);

    /// <summary>
    /// Видеокарта из ключа класса видеоадаптеров: WMI (Win32_VideoController.AdapterRAM) упирается в 4 ГБ,
    /// а HardwareInformation.qwMemorySize — нет. Из нескольких адаптеров берём тот, у кого больше памяти.
    /// </summary>
    private static GpuInfo? ReadGpu()
    {
        GpuInfo? best = null;
        try
        {
            using var cls = Registry.LocalMachine.OpenSubKey(@"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}");
            if (cls is null) return null;

            foreach (var sub in cls.GetSubKeyNames().Where(n => n.Length == 4 && n.All(char.IsDigit)))
            {
                try
                {
                    using var key = cls.OpenSubKey(sub);
                    if (key?.GetValue("DriverDesc") is not string name || string.IsNullOrWhiteSpace(name)) continue;

                    var memory = ToLong(key.GetValue("HardwareInformation.qwMemorySize"));
                    if (memory <= 0) memory = ToLong(key.GetValue("HardwareInformation.MemorySize"));
                    var info = new GpuInfo(name.Trim(), memory, key.GetValue("DriverVersion") as string,
                        key.GetValue("RadeonSoftwareVersion") as string);
                    if (best is null || info.MemoryBytes > best.MemoryBytes) best = info;
                }
                catch (Exception ex) { QuietLog.Write(ex); }   // ключ без доступа — следующий
            }
        }
        catch (Exception ex) { QuietLog.Write(ex); }

        return best;
    }

    private static SpecRow ReadMemory()
    {
        var modules = Query("SELECT Capacity, ConfiguredClockSpeed, Speed, SMBIOSMemoryType, Manufacturer, PartNumber FROM Win32_PhysicalMemory",
            "Capacity", "ConfiguredClockSpeed", "Speed", "SMBIOSMemoryType", "Manufacturer", "PartNumber");

        if (modules.Count == 0)
        {
            var total = GC.GetGCMemoryInfo().TotalAvailableMemoryBytes / (1024.0 * 1024 * 1024);
            return new SpecRow("Оперативная память", total > 0 ? $"{Math.Ceiling(total)} ГБ" : "Не определена", null);
        }

        const double gb = 1024.0 * 1024 * 1024;
        var sizes = modules.Select(m => ToLong(m["Capacity"]) / gb).ToList();
        var type = ToLong(modules[0]["SMBIOSMemoryType"]) switch
        {
            20 => "DDR", 21 => "DDR2", 24 => "DDR3", 26 => "DDR4", 30 => "LPDDR4", 34 => "DDR5", 35 => "LPDDR5",
            _ => null
        };
        var speed = modules.Max(m => ToLong(m["ConfiguredClockSpeed"]));
        if (speed <= 0) speed = modules.Max(m => ToLong(m["Speed"]));

        // Старые прошивки отдают частоту в МГц (3000 вместо 6000 МТ/с); DDR4 медленнее 1600, а DDR5 — 4000 МТ/с не бывает.
        if (speed > 0 && ((type == "DDR5" && speed < 4000) || (type == "DDR4" && speed < 1600))) speed *= 2;

        var layout = sizes.Distinct().Count() == 1
            ? $"{sizes.Count} × {sizes[0]:0} ГБ"
            : string.Join(" + ", sizes.Select(s => $"{s:0}")) + " ГБ";
        var parts = new List<string> { layout };
        if (speed > 0 && type is null) parts.Add($"{speed} МТ/с");

        var model = modules
            .Select(m => $"{(m["Manufacturer"] as string)?.Trim()} {(m["PartNumber"] as string)?.Trim()}".Trim())
            .Where(s => s.Length > 0 && !s.Contains("Unknown", StringComparison.OrdinalIgnoreCase))
            .Distinct()
            .ToList();
        if (model.Count > 0) parts.Add(string.Join(", ", model));

        // «32 ГБ DDR5-6000» — частота, на которой память работает сейчас (с EXPO/XMP — разогнанная).
        var title = $"{sizes.Sum():0} ГБ" + (type is null ? "" : " " + type + (speed > 0 ? $"-{speed}" : ""));
        return new SpecRow("Оперативная память", title, string.Join(" · ", parts));
    }

    private static SpecRow ReadWindows()
    {
        const string path = @"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
        var name = Reg(path, "ProductName") ?? "Windows";
        var build = Reg(path, "CurrentBuildNumber");

        // В реестре Windows 11 до сих пор называется «Windows 10 …» — отличаем по номеру сборки.
        if (int.TryParse(build, out var number) && number >= 22000)
            name = name.Replace("Windows 10", "Windows 11", StringComparison.OrdinalIgnoreCase);

        var ubr = ToLong(RegValue(path, "UBR"));
        var version = Reg(path, "DisplayVersion");
        var detail = string.Join(" · ", new[]
        {
            version,
            build is null ? null : "сборка " + build + (ubr > 0 ? "." + ubr : "")
        }.Where(s => s is not null));
        return new SpecRow("Windows", name, detail.Length > 0 ? detail : null);
    }

    private static string? ShortVendor(string? vendor) => vendor switch
    {
        null => null,
        _ when vendor.Contains("ASUSTeK", StringComparison.OrdinalIgnoreCase) => "ASUS",
        _ when vendor.Contains("Micro-Star", StringComparison.OrdinalIgnoreCase) => "MSI",
        _ when vendor.Contains("Gigabyte", StringComparison.OrdinalIgnoreCase) => "Gigabyte",
        _ => vendor.Trim()
    };

    private static string? Reg(string path, string name) => (RegValue(path, name) as string)?.Trim() is { Length: > 0 } s ? s : null;

    private static object? RegValue(string path, string name)
    {
        try
        {
            using var key = Registry.LocalMachine.OpenSubKey(path);
            return key?.GetValue(name);
        }
        catch (Exception ex)
        {
            QuietLog.Write(ex);
            return null;
        }
    }

    private static long ToLong(object? value) => value switch
    {
        long l => l,
        ulong u => (long)u,
        int i => (uint)i,
        uint u => u,
        ushort s => s,
        byte[] { Length: >= 8 } b => BitConverter.ToInt64(b, 0),
        byte[] { Length: >= 4 } b => BitConverter.ToUInt32(b, 0),
        string s when long.TryParse(s, out var n) => n,
        _ => 0
    };

    /// <summary>WMI-запрос; значения забираем сразу, пока объекты не освобождены.</summary>
    private static List<Dictionary<string, object?>> Query(string wql, params string[] properties)
    {
        var list = new List<Dictionary<string, object?>>();
        try
        {
            using var searcher = new ManagementObjectSearcher(wql);
            using var results = searcher.Get();
            foreach (var item in results)
            {
                using (item)
                {
                    var row = new Dictionary<string, object?>();
                    foreach (var property in properties) row[property] = item[property];
                    list.Add(row);
                }
            }
        }
        catch (Exception ex)
        {
            QuietLog.Write(ex);
        }

        return list;
    }
}
