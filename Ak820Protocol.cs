using System.Globalization;

namespace RigControl;

/// <summary>
/// Протокол AJAZZ AK820 Pro, восстановленный из официального драйвера:
/// команды — 64-байтные feature-отчёты (0xFF13), кадры экрана — 4096-байтные
/// output-отчёты (0xFF68). Каждая команда — сессия 04 18 … 04 02.
/// </summary>
public static class Ak820Protocol
{
    public const ushort VendorId = 0x0C45;

    /// <summary>PID проводного режима; 0x800A — вариант прошивки.</summary>
    public static readonly ushort[] ProductIds = { 0x8009, 0x800A };

    public const ushort ControlUsagePage = 0xFF13;
    public const ushort DataUsagePage = 0xFF68;

    public const int ScreenSize = 128;
    public const int Chunk = 4096;
    public const int FrameHeader = 256;
    public const int MaxFrames = 140;

    /// <summary>Пауза между командами — как в официальном конфигураторе (35 мс).</summary>
    public const int CommandDelayMs = 35;

    /// <summary>0x04 &lt;op&gt;, в байте 8 — сколько пакетов данных идёт следом.</summary>
    public static byte[] Command(byte op, int count = 0, byte extra0 = 0)
    {
        var packet = new byte[64];
        packet[0] = 0x04;
        packet[1] = op;
        packet[2] = extra0;
        packet[8] = (byte)(count & 0xFF);
        packet[9] = (byte)(count >> 8);
        return packet;
    }

    /// <summary>Пакет эффекта подсветки: режим, цвет, радуга, яркость, скорость, направление.</summary>
    public static byte[] LightPacket(int mode, byte r, byte g, byte b, bool rainbow,
        int brightness = 5, int speed = 3, int direction = 0)
    {
        var packet = new byte[64];
        packet[0] = (byte)mode;

        if (mode != 0)
        {
            packet[1] = r;
            packet[2] = g;
            packet[3] = b;
            packet[8] = (byte)(rainbow ? 1 : 0);
            packet[9] = (byte)Math.Clamp(brightness, 0, 5);
            packet[10] = (byte)Math.Clamp(speed, 0, 5);
            packet[11] = (byte)Math.Clamp(direction, 0, 3);
        }

        packet[14] = 0xAA;
        packet[15] = 0x55;
        return packet;
    }

    /// <summary>Пакет времени для экрана: год−2000, месяц, день, часы, минуты, секунды, день недели.</summary>
    public static byte[] TimePacket(DateTime now, byte slot = 1)
    {
        var packet = new byte[64];
        packet[1] = slot;
        packet[2] = 0x5A;
        packet[3] = (byte)(now.Year % 2000);
        packet[4] = (byte)now.Month;
        packet[5] = (byte)now.Day;
        packet[6] = (byte)now.Hour;
        packet[7] = (byte)now.Minute;
        packet[8] = (byte)now.Second;
        packet[10] = (byte)((int)now.DayOfWeek % 7); // воскресенье = 0
        packet[62] = 0xAA;
        packet[63] = 0x55;
        return packet;
    }

    /// <summary>
    /// Полотно для экрана: 256-байтный заголовок (число кадров + задержки в 2 мс)
    /// и кадры RGB565 подряд, добитые 0xFF до границы 4 КиБ.
    /// </summary>
    public static byte[] ScreenBlob(IReadOnlyList<ScreenFrame> frames)
    {
        if (frames.Count == 0) throw new ArgumentException("Нет кадров", nameof(frames));

        var count = Math.Min(frames.Count, MaxFrames);

        var header = new byte[FrameHeader];
        Array.Fill(header, (byte)0xFF);
        header[0] = (byte)count;

        for (var i = 0; i < count; i++)
            header[1 + i] = (byte)Math.Clamp(Math.Max(1, frames[i].DelayMs / 2), 1, 255);

        var pixels = Ak820Protocol.ScreenSize * Ak820Protocol.ScreenSize * 2;
        var total = FrameHeader + count * pixels;
        var blob = new byte[(total + Chunk - 1) / Chunk * Chunk];
        Array.Fill(blob, (byte)0xFF);

        header.CopyTo(blob, 0);
        for (var i = 0; i < count; i++)
        {
            var frame = frames[i].Rgb565;
            if (frame.Length != pixels)
                throw new ArgumentException($"Кадр {i}: {frame.Length} байт вместо {pixels}");

            frame.CopyTo(blob, FrameHeader + i * pixels);
        }

        return blob;
    }

