using System;

namespace RigControl;

/// <summary>
/// Свои эффекты подсветки — рисуются попиксельно и уходят живыми кадрами через
/// <see cref="Ak820Device.SendLiveFrame"/>. Работают только по кабелю:
/// по радио доступны лишь встроенные режимы прошивки.
/// </summary>
public static class LightEffects
{
    public const int WaveRight = 100;   // волна от правого края влево
    public const int WaveLeft = 101;    // волна слева направо
    public const int Gradient = 102;    // плавный перелив по клавишам
    public const int Breathe = 103;     // дыхание без погасания
    public const int GifFlow = 104;     // гифка с экрана «продолжается» на клавиши справа налево
    public const int Paint = 105;       // «Своя раскраска»: свой цвет на каждую клавишу (вкладка «Раскраска»)
    public const int Spectrum = 106;    // свой спектр: вся клавиатура плавно меняет цвет (синий → бирюзовый → зелёный…)
    public const int CenterWave = 107;  // своя волна от центра: радужные кольца расходятся от середины
    public const int Flow = 108;        // своё течение: радуга плывёт по клавиатуре (направление — как у эффектов)
    public const int Rotate = 109;      // своё вращение: радуга крутится вокруг центра (у вентиляторов — вокруг своего)

    /// <summary>Радужные свои эффекты: вентиляторы считают их сами в своих точках — идут ровно в такт.</summary>
    public static bool IsRainbowFlow(int mode) => mode is Spectrum or CenterWave or Flow or Rotate;

    /// <summary>Полный круг цветов для волны, течения и вращения в секундах, скорости 0–5.</summary>
    private static readonly double[] FlowPeriod = { 9, 6.5, 5, 3.8, 2.8, 2 };

    /// <summary>Ширина клавиатуры к высоте (17 клавиш на 6 рядов): чтобы круги были кругами, а не овалами.</summary>
    private const double Aspect = 2.8;

    /// <summary>
    /// Цвет радужного эффекта в точке (x, y — 0…1 по ширине и высоте клавиатуры) на момент <paramref name="seconds"/>.
    /// <paramref name="ring"/> — для вентилятора: место светодиода на кольце (0 — верх, по часовой);
    /// у «вращения» вентилятор крутит радугу вокруг своего центра, клавиатура — вокруг своего.
    /// Значения — как для светодиодов клавиатуры (без её яркости).
    /// </summary>
    public static (byte R, byte G, byte B) RainbowAt(int mode, double x, double y, double? ring,
        double seconds, int speed, int direction)
    {
        speed = Math.Clamp(speed, 0, 5);
        if (mode == Spectrum) return FromHsv(240 - seconds / SpectrumPeriod[speed] * 360, 1, 1);

        var phase = seconds / FlowPeriod[speed];
        double lag;

        switch (mode)
        {
            case CenterWave:
            {
                var dx = (x - 0.5) * Aspect;
                var dy = y - 0.5;
                lag = Math.Sqrt(dx * dx + dy * dy) / 1.45;   // от центра до края — почти целая радуга
                break;
            }

            case Rotate:
            {
                var angle = ring ?? (Math.Atan2(y - 0.5, (x - 0.5) * Aspect) / (2 * Math.PI) + 0.25);
                lag = (angle % 1 + 1) % 1;
                if (Math.Clamp(direction, 0, 3) % 2 == 1) lag = 1 - lag;   // в другую сторону
                break;
            }

            default: // течение
                lag = Math.Clamp(direction, 0, 3) switch
                {
                    1 => 1 - x,
                    2 => y,
                    3 => 1 - y,
                    _ => x
                };
                break;
        }

        // Как «Спектр»: каждая точка идёт синий → бирюзовый → зелёный…, дальние — с запаздыванием.
        return FromHsv(240 - (phase - lag) * 360, 1, 1);
    }

    /// <summary>Полный круг «Своего спектра» в секундах для скоростей 0–5.</summary>
    private static readonly double[] SpectrumPeriod = { 24, 16, 11, 8, 5.5, 3.5 };

    public static bool IsCustom(int mode) => mode >= 100;

    /// <summary>Сколько кадров в секунду держим: больше 20 не нужно, USB и так занят.</summary>
    public const int FrameIntervalMs = 50;

