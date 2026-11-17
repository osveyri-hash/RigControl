namespace RigControl.Mouse;

/// <summary>Состояние мыши из команды 0x11 0x06.</summary>
public sealed record MouseStatus(ushort Vid, ushort Pid, uint Firmware, int ConnectMode, bool Online,
    int Battery, int Charge, string Raw)
{
    /// <summary>Мышь на связи (донгл без мыши отдаёт pid 0 и заряд 0).</summary>
    public bool HasMouse => Pid != 0 && Battery is > 0 and <= 100;
    public bool Charging => Charge != 0;
}

/// <summary>Назначение кнопки: тип и значение (3 байта, как в M HUB). Тип 0 — родная функция кнопки.</summary>
public readonly record struct MouseKey(int Type, int Value)
{
    public static readonly MouseKey Default = new(0, 0);
    public bool IsLeftClick => Type == 1 && Value == 0x010000;
}

/// <summary>Настройки текущего профиля из команды 0x12 0x67.</summary>
public sealed record MouseConfig(int Profile, int GDpi, int GRate, int UsbDpi, int UsbRate, int Reserved,
    int[] Dpi, int[] DpiY, int Count, bool Valid, string Raw)
{
    /// <summary>Байт «sensor»: биты 0–1 — высота отрыва, 2 — ripple, 3 — линейная коррекция, 4 — motion sync, 6–7 — режим.</summary>
    public int Sensor { get; init; }
    public int Debounce { get; init; }
    /// <summary>Минуты до сна, 0 — не засыпать.</summary>
    public int Sleep { get; init; }
    /// <summary>Угол сенсора, −30…30.</summary>
    public int Rotate { get; init; }
    /// <summary>Кнопки: 0 — левая, 1 — средняя, 2 — правая, 3 — «вперёд», 4 — «назад».</summary>
    public MouseKey[] Keys { get; init; } = new MouseKey[6];

    /// <summary>Высота отрыва: 0 — 0,7 мм, 1 — 1 мм, 2 — 2 мм.</summary>
    public int Lod => (Sensor & 3) switch { 1 => 1, 2 => 2, _ => 0 };
    public bool Ripple => (Sensor >> 2 & 1) != 0;
    public bool AngleSnap => (Sensor >> 3 & 1) != 0;
    public bool MotionSync => (Sensor >> 4 & 1) != 0;
    /// <summary>0 — «Мощность», 1 — «Игровой», 2 — «Супер».</summary>
    public int Mode => (Sensor >> 7 & 1) == 0 ? 0 : (Sensor >> 6 & 1) == 0 ? 1 : 2;
    /// <summary>У уровней своя чувствительность по оси Y.</summary>
    public bool SeparateY => Enumerable.Range(0, Count).Any(i => DpiY[i] != Dpi[i]);
}

/// <summary>Канал feature-отчётов: настоящий HID-интерфейс или демо-мышь.</summary>
internal interface IFeatureLink : IDisposable
{
    ushort VendorId { get; }
    ushort ProductId { get; }
    ushort UsagePage { get; }
    int FeatureReportLength { get; }
    void SetFeature(byte reportId, byte[] payload);
    byte[] GetFeature(byte reportId, int length);
}

internal sealed class HidLink(HidInterface hid) : IFeatureLink
{
    public ushort VendorId => hid.VendorId;
    public ushort ProductId => hid.ProductId;
    public ushort UsagePage => hid.UsagePage;
    public int FeatureReportLength => hid.FeatureReportLength;
    public void SetFeature(byte reportId, byte[] payload) => hid.SetFeatureReport(reportId, payload, 0xFF);
    public byte[] GetFeature(byte reportId, int length) => hid.GetFeatureReport(reportId, length);
    public void Dispose() => hid.Dispose();
}

/// <summary>
/// Мыши MCHOSE (A7 / A7 Pro / A7 Ultra, L7, M7 …) — протокол из официального веб-драйвера M HUB:
/// feature-отчёты 0x11 (20 байт) и 0x12 (64 байта), каждый байт данных инвертирован (XOR 0xFF).
/// Запрос: [id, ~команда, ~данные…, 0xFF…]; ответ на чтение: [id, ~команда, ~данные…].
/// </summary>
public sealed class McMouse : IDisposable
{
    private const byte ShortReport = 0x11;
    private const byte LongReport = 0x12;
    private const int ShortLength = 20;
    private const int LongLength = 64;

    /// <summary>Устройства «старого» протокола MCHOSE (фильтры ryMouseFilters из M HUB).</summary>
    private static readonly ushort[] MchosePids =
    {
        0x1001, 0x1009, 0x100A, 0x100B, 0x100C, 0x4010, 0x4011, 0x4015, 0x4016, 0x4018, 0x4019,
        0x401B, 0x4021, 0x4022, 0x4023, 0x4110, 0x4150
    };