    /// <summary>
    /// Средний цвет кадра RGB565. Почти чёрные пиксели пропускаем: у гифок с
    /// чёрным фоном иначе и подсветка была бы чёрной.
    /// </summary>
    public static (byte R, byte G, byte B) AverageColor(ReadOnlySpan<byte> rgb565)
    {
        long red = 0, green = 0, blue = 0;
        var count = 0;

        for (var i = 0; i + 1 < rgb565.Length; i += 2)
        {
            var value = (ushort)(rgb565[i] | (rgb565[i + 1] << 8));
            var pr = (value >> 11) & 0x1F;
            var pg = (value >> 5) & 0x3F;
            var pb = value & 0x1F;

            if (pr + pg + pb < 6) continue; // фон

            red += (pr * 255 + 15) / 31;
            green += (pg * 255 + 31) / 63;
            blue += (pb * 255 + 15) / 31;
            count++;
        }

        if (count == 0) return (0, 0, 0);

        return ((byte)(red / count), (byte)(green / count), (byte)(blue / count));
    }

    /// <summary>
    /// Цвета правого края кадра (экран клавиатуры справа): полосы сверху вниз, в каждой — средний цвет
    /// правых <paramref name="rightFraction"/> кадра. По ним подсветка «продолжает» гифку.
    /// </summary>
    public static (byte R, byte G, byte B)[] StripColors(ReadOnlySpan<byte> rgb565, int rows, double rightFraction = 0.3)
    {
        var result = new (byte R, byte G, byte B)[rows];
        var x0 = (int)(ScreenSize * (1 - rightFraction));

        for (var r = 0; r < rows; r++)
        {
            var y0 = r * ScreenSize / rows;
            var y1 = (r + 1) * ScreenSize / rows;
            long red = 0, green = 0, blue = 0;
            var count = 0;

            for (var y = y0; y < y1; y++)
            {
                for (var x = x0; x < ScreenSize; x++)
                {
                    var i = (y * ScreenSize + x) * 2;
                    if (i + 1 >= rgb565.Length) continue;

                    var value = (ushort)(rgb565[i] | (rgb565[i + 1] << 8));
                    red += (((value >> 11) & 0x1F) * 255 + 15) / 31;
                    green += (((value >> 5) & 0x3F) * 255 + 31) / 63;
                    blue += ((value & 0x1F) * 255 + 15) / 31;
                    count++;
                }
            }

            result[r] = count == 0
                ? ((byte)0, (byte)0, (byte)0)
                : ((byte)(red / count), (byte)(green / count), (byte)(blue / count));
        }

        return result;
    }

    /// <summary>
    /// Делает цвета кадров выразительными: у бледных гифок средний цвет почти
    /// белый, и подсветка выглядит «обычной». Поднимаем насыщенность и
    /// растягиваем яркость кадров на весь диапазон, чтобы было видно ритм.
    /// </summary>
    public static (byte R, byte G, byte B)[] VividPalette(IReadOnlyList<(byte R, byte G, byte B)> colors)
    {
        if (colors.Count == 0) return Array.Empty<(byte R, byte G, byte B)>();

        var hsv = new (double H, double S, double V)[colors.Count];
        for (var i = 0; i < colors.Count; i++)
            hsv[i] = ToHsv(colors[i].R, colors[i].G, colors[i].B);

        var minV = hsv.Min(x => x.V);
        var maxV = hsv.Max(x => x.V);
        var span = Math.Max(0.03, maxV - minV);

        var result = new (byte R, byte G, byte B)[colors.Count];

        for (var i = 0; i < colors.Count; i++)
        {
            var (h, s, v) = hsv[i];

            var vividS = Math.Clamp(s * 2.0 + 0.25, 0, 0.75);
            var vividV = Math.Clamp(0.72 + 0.28 * (v - minV) / span, 0, 1);

            result[i] = FromHsv(h, vividS, vividV);
        }

        return result;
    }