    /// <summary>
    /// Кадр своих цветов для момента <paramref name="seconds"/>.
    /// Клавиши никогда не гаснут полностью: нижняя граница яркости ~30 %.
    /// </summary>
    public static byte[] Build(int mode, double seconds, int speed, int direction,
        (byte R, byte G, byte B) color)
    {
        speed = Math.Clamp(speed, 0, 5);
        var rate = 0.12 + 0.10 * speed;      // полных проходов в секунду
        var phase = seconds * rate % 1.0;

        // Цвет из гифки часто бледный: на светодиодах он выглядит белым.
        // Поднимаем насыщенность, чтобы волна читалась цветом, а не «белым».
        color = Saturate(color, 1.35);

        var (baseH, baseS, _) = ToHsv(color);

        var frame = new byte[9 * 64];
        frame[^2] = 0xAA;
        frame[^1] = 0x55;

        foreach (var key in Ak820Protocol.KeyLayout)
        {
            var x = key.Col / 17.0;                 // 0 — левый край, 1 — правый
            var y = key.Row / 5.0;                  // 0 — верх, 1 — низ
            double crest;
            double value;

            switch (mode)
            {
                case Spectrum or CenterWave or Flow or Rotate:
                    // Как «Спектр» прошивки: с синего, в сторону бирюзового и зелёного.
                    Put(frame, key.Key, RainbowAt(mode, x, y, null, seconds, speed, direction));
                    continue;

                case Gradient:
                {
                    var hue = baseH + x * 90 + y * 20 + seconds * 22;
                    var band = 0.5 + 0.5 * Math.Cos(2 * Math.PI * (x * 0.5 - y * 0.25 - seconds * 0.12));
                    value = 0.62 + 0.30 * band;
                    var rgb = FromHsv(hue, Math.Max(0.72, baseS), value);
                    Put(frame, key.Key, rgb);
                    continue;
                }

                case Breathe:
                {
                    var pulseRate = 0.10 + 0.06 * speed;
                    var wave = 0.5 + 0.5 * Math.Sin(2 * Math.PI * (seconds * pulseRate - x * 0.08));
                    value = 0.35 + 0.65 * wave;
                    var rgb = Blend(color, (255, 255, 255), 0.10 * wave);
                    Put(frame, key.Key, Scale(rgb, value));
                    continue;
                }

                default:
                {
                    var d = WaveDistance(mode, direction, x, y);
                    crest = 0.5 + 0.5 * Math.Cos(2 * Math.PI * (d - phase));
                    value = 0.32 + 0.68 * Math.Pow(crest, 1.3);
                    var rgb = Blend(color, (255, 255, 255), 0.08 * Math.Pow(crest, 6));
                    Put(frame, key.Key, Scale(rgb, value));
                    continue;
                }
            }
        }

        return frame;
    }

    /// <summary>
    /// «Продолжение гифки»: экран стоит справа, поэтому цвета правого края кадра уходят на клавиши и
    /// «вытекают» влево. Клавиша на расстоянии d от экрана показывает то, что экран показывал d·travel секунд назад.
    /// По высоте клавиатура делится на те же 6 полос, что и край кадра.
    /// </summary>
    public static byte[] BuildFlow((byte R, byte G, byte B)[][] strips, int[] cumulative, int loopMs,
        double seconds, int speed)
    {
        speed = Math.Clamp(speed, 0, 5);
        var travel = 2.6 - 0.35 * speed;   // за сколько секунд цвет пересекает клавиатуру: 2,6 … 0,85

        var frame = new byte[9 * 64];
        frame[^2] = 0xAA;
        frame[^1] = 0x55;

        var total = Math.Max(1, loopMs);

        foreach (var key in Ak820Protocol.KeyLayout)
        {
            var distance = 1 - key.Col / 17.0;           // 0 — у экрана (справа), 1 — левый край
            var ms = (long)Math.Floor((seconds - distance * travel) * 1000);
            var at = (int)(((ms % total) + total) % total);

            var index = Array.FindIndex(cumulative, v => at < v);
            if (index < 0) index = strips.Length - 1;

            var row = Math.Clamp(key.Row, 0, strips[index].Length - 1);
            Put(frame, key.Key, Vivid(strips[index][row]));
        }

        return frame;
    }

