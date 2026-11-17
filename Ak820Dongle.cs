using System.IO;
using System.Threading;

namespace RigControl;

/// <summary>
/// Клавиатура по радио: донгл 2,4 ГГц (USB 0C45:FDFD) отдаёт вендорский HID-интерфейс 0xFF60 с 32-байтными отчётами.
/// Формат разобран по захвату USB-трафика официальной программы: [команда, …, контрольная сумма],
/// контрольная сумма — сумма первых 31 байта по модулю 256.
/// Работают синхронизация часов и подсветка; экран (картинка/GIF) — только по кабелю.
/// </summary>
public sealed class Ak820Dongle : IDisposable
{
    public const ushort ProductId = 0xFDFD;
    public const ushort UsagePage = 0xFF60;
    private const int PacketSize = 32;

    private readonly HidInterface _hid;

    /// <summary>Одна команда за раз: иначе ответы перепутываются, и команды теряются.</summary>
    private readonly object _sync = new();
    private bool _disposed;

    /// <summary>Донгл хоть раз подтвердил пакет подсветки — значит, отсутствие ответа означает потерю пакета.</summary>
    private bool _lightAckSeen;

    public string Description => $"Донгл 2,4 ГГц {_hid.VendorId:X4}:{_hid.ProductId:X4}";

    private Ak820Dongle(HidInterface hid) => _hid = hid;

    public static Ak820Dongle? TryOpen()
    {
        try
        {
            var interfaces = HidInterface.Enumerate(Ak820Protocol.VendorId, ProductId);
            var raw = interfaces.FirstOrDefault(i => i.UsagePage == UsagePage);

            foreach (var item in interfaces)
            {
                if (!ReferenceEquals(item, raw)) item.Dispose();
            }

            return raw is null ? null : new Ak820Dongle(raw);
        }
        catch
        {
            return null;
        }
    }

    private static byte[] Packet(params byte[] head)
    {
        var packet = new byte[PacketSize];
        head.CopyTo(packet, 0);
        FinishChecksum(packet);
        return packet;
    }

    private static void FinishChecksum(byte[] packet)
    {
        var sum = 0;
        for (var i = 0; i < PacketSize - 1; i++) sum += packet[i];
        packet[PacketSize - 1] = (byte)(sum & 0xFF);
    }

    /// <summary>Отправляет пакет и ждёт ответ с той же командой (лишние отчёты пропускает).</summary>
    private void Exchange(byte[] packet)
    {
        Drain();
        _hid.WriteOutput(packet);

        var size = _hid.InputReportLength > 0 ? _hid.InputReportLength : PacketSize + 1;
        for (var attempt = 0; attempt < 4; attempt++)
        {
            var report = _hid.ReadReport(size, 500);
            if (report is null) return;
            Notice(report);

            // Первый байт — Report ID, дальше сама команда.
            if (report.Length > 1 && report[1] == packet[0]) return;
        }
    }

    /// <summary>
    /// Выкидывает накопившиеся отчёты (нажатия, ответы на прошлые команды): иначе следующий ответ
    /// читается «со сдвигом» и подтверждение не совпадает с командой.
    /// </summary>
    private static int? LevelOf(byte[] report) =>
        report.Length > 4 && report[1] == 0x20 && report[2] == 0x01 && report[4] is > 0 and var level
            ? Math.Min((int)level, 100)
            : null;

    /// <summary>Заряд, случайно пойманный среди ответов на другие команды (опоздавший ответ 0x20).</summary>
    private int? _strayBattery;

    private void Notice(byte[] report)
    {
        if (LevelOf(report) is { } level) _strayBattery = level;
    }

    private void Drain(List<byte[]>? keep = null)
    {
        var size = _hid.InputReportLength > 0 ? _hid.InputReportLength : PacketSize + 1;
        for (var i = 0; i < 8; i++)
        {
            try
            {
                var report = _hid.ReadReport(size, 1);
                if (report is null) return;
                Notice(report);
                keep?.Add(report);
            }
            catch
            {
                return;
            }
        }
    }

    /// <summary>Ждёт ответ с той же командой; true — пришёл.</summary>
    private bool WaitAck(byte command, int timeoutMs)
    {
        var size = _hid.InputReportLength > 0 ? _hid.InputReportLength : PacketSize + 1;
        var deadline = Environment.TickCount64 + timeoutMs;

        while (Environment.TickCount64 < deadline)
        {
            byte[]? report;
            try { report = _hid.ReadReport(size, (int)Math.Max(1, deadline - Environment.TickCount64)); }
            catch { return false; }

            if (report is null) return false;
            Notice(report);
            if (report.Length > 1 && report[1] == command) return true;
        }

        return false;
    }

