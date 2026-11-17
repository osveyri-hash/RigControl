using System.Text;

namespace RigControl;

/// <summary>
/// USB-контроллер подсветки материнских плат ASUS (AURA, «AULA3-…»): VID 0x0B05, usage page 0xFF72,
/// 65-байтные отчёты с Report ID 0xEC. Через него горят разъёмы ARGB/RGB на плате.
/// Протокол — как в OpenRGB (Asus Aura USB / Mainboard): прямой режим 0x40 — цвета каждого светодиода
/// от программы, по 20 штук в отчёте.
/// </summary>
public sealed class AuraUsb : IDisposable
{
    public const ushort Vid = 0x0B05;
    private const ushort UsagePage = 0xFF72;
    private const byte ReportId = 0xEC;
    private const int LedsPerReport = 20;

    private readonly HidInterface _hid;

    /// <summary>Строка прошивки, например «AULA3-AR32-0207».</summary>
    public string Firmware { get; }

    /// <summary>Сколько адресных (ARGB) разъёмов на плате, по таблице контроллера.</summary>
    public int ArgbHeaders { get; }

    /// <summary>Светодиоды самой платы и 12-вольтовые RGB-разъёмы (байт 0x1B таблицы).</summary>
    public int MainboardLeds { get; }

    /// <summary>Таблица настроек контроллера (60 байт) — для журнала.</summary>
    public byte[] ConfigTable { get; }

    public ushort ProductId => _hid.ProductId;

    private AuraUsb(HidInterface hid, string firmware, byte[] table)
    {
        _hid = hid;
        Firmware = firmware;
        ConfigTable = table;
        ArgbHeaders = table.Length > 2 && table[2] is >= 1 and <= 8 ? table[2] : 1;
        MainboardLeds = table.Length > 0x1B && table[0x1B] <= 12 ? table[0x1B] : 0;
    }

    /// <summary>Ищет контроллер; null — на плате его нет (или подсветка на SMBus, а не на USB).</summary>
    public static AuraUsb? Find()
    {
        var candidates = HidInterface.EnumerateUsagePage(Vid, UsagePage)
            .OrderByDescending(i => i.OutputReportLength == 65)
            .ToList();

        AuraUsb? found = null;

        foreach (var hid in candidates)
        {
            if (found is null && hid.OutputReportLength >= 65)
            {
                try
                {
                    var firmware = ReadFirmware(hid);
                    if (firmware is not null)
                    {
                        found = new AuraUsb(hid, firmware, ReadConfigTable(hid));
                        continue;
                    }
                }
                catch (Exception ex)
                {
                    // Не отвечает — пробуем следующий интерфейс.
                    QuietLog.Write(ex);
                }
            }

            hid.Dispose();
        }

        return found;
    }

    private static string? ReadFirmware(HidInterface hid)
    {
        var reply = Request(hid, 0x82, 0x02);
        if (reply is null) return null;

        var text = Encoding.ASCII.GetString(reply, 2, Math.Min(16, reply.Length - 2)).TrimEnd('\0', ' ');
        return text.Length > 0 ? text : "ASUS Aura";
    }

    /// <summary>Таблица настроек (0xB0): байт 2 — число ARGB-разъёмов, 0x1B — светодиодов платы.</summary>
    private static byte[] ReadConfigTable(HidInterface hid)
    {
        try
        {
            var reply = Request(hid, 0xB0, 0x30);
            if (reply is null || reply.Length < 4 + 0x1C) return Array.Empty<byte>();
            return reply[4..Math.Min(reply.Length, 64)];
        }
        catch
        {
            return Array.Empty<byte>();
        }
    }

    /// <summary>Запрос и ответ: ответ начинается с 0xEC и кода <paramref name="answer"/>.</summary>
    private static byte[]? Request(HidInterface hid, byte command, byte answer)
    {
        var report = new byte[65];
        report[0] = ReportId;
        report[1] = command;
        hid.WriteReport(report);

        for (var attempt = 0; attempt < 4; attempt++)
        {
            var reply = hid.ReadReport(Math.Max(65, (int)hid.InputReportLength), 400);
            if (reply is null) return null;
            if (reply.Length > 2 && reply[0] == ReportId && reply[1] == answer) return reply;
        }

        return null;
    }

    /// <summary>Канал прямого режима для светодиодов платы и RGB-разъёма 12 В.</summary>
    public const int FixedDirectChannel = 0x04;

    /// <summary>
    /// Прямой режим (эффект 0xFF): дальше цвета задаёт программа.
    /// Каналы эффектов: 0 — плата и RGB 12 В, 1… — ARGB-разъёмы.
    /// </summary>
    public void EnterDirect(IEnumerable<int> effectChannels)
    {
        foreach (var channel in effectChannels)
            Send(0x35, (byte)channel, 0x00, 0x00, 0xFF);
        Send(0x3F, 0x55);
    }

    /// <summary>
    /// Встроенный эффект платы (0 — выкл, 1 — цвет, 2 — дыхание, 3 — вспышки, 4 — смена цветов, 5 — радуга…):
    /// его крутит сам контроллер, программа нужна только чтобы включить.
    /// </summary>
    public void SetEffect(IEnumerable<int> effectChannels, byte mode, byte r, byte g, byte b)
    {
        foreach (var channel in effectChannels)
        {
            Send(0x35, (byte)channel, 0x00, 0x00, mode);

            // Цвет: маска светодиодов в общей таблице (сначала светодиоды платы, затем по одному на ARGB-разъём).
            int start, count;
            if (channel == 0)
            {
                start = 0;
                count = Math.Max(1, MainboardLeds);
            }
            else
            {
                start = MainboardLeds + channel - 1;
                count = 1;
            }

            if (start + count > 16) continue;

            var report = new byte[65];
            report[0] = ReportId;
            report[1] = 0x36;
            var mask = (ushort)(((1 << count) - 1) << start);
            report[2] = (byte)(mask >> 8);
            report[3] = (byte)mask;
            report[4] = 0x00;
            for (var i = start; i < start + count; i++)
            {
                report[5 + i * 3] = r;
                report[6 + i * 3] = g;
                report[7 + i * 3] = b;
            }
            _hid.WriteReport(report);
        }

        Send(0x3F, 0x55);
    }

    /// <summary>Цвета светодиодов разъёма: rgb — по 3 байта (R, G, B) на светодиод.</summary>
    public void SendDirect(int directChannel, ReadOnlySpan<byte> rgb, int count)
    {
        count = Math.Clamp(count, 1, rgb.Length / 3);

        var report = new byte[65];
        for (var start = 0; start < count; start += LedsPerReport)
        {
            var n = Math.Min(LedsPerReport, count - start);
            var last = start + n >= count;

            Array.Clear(report);
            report[0] = ReportId;
            report[1] = 0x40;
            report[2] = (byte)(directChannel | (last ? 0x80 : 0x00));   // 0x80 — «показать»
            report[3] = (byte)start;
            report[4] = (byte)n;
            rgb.Slice(start * 3, n * 3).CopyTo(report.AsSpan(5));
            _hid.WriteReport(report);
        }
    }

    private void Send(params byte[] data)
    {
        var report = new byte[65];
        report[0] = ReportId;
        data.CopyTo(report, 1);
        _hid.WriteReport(report);
    }

    public void Dispose() => _hid.Dispose();
}