    /// <summary>Донглы 2,4 ГГц с частотой до 1000 Гц (у A7 Ultra — 0x1021), остальные — до 8000.</summary>
    private static readonly ushort[] Dongles1K = { 0x1021 };
    private static readonly ushort[] Dongles = { 0x1020, 0x1021, 0x1001, 0x1009, 0x100A, 0x100B, 0x100C };
    private static readonly ushort[] UltraPids = { 0x0011, 0x1020, 0x1021, 0x4110 };

    public static readonly int[] Rates8K = { 125, 500, 1000, 2000, 4000, 8000 };
    public static readonly int[] Rates1K = { 125, 500, 1000 };

    private readonly IFeatureLink _link;
    private readonly object _sync = new();
    private bool _disposed;

    private McMouse(IFeatureLink link, string name)
    {
        _link = link;
        Name = name;
    }

    public string Name { get; }
    public ushort ProductId => _link.ProductId;
    public ushort VendorId => _link.VendorId;
    public string Interface => $"{_link.VendorId:X4}:{_link.ProductId:X4} 0x{_link.UsagePage:X4} feat {_link.FeatureReportLength}";

    /// <summary>Подключено через донгл (по USB-ID интерфейса).</summary>
    public bool ViaDongle => Dongles.Contains(_link.ProductId);

    /// <summary>Частоты опроса для текущего подключения.</summary>
    public int[] Rates => ViaDongle && Dongles1K.Contains(_link.ProductId) ? Rates1K : Rates8K;

    public bool IsUltra(MouseStatus? status) =>
        UltraPids.Contains(_link.ProductId) || (status is not null && UltraPids.Contains(status.Pid));

    /// <summary>Потолок DPI: у A7 Ultra — 42000, у остальных A7 — 26000 (по инструкции).</summary>
    public int MaxDpi(MouseStatus? status) => IsUltra(status) ? 42000 : 26000;

    /// <summary>Заводские уровни DPI (как кнопка «Сброс» в M HUB).</summary>
    public int[] DefaultDpi(MouseStatus? status) => new[] { 400, 800, 1600, 3200, 6400, MaxDpi(status) };

    /// <summary>Модель по USB-ID (как в M HUB), иначе — имя из USB.</summary>
    public string Model(MouseStatus? status)
    {
        if (IsUltra(status)) return "A7 Ultra";
        var name = Name.Replace("MCHOSE", "", StringComparison.OrdinalIgnoreCase).Trim();
        return name.Length == 0 || name.Contains("dongle", StringComparison.OrdinalIgnoreCase)
                                || name.Contains("receiver", StringComparison.OrdinalIgnoreCase) ? "Мышь MCHOSE" : name;
    }

    public static bool IsCandidate(HidInterface i) =>
        (i.VendorId == 0x5253 || (i.VendorId == 0x3837 && MchosePids.Contains(i.ProductId))
         || (i.VendorId == 0x41E4 && i.ProductId is 0x1101 or 0x1102) || (i.VendorId == 0x0BDA && i.ProductId == 0x4762))
        && i.UsagePage is 0xFF01 or 0xFF0B
        && i.FeatureReportLength >= ShortLength + 1;

    /// <summary>По пути устройства (…vid_5253&pid_1020…) — чтобы не открывать помпу, клавиатуру и прочее.</summary>
    private static bool PathMayMatch(string path)
    {
        var p = path.ToLowerInvariant();
        return p.Contains("vid_5253") || p.Contains("vid_3837") || p.Contains("vid_41e4") || p.Contains("vid_0bda");
    }

    /// <summary>Ищет мышь: интерфейс, который отвечает на запрос состояния. log — что нашли (для диагностики).</summary>
    public static McMouse? Find(out string log)
    {
        var lines = new List<string>();
        List<HidInterface> found;
        try { found = HidInterface.EnumerateMatching(IsCandidate, PathMayMatch); }
        catch (Exception ex)
        {
            log = "Поиск: " + ex.Message;
            return null;
        }

        McMouse? best = null;
        MouseStatus? bestStatus = null;

        foreach (var hid in found)
        {
            string name;
            try { name = hid.ProductName() ?? ""; } catch { name = ""; }
            var probe = new McMouse(new HidLink(hid), string.IsNullOrWhiteSpace(name) ? "Мышь MCHOSE" : name);

            MouseStatus? status = null;
            string? error = null;
            try { status = probe.ReadStatus(); }
            catch (Exception ex) { error = ex.Message; }
            lines.Add($"{probe.Interface} «{name}»: {status?.Raw ?? error ?? "нет ответа"}");

            // Лучше тот, через который видно саму мышь (заряд); иначе — первый ответивший.
            var better = status is not null && (best is null || (status.HasMouse && bestStatus?.HasMouse != true));
            if (better)
            {
                best?.Dispose();
                best = probe;
                bestStatus = status;
            }
            else
            {
                probe.Dispose();
            }
        }

        log = lines.Count == 0 ? "Подходящих HID-интерфейсов MCHOSE нет." : string.Join("\n", lines);
        return best;
    }