    /// <summary>
    /// Заряд батареи, %: запрос 0x20 0x01, как у официальной программы (раз в 15 секунд по радио).
    /// Ответ [Report ID, 0x20, 0x01, 0x00, заряд, …]. null — клавиатура не ответила (спит, выключена).
    /// </summary>
    public int? ReadBattery()
    {
        lock (_sync)
        {
            ObjectDisposedException.ThrowIf(_disposed, this);

            var seen = new List<string>();
            int? result = null;

            void See(byte[] report)
            {
                var hex = Convert.ToHexString(report, 0, Math.Min(report.Length, 33));
                if (!seen.Contains(hex)) seen.Add(hex);
            }

            // Ответ с зарядом может прийти не сразу, а «вторым» — после того, как донгл спросит
            // клавиатуру. Официальная программа очередь не чистит и подхватывает его при следующем
            // запросе; мы смотрим и в накопившееся, и ждём подольше после первого подтверждения.
            var pending = new List<byte[]>();
            result = _strayBattery;
            _strayBattery = null;
            Drain(pending);
            foreach (var report in pending)
            {
                if (report.Length > 1 && report[1] == 0x20) See(report);
                result ??= LevelOf(report);
            }
            if (result is not null) seen.Add("из очереди");

            var answered = false;
            for (var attempt = 0; attempt < 2 && result is null && !answered; attempt++)
            {
                _hid.WriteOutput(Packet(0x20, 0x01));

                var size = _hid.InputReportLength > 0 ? _hid.InputReportLength : PacketSize + 1;
                var deadline = Environment.TickCount64 + 1000;
                while (Environment.TickCount64 < deadline && result is null)
                {
                    var report = _hid.ReadReport(size, (int)Math.Max(1, deadline - Environment.TickCount64));
                    if (report is null) break;
                    if (report.Length > 1 && report[1] == 0x20)
                    {
                        See(report);
                        // Ответ без заряда — ждём ещё чуть-чуть: вдруг следом придёт настоящий.
                        if (!answered) deadline = Math.Min(deadline, Environment.TickCount64 + 300);
                        answered = true;
                    }
                    result = LevelOf(report);
                }
            }

            LogBattery(result is null
                ? "нет заряда в ответе: " + (seen.Count > 0 ? string.Join(" | ", seen) : "ответов нет")
                : $"{result} % ({string.Join(" | ", seen)})");
            return result;
        }
    }

    private static string? _lastBatteryLog;

    /// <summary>%AppData%\RigControl\battery.log — что ответила клавиатура (пишем только изменения).</summary>
    private static void LogBattery(string text)
    {
        if (text == _lastBatteryLog) return;
        _lastBatteryLog = text;

        try
        {
            LogFile.Append("battery.log", $"{DateTime.Now:dd.MM HH:mm:ss} {text}{Environment.NewLine}",
                maxBytes: 100_000);
        }
        catch
        {
            // журнал не обязателен
        }
    }

    /// <summary>Часы клавиатуры: как в официальной программе — два запроса состояния и пакет времени 0x0C.</summary>
    public void SyncTime(DateTime now)
    {
        lock (_sync)
        {
            ObjectDisposedException.ThrowIf(_disposed, this);
            SyncTimeCore(now);
        }
    }

    private void SyncTimeCore(DateTime now)
    {
        Exchange(Packet(0x02));
        Thread.Sleep(20);
        Exchange(Packet(0x20, 0x01));
        Thread.Sleep(20);

        var time = new byte[PacketSize];
        time[0] = 0x0C;
        time[1] = 0x10;
        time[5] = 0x5A;
        time[6] = (byte)(now.Year % 2000);
        time[7] = (byte)now.Month;
        time[8] = (byte)now.Day;
        time[9] = (byte)now.Hour;
        time[10] = (byte)now.Minute;
        time[11] = (byte)now.Second;
        time[13] = (byte)((int)now.DayOfWeek % 7);   // воскресенье = 0
        time[17] = 0xAA;
        time[18] = 0x55;
        FinishChecksum(time);

        Exchange(time);
    }