    /// <summary>
    /// Цвет из гифки делаем «светодиодным»: пастель на LED выглядит белой, поэтому поднимаем насыщенность,
    /// а тёмные участки не гасим полностью. Почти белый остаётся белым.
    /// </summary>
    private static (byte R, byte G, byte B) Vivid((byte R, byte G, byte B) c)
    {
        var (h, s, v) = ToHsv(c);

        if (s > 0.04) s = Math.Clamp(s * 2.6 + 0.2, 0, 0.95);
        v = Math.Clamp(v * 1.15, 0.35, 1);

        return FromHsv(h, s, v);
    }

    /// <summary>Расстояние «откуда идёт волна»: 0 — источник, 1 — дальний край.</summary>
    private static double WaveDistance(int mode, int direction, double x, double y)
    {
        // Слева направо — зеркалим ось.
        if (mode == WaveLeft) x = 1 - x;

        return Math.Clamp(direction, 0, 3) switch
        {
            // 1 — от верхнего правого угла по диагонали.
            1 => 0.78 * (1 - x) + 0.22 * y,
            // 2 — от нижнего правого (там, где экран) по диагонали влево.
            2 => 0.78 * (1 - x) + 0.22 * (1 - y),
            // 3 — от середины правого края, волна расходится веером.
            3 => 0.62 * (1 - x) + 0.38 * Math.Abs(y - 0.5) * 2,
            // 0 — строго горизонтально справа налево.
            _ => 1 - x
        };
    }

    /// <summary>Яркость 0…1 поверх готового кадра: живые кадры прошивка не приглушает, яркость накладываем сами.</summary>
    public static byte[] Dim(byte[] frame, double scale)
    {
        if (scale >= 0.999) return frame;
        scale = Math.Clamp(scale, 0, 1);

        for (var key = 1; key * 4 + 3 < frame.Length - 2; key++)
        {
            var at = key * 4;
            if (frame[at] != key) continue;

            frame[at + 1] = (byte)Math.Round(frame[at + 1] * scale);
            frame[at + 2] = (byte)Math.Round(frame[at + 2] * scale);
            frame[at + 3] = (byte)Math.Round(frame[at + 3] * scale);
        }

        return frame;
    }

    private static void Put(byte[] frame, byte key, (byte R, byte G, byte B) color)
    {
        var offset = key * 4;
        frame[offset] = key;
        frame[offset + 1] = color.R;
        frame[offset + 2] = color.G;
        frame[offset + 3] = color.B;
    }

    /// <summary>Поднимает насыщенность бледного цвета, чтобы он не выглядел белым.</summary>
    private static (byte R, byte G, byte B) Saturate((byte R, byte G, byte B) c, double boost)
    {
        var (h, s, v) = ToHsv(c);
        return FromHsv(h, Math.Clamp(s * boost + 0.08, 0, 0.92), Math.Max(v, 0.85));
    }

    private static (byte R, byte G, byte B) Scale((byte R, byte G, byte B) c, double factor) =>        ((byte)Math.Clamp(c.R * factor, 0, 255),
         (byte)Math.Clamp(c.G * factor, 0, 255),
         (byte)Math.Clamp(c.B * factor, 0, 255));

    private static (byte R, byte G, byte B) Blend((byte R, byte G, byte B) a, (byte R, byte G, byte B) b, double k) =>
        ((byte)Math.Clamp(a.R + (b.R - a.R) * k, 0, 255),
         (byte)Math.Clamp(a.G + (b.G - a.G) * k, 0, 255),
         (byte)Math.Clamp(a.B + (b.B - a.B) * k, 0, 255));

    private static (double H, double S, double V) ToHsv((byte R, byte G, byte B) c)
    {
        double r = c.R / 255.0, g = c.G / 255.0, b = c.B / 255.0;
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

        return ((byte)Math.Round(Math.Clamp((r + m) * 255, 0, 255)),
                (byte)Math.Round(Math.Clamp((g + m) * 255, 0, 255)),
                (byte)Math.Round(Math.Clamp((b + m) * 255, 0, 255)));
    }
}