    private static (double H, double S, double V) ToHsv(double r, double g, double b)
    {
        r /= 255; g /= 255; b /= 255;

        var max = Math.Max(r, Math.Max(g, b));
        var min = Math.Min(r, Math.Min(g, b));
        var delta = max - min;

        double h = 0;
        if (delta > 0)
        {
            if (max == r) h = 60 * (((g - b) / delta) % 6);
            else if (max == g) h = 60 * (((b - r) / delta) + 2);
            else h = 60 * (((r - g) / delta) + 4);
        }

        if (h < 0) h += 360;

        return (h, max <= 0 ? 0 : delta / max, max);
    }

    private static (byte R, byte G, byte B) FromHsv(double h, double s, double v)
    {
        h %= 360;
        if (h < 0) h += 360;

        var c = v * s;
        var x = c * (1 - Math.Abs(h / 60 % 2 - 1));
        var m = v - c;

        var (r, g, b) = h switch
        {
            < 60 => (c, x, 0d),
            < 120 => (x, c, 0d),
            < 180 => (0d, c, x),
            < 240 => (0d, x, c),
            < 300 => (x, 0d, c),
            _ => (c, 0d, x)
        };

        return ((byte)Math.Round((r + m) * 255),
                (byte)Math.Round((g + m) * 255),
                (byte)Math.Round((b + m) * 255));
    }

    /// <summary>Палитра встроенных эффектов: имя → номер режима.</summary>
    public static readonly (string Name, int Mode)[] Effects =
    {
        ("Выключено", 0),
        ("Статичный цвет", 1),
        ("Одна клавиша горит", 2),
        ("Одна клавиша гаснет", 3),
        ("Мерцание", 4),
        ("Падение", 5),
        ("Разноцветный", 6),
        ("Дыхание", 7),
        ("Спектр", 8),
        ("Волна от центра", 9),
        ("Прокрутка", 10),
        ("Перекат", 11),
        ("Вращение", 12),
        ("Взрыв", 13),
        ("Запуск", 14),
        ("Круги по воде", 15),
        ("Течение", 16),
        ("Пульсация", 17),
        ("Наклон", 18),
        ("Шаттл", 19),

        // Свои эффекты: рисуются попиксельно (только по кабелю).
        ("Своя волна →", LightEffects.WaveRight),
        ("Своя волна ←", LightEffects.WaveLeft),
        ("Свой градиент", LightEffects.Gradient),
        ("Своё дыхание", LightEffects.Breathe),
        ("Гифка → справа налево", LightEffects.GifFlow),
        ("Своя раскраска", LightEffects.Paint),
        ("Свой спектр", LightEffects.Spectrum),
        ("Своя волна от центра", LightEffects.CenterWave),
        ("Своё течение", LightEffects.Flow),
        ("Своё вращение", LightEffects.Rotate)
    };

    /// <summary>Клавиша с подсветкой: код прошивки, ряд (0 — верх) и колонка (0 — левый край).</summary>
    public readonly record struct KeyPosition(byte Key, int Row, int Col);