    /// <summary>
    /// Подсветка по радио. Формат снят с захвата: [05 10 00] + 16 байт проводного
    /// пакета (режим, RGB, радуга, яркость, скорость, направление, AA 55) + хвост
    /// нулей, контрольная сумма в байте 31.
    /// </summary>
    public void SendLight(int mode, byte r, byte g, byte b, bool rainbow,
        int brightness = 5, int speed = 3, int direction = 0)
    {
        var packet = new byte[PacketSize];
        packet[0] = 0x05;
        packet[1] = 0x10;
        packet[2] = 0x00;
        packet[3] = (byte)mode;

        if (mode != 0)
        {
            packet[4] = r;
            packet[5] = g;
            packet[6] = b;
            packet[11] = (byte)(rainbow ? 1 : 0);
            packet[12] = (byte)Math.Clamp(brightness, 0, 5);
            packet[13] = (byte)Math.Clamp(speed, 0, 5);
            packet[14] = (byte)Math.Clamp(direction, 0, 3);
        }

        packet[17] = 0xAA;
        packet[18] = 0x55;
        FinishChecksum(packet);

        lock (_sync)
        {
            ObjectDisposedException.ThrowIf(_disposed, this);

            // По радио пакет иногда теряется (клавиатура только проснулась, помехи).
            // Если донгл умеет подтверждать подсветку, без подтверждения повторяем;
            // если не умеет — шлём один раз, чтобы не перезапускать анимацию впустую.
            for (var attempt = 0; attempt < 3; attempt++)
            {
                Drain();
                _hid.WriteOutput(packet);

                // Каждый пакет по радио начинает анимацию заново — и часы схемы/вентиляторов тоже.
                if (mode != KeyPaint.UserLightMode)
                {
                    if (attempt == 0) LightMirror.Effect(mode, r, g, b, rainbow, brightness, speed, direction, restart: true);
                    else LightMirror.RestartEffectClock();
                }

                if (WaitAck(packet[0], _lightAckSeen ? 250 : 150))
                {
                    _lightAckSeen = true;
                    return;
                }

                if (!_lightAckSeen) return;
                Thread.Sleep(40);
            }
        }
    }

    /// <summary>
    /// Пользовательская подсветка по радио — точно как в USB-логе официальной программы:
    /// пакет подсветки с режимом 0x80, затем таблица цветов (576 байт + 12 нулей = 21 порция по 28 байт)
    /// пакетами [14 1C номер, 28 байт, контрольная сумма]. На каждую порцию донгл отвечает тем же
    /// заголовком — ждём ответ и при потере повторяем порцию.
    /// </summary>
    public void SetUserLight(byte[] frame)
    {
        const int part = 28;
        var blob = new byte[21 * part];
        frame.AsSpan(0, Math.Min(frame.Length, blob.Length)).CopyTo(blob);

        lock (_sync)
        {
            ObjectDisposedException.ThrowIf(_disposed, this);

            SendLight(KeyPaint.UserLightMode, 0, 0, 0, false, 5, 0, 0);
            Thread.Sleep(150);

            for (var i = 0; i < 21; i++)
            {
                var packet = new byte[PacketSize];
                packet[0] = 0x14;
                packet[1] = part;
                packet[2] = (byte)i;
                blob.AsSpan(i * part, part).CopyTo(packet.AsSpan(3));
                FinishChecksum(packet);

                var ok = false;
                for (var attempt = 0; attempt < 3 && !ok; attempt++)
                {
                    Drain();
                    _hid.WriteOutput(packet);
                    ok = WaitPart((byte)i, 300);
                }

                if (!ok) throw new IOException($"Донгл не подтвердил порцию раскраски {i + 1} из 21.");
            }

            LightMirror.UserLight(frame);
        }
    }

    /// <summary>Ответ на порцию таблицы: [Report ID, 14, 1C, номер, …].</summary>
    private bool WaitPart(byte index, int timeoutMs)
    {
        var size = _hid.InputReportLength > 0 ? _hid.InputReportLength : PacketSize + 1;
        var deadline = Environment.TickCount64 + timeoutMs;

        while (Environment.TickCount64 < deadline)
        {
            byte[]? report;
            try { report = _hid.ReadReport(size, (int)Math.Max(1, deadline - Environment.TickCount64)); }
            catch { return false; }

            if (report is null) return false;
            Notice(report);
            if (report.Length > 3 && report[1] == 0x14 && report[3] == index) return true;
        }

        return false;
    }

    public void Dispose()
    {
        lock (_sync)
        {
            if (_disposed) return;
            _disposed = true;
            _hid.Dispose();
        }
    }
}