    public MouseStatus? ReadStatus()
    {
        var d = Read(ShortReport, ShortLength, 0x06);
        if (d is null) return null;

        var flags = d[8];
        return new MouseStatus(
            (ushort)(d[0] | d[1] << 8), (ushort)(d[2] | d[3] << 8),
            (uint)(d[4] | d[5] << 8 | d[6] << 16 | d[7] << 24),
            flags & 0x07, (flags >> 3 & 1) != 0, d[9], d[10], Hex(d, 12));
    }

    /// <summary>Версия прошивки мыши или приёмника (0x11 0x04 00 / AA) — как «Версия ПО» в M HUB.</summary>
    public string? ReadVersion(bool receiver)
    {
        var d = Read(ShortReport, ShortLength, 0x04, receiver ? (byte)0xAA : (byte)0x00);
        if (d is null || d[0] is 0 or > 16) return null;
        var chars = d.Skip(1).Take(d[0]).TakeWhile(b => b != 0).Select(b => (char)b).ToArray();
        if (chars.Length == 0 || chars.Any(c => c < 0x20 || c > 0x7E)) return null;
        var text = new string(chars).Trim();
        return text.Length > 0 && char.IsDigit(text[0]) ? "V" + text : text;
    }

    public MouseConfig? ReadConfig()
    {
        var d = Read(LongReport, LongLength, 0x67);
        if (d is null || d.Length < 63) return null;

        var dpi = new int[6];
        var dpiY = new int[6];
        for (var i = 0; i < 6; i++)
        {
            dpi[i] = d[4 + i * 2] | d[5 + i * 2] << 8;
            dpiY[i] = d[51 + i * 2] | d[52 + i * 2] << 8;
            if (dpiY[i] is < 50 or > 50000) dpiY[i] = dpi[i];
        }

        // Кнопки: 6 ячеек по 4 байта — [индекс << 4 | тип, значение (3 байта)].
        var keys = new MouseKey[6];
        for (var i = 0; i < 6; i++)
        {
            var at = 20 + i * 4;
            keys[i] = new MouseKey(d[at] & 0x0F, d[at + 1] << 16 | d[at + 2] << 8 | d[at + 3]);
        }

        var count = d[16];
        // [1] — кабель (частота в старшей половине, уровень DPI в младшей), [2] — радио 2.4G.
        var usbDpi = d[1] & 0x0F;
        var gDpi = d[2] & 0x0F;
        var valid = count is >= 1 and <= 6 && gDpi < 6 && usbDpi < 6
                    && dpi.Take(count).All(v => v is >= 50 and <= 50000);

        return new MouseConfig(d[0], gDpi, d[2] >> 4, usbDpi, d[1] >> 4, d[3], dpi, dpiY, count, valid, Hex(d, 24))
        {
            Sensor = d[17],
            Debounce = d[18],
            Sleep = d[19],
            Rotate = d[49] > 127 ? d[49] - 256 : d[49],
            Keys = keys,
        };
    }

    /// <summary>Уровни DPI (X и Y) и выбранный уровень — «0x12 0x40», как в M HUB: один уровень для кабеля и радио.</summary>
    public void WriteDpi(MouseConfig config, int[] dpi, int[] dpiY, int count, int selected)
    {
        count = Math.Clamp(count, 1, 6);
        selected = Math.Clamp(selected, 0, count - 1);

        var data = new List<byte> { 0x40, (byte)selected, (byte)selected, (byte)config.Reserved };
        for (var i = 0; i < 6; i++) AddLe(data, Math.Clamp(dpi[i], 50, 50000));
        data.Add((byte)count);
        data.Add(0xFF);   // diff — всегда 0xFF, как в M HUB
        for (var i = 0; i < 6; i++) AddLe(data, Math.Clamp(dpiY[i], 50, 50000));

        Write(LongReport, LongLength, data);
    }

    /// <summary>Частота опроса (индекс в списке частот). По радио — второй байт, по кабелю — первый (как в M HUB).</summary>
    public void WriteRate(int index, bool wireless) =>
        Write(ShortReport, ShortLength, new List<byte> { 0x41, (byte)(wireless ? 0 : index), (byte)(wireless ? index : 0) });