    /// <summary>Раскладка из rgb-keyboard.xml: light_index → row_col.</summary>
    public static readonly KeyPosition[] KeyLayout =
    {
        new(1, 0, 0), new(2, 0, 1), new(3, 0, 2), new(4, 0, 3), new(5, 0, 5),
        new(6, 0, 6), new(7, 0, 7), new(8, 0, 8), new(9, 0, 9), new(10, 0, 10),
        new(11, 0, 11), new(12, 0, 12), new(13, 0, 13),

        new(19, 1, 0), new(20, 1, 1), new(21, 1, 2), new(22, 1, 3), new(23, 1, 4),
        new(24, 1, 5), new(25, 1, 6), new(26, 1, 7), new(27, 1, 8), new(28, 1, 9),
        new(29, 1, 10), new(30, 1, 11), new(31, 1, 12), new(103, 1, 14),
        new(117, 1, 15),

        new(37, 2, 0), new(38, 2, 1), new(39, 2, 2), new(40, 2, 3), new(41, 2, 4),
        new(42, 2, 5), new(43, 2, 6), new(44, 2, 7), new(45, 2, 8), new(46, 2, 9),
        new(47, 2, 10), new(48, 2, 11), new(49, 2, 12), new(67, 2, 14), new(118, 2, 15),

        new(55, 3, 0), new(56, 3, 2), new(57, 3, 3), new(58, 3, 4), new(59, 3, 5),
        new(60, 3, 6), new(61, 3, 7), new(62, 3, 8), new(63, 3, 9), new(64, 3, 10),
        new(65, 3, 11), new(66, 3, 12), new(85, 3, 14), new(121, 3, 15),

        new(73, 4, 0), new(74, 4, 2), new(75, 4, 3), new(76, 4, 4), new(77, 4, 5),
        new(78, 4, 6), new(79, 4, 7), new(80, 4, 8), new(81, 4, 9), new(82, 4, 10),
        new(83, 4, 11), new(84, 4, 14), new(101, 4, 16),

        new(91, 5, 0), new(92, 5, 1), new(93, 5, 3), new(94, 5, 6), new(95, 5, 10),
        new(96, 5, 12), new(98, 5, 14), new(99, 5, 15), new(100, 5, 16),
        new(102, 5, 17), new(119, 0, 14)
    };

    /// <summary>Размер живого кадра (команда 0x20): 128 записей по 4 байта.</summary>
    public const int LiveFrameSize = 512;

    /// <summary>
    /// Таблица цветов (576 байт, клавиша по смещению код×4) → живой кадр для команды 0x20:
    /// записи [код, R, G, B] идут подряд, без пропусков — так их собирает официальная программа
    /// на странице «Музыка». Остаток кадра — нули.
    /// </summary>
    public static byte[] LiveFrame(ReadOnlySpan<byte> bank)
    {
        var live = new byte[LiveFrameSize];
        var n = 0;

        for (var code = 1; code * 4 + 3 < bank.Length - 2 && n < LiveFrameSize / 4; code++)
        {
            var at = code * 4;
            if (bank[at] != code) continue;

            bank.Slice(at, 4).CopyTo(live.AsSpan(n * 4));
            n++;
        }

        return live;
    }

    /// <summary>Цвет «#RRGGBB» → байты; при ошибке — белый.</summary>
    public static (byte R, byte G, byte B) ParseColor(string? hex, (byte, byte, byte) fallback = default)
    {
        if (string.IsNullOrWhiteSpace(hex)) return fallback;

        var text = hex.Trim().TrimStart('#');
        if (text.Length == 3)
            text = string.Concat(text.Select(c => new string(c, 2)));

        if (text.Length == 6
            && byte.TryParse(text[..2], NumberStyles.HexNumber, CultureInfo.InvariantCulture, out var r)
            && byte.TryParse(text[2..4], NumberStyles.HexNumber, CultureInfo.InvariantCulture, out var g)
            && byte.TryParse(text[4..], NumberStyles.HexNumber, CultureInfo.InvariantCulture, out var b))
            return (r, g, b);

        return fallback;
    }
}

/// <summary>Один кадр экрана: RGB565 (128×128, little-endian) и задержка показа.</summary>
public readonly record struct ScreenFrame(byte[] Rgb565, int DelayMs);