    /// <summary>
    /// Команда 0x11 0x42 (как в M HUB): высота отрыва, переключатели (1 — вкл, 2 — выкл, 0 — не трогать),
    /// режим (1 — мощность, 2 — игровой, 3 — супер, 0 — не трогать), поворот сенсора.
    /// </summary>
    public void WriteSensor(int lod, int ripple = 0, int line = 0, int motionSync = 0, int mode = 0, int? rotate = null)
    {
        var data = new List<byte>
        {
            0x42, (byte)lod, (byte)ripple, (byte)line, (byte)motionSync, 0, 0, (byte)mode,
            (byte)(rotate is null ? 0 : 1), (byte)(sbyte)Math.Clamp(rotate ?? 0, -30, 30),
        };
        Write(ShortReport, ShortLength, data);
    }

    /// <summary>Время отключения кнопки (антидребезг), мс.</summary>
    public void WriteDebounce(int ms) =>
        Write(ShortReport, ShortLength, new List<byte> { 0x43, (byte)Math.Clamp(ms, 0, 20) });

    /// <summary>Сон через minutes минут; 0 — не засыпать.</summary>
    public void WriteSleep(int minutes) =>
        Write(ShortReport, ShortLength, new List<byte> { 0x0A, (byte)(minutes > 0 ? 1 : 0), (byte)Math.Clamp(minutes, 0, 255) });

    /// <summary>Назначить кнопке функцию — «0x12 0x52» [кнопка, 0, тип, значение]. Тип 0 — вернуть родную.</summary>
    public void WriteKey(int button, MouseKey key) =>
        Write(LongReport, LongLength, new List<byte>
        {
            0x52, (byte)button, 0, (byte)key.Type,
            (byte)(key.Value >> 16 & 0xFF), (byte)(key.Value >> 8 & 0xFF), (byte)(key.Value & 0xFF),
        });

    /// <summary>Все кнопки текущего профиля — как с завода («0x11 0x0B», 0x0101).</summary>
    public void ResetKeys() => Write(ShortReport, ShortLength, new List<byte> { 0x0B, 0x01, 0x01 });

    /// <summary>Заводские настройки текущего профиля («0x11 0x0B», 0xAA00).</summary>
    public void FactoryReset() => Write(ShortReport, ShortLength, new List<byte> { 0x0B, 0xAA, 0x00 });

    /// <summary>Встроенный профиль 0…2 («0x11 0x58»).</summary>
    public void SwitchProfile(int index) =>
        Write(ShortReport, ShortLength, new List<byte> { 0x58, (byte)Math.Clamp(index, 0, 2) });

    private static void AddLe(List<byte> data, int value)
    {
        data.Add((byte)(value & 0xFF));
        data.Add((byte)(value >> 8 & 0xFF));
    }

    private void Write(byte reportId, int length, List<byte> data)
    {
        var payload = new byte[length];
        Array.Fill(payload, (byte)0xFF);
        for (var i = 0; i < data.Count && i < length; i++) payload[i] = (byte)~data[i];

        lock (_sync)
        {
            ObjectDisposedException.ThrowIf(_disposed, this);
            _link.SetFeature(reportId, payload);
            Thread.Sleep(30);
        }
    }

    /// <summary>Запрос чтения и ответ: данные уже без инверсии (после [id, команда]).</summary>
    private byte[]? Read(byte reportId, int length, byte command, params byte[] args)
    {
        var payload = new byte[length];
        Array.Fill(payload, (byte)0xFF);
        payload[0] = (byte)~command;
        for (var i = 0; i < args.Length && i + 1 < length; i++) payload[i + 1] = (byte)~args[i];

        lock (_sync)
        {
            ObjectDisposedException.ThrowIf(_disposed, this);
            _link.SetFeature(reportId, payload);

            for (var attempt = 0; attempt < 8; attempt++)
            {
                Thread.Sleep(attempt == 0 ? 20 : 40);
                byte[] reply;
                try { reply = _link.GetFeature(reportId, length); }
                catch { continue; }

                if (reply.Length < 3 || reply[0] != reportId || (byte)~reply[1] != command) continue;

                var data = new byte[reply.Length - 2];
                for (var i = 0; i < data.Length; i++) data[i] = (byte)~reply[i + 2];
                return data;
            }
        }

        return null;
    }

    private static string Hex(byte[] data, int count) =>
        Convert.ToHexString(data, 0, Math.Min(count, data.Length));

    public void Dispose()
    {
        lock (_sync)
        {
            if (_disposed) return;
            _disposed = true;
            _link.Dispose();
        }
    }
}
