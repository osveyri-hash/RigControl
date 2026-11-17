namespace RigControl;

/// <summary>
/// Подсветка по гифке с экрана клавиатуры — живыми кадрами по клавишам (только по кабелю).
/// <list type="bullet">
/// <item>«Картинка» — кадр растянут на всю клавиатуру: каждая клавиша светит средним цветом своего участка.</item>
/// <item>«Волна от экрана» — главный цвет кадра выходит из экранчика (справа, над «→») и расходится кругами:
/// дальние клавиши показывают то, что экран показывал чуть раньше.</item>
/// </list>
/// Всё тяжёлое считается один раз при запуске, на каждый кадр подсветки остаётся смешать два кадра гифки.
/// </summary>
public sealed class GifLight
{
    public const int Picture = 0;
    public const int Ripple = 1;
    public const int Single = 2;

    private static readonly KeyPaint.KeyCap[] Keys = KeyPaint.Keys;

    // Каналы RGB565 → 0…1: как на экране (с гаммой) и в линейной яркости (для честного усреднения).
    private static readonly double[] Gamma5 = Table(31, linear: false), Gamma6 = Table(63, linear: false);
    private static readonly double[] Linear5 = Table(31, linear: true), Linear6 = Table(63, linear: true);

    private readonly int _style;
    private readonly byte[] _keyLed;       // [кадр][клавиша][R,G,B] — уже яркость светодиода (линейная)
    private readonly byte[] _dominantLed;  // [кадр][R,G,B]
    private readonly int[] _start;         // начало кадра в цикле гифки, мс
    private readonly int[] _length;        // сколько экран показывает кадр, мс
    private readonly double[] _distance;   // 0 — у экранчика, 1 — самая дальняя клавиша

    public int FrameCount => _start.Length;
    public int LoopMs { get; }

    /// <summary>Общий цвет гифки (#RRGGBB) — им клавиатура светит сама, когда программа не шлёт кадры.</summary>
    public string FallbackHex { get; }

    public GifLight(IReadOnlyList<ScreenFrame> frames, int style)
    {
        if (frames.Count == 0) throw new ArgumentException("Нет кадров", nameof(frames));

        _style = style == Picture ? Picture : Ripple;
        var count = Math.Min(frames.Count, Ak820Protocol.MaxFrames);

        _start = new int[count];
        _length = new int[count];
        _dominantLed = new byte[count * 3];
        _keyLed = _style == Picture ? new byte[count * Keys.Length * 3] : Array.Empty<byte>();

        var rects = _style == Picture ? SampleRects() : null;
        double sumR = 0, sumG = 0, sumB = 0;
        var acc = 0;

        for (var f = 0; f < count; f++)
        {
            // Экран держит кадр столько, сколько записано в заголовке: единицы по 2 мс, 1…255.
            var delay = Math.Clamp(Math.Max(1, frames[f].DelayMs / 2), 1, 255) * 2;
            _start[f] = acc;
            _length[f] = Math.Max(20, delay);
            acc += _length[f];

            var pixels = frames[f].Rgb565;
            var dominant = Dominant(pixels);
            sumR += dominant.R;
            sumG += dominant.G;
            sumB += dominant.B;

            Store(_dominantLed, f * 3, ToLed(Vivid(dominant, 0.5, 0.72)));

            if (rects is null) continue;

            // Тёмные участки картинки не гасим совсем: там тускло светится главный цвет кадра.
            var glow = ToLed(WithValue(Vivid(dominant, 0.5, 0), 0.30));

            for (var k = 0; k < Keys.Length; k++)
            {
                var color = Vivid(AverageColor(pixels, rects[k]), 0.35, 0);
                var led = ToLed(color);
                led = (Math.Max(led.R, glow.R), Math.Max(led.G, glow.G), Math.Max(led.B, glow.B));
                Store(_keyLed, (f * Keys.Length + k) * 3, led);
            }
        }

        LoopMs = Math.Max(1, acc);

        var average = Vivid(((byte)(sumR / count), (byte)(sumG / count), (byte)(sumB / count)), 0.5, 0.72);
        FallbackHex = $"#{average.R:X2}{average.G:X2}{average.B:X2}";

        _distance = Distances();
    }

    /// <summary>Таблица цветов (576 байт, клавиша по смещению код×4) на момент <paramref name="seconds"/>.</summary>
    public byte[] Build(double seconds, int brightness, int speed)
    {
        var scale = Math.Clamp(brightness, 0, 5) / 5.0;
        var frame = new byte[9 * 64];
        frame[^2] = 0xAA;
        frame[^1] = 0x55;

        var ms = seconds * 1000;

        if (_style == Picture)
        {
            var (a, b, k, _, _) = FrameAt(ms);
            for (var key = 0; key < Keys.Length; key++)
            {
                Put(frame, Keys[key].Code, Mix(_keyLed, (a * Keys.Length + key) * 3, (b * Keys.Length + key) * 3, k, scale));
            }

            return frame;
        }

        // Волна: клавиша на расстоянии d от экрана показывает то, что экран показывал d·travel назад.
        var travel = (1.6 - 0.2 * Math.Clamp(speed, 0, 5)) * 1000;
        for (var key = 0; key < Keys.Length; key++)
        {
            var d = _distance[key];
            var (a, b, k, since, length) = FrameAt(ms - d * travel);

            // У медленных гифок на каждой смене кадра от экрана расходится светлое кольцо.
            var flash = Math.Clamp((length - 120) / 300.0, 0, 1) * Math.Exp(-since / 200.0);
            var level = scale * (1 - 0.3 * d) * (0.82 + 0.18 * flash);

            Put(frame, Keys[key].Code, Mix(_dominantLed, a * 3, b * 3, k, level));
        }

        return frame;
    }

    /// <summary>
    /// Главный цвет кадра RGB565: оттенок, который занимает больше всего места с учётом насыщенности и яркости
    /// (а не средний цвет — тот у пёстрых гифок выходит серо-бурым). Почти бесцветная картинка — её средний цвет.
    /// </summary>
    public static (byte R, byte G, byte B) Dominant(ReadOnlySpan<byte> rgb565)
    {
        const int bins = 24;
        Span<double> weight = stackalloc double[bins];
        Span<double> sumR = stackalloc double[bins];
        Span<double> sumG = stackalloc double[bins];
        Span<double> sumB = stackalloc double[bins];

        double plainR = 0, plainG = 0, plainB = 0;
        var plain = 0;
        var samples = 0;
        var size = Ak820Protocol.ScreenSize;

        // Каждый второй пиксель по обеим осям: 4096 точек на кадр — с запасом.
        for (var y = 0; y < size; y += 2)
        {
            for (var x = 0; x < size; x += 2)
            {
                var i = (y * size + x) * 2;
                if (i + 1 >= rgb565.Length) continue;

                var value = rgb565[i] | (rgb565[i + 1] << 8);
                var r = Gamma5[(value >> 11) & 0x1F];
                var g = Gamma6[(value >> 5) & 0x3F];
                var b = Gamma5[value & 0x1F];
                samples++;

                var max = Math.Max(r, Math.Max(g, b));
                if (max < 0.08) continue; // фон

                plainR += r;
                plainG += g;
                plainB += b;
                plain++;

                var delta = max - Math.Min(r, Math.Min(g, b));
                var s = delta / max;
                if (s < 0.12) continue; // почти серый — в оттенки не идёт

                var bin = (int)(Hue(r, g, b, max, delta) / (360.0 / bins)) % bins;
                var w = s * s * max;
                weight[bin] += w;
                sumR[bin] += r * w;
                sumG[bin] += g * w;
                sumB[bin] += b * w;
            }
        }

        var best = -1;
        double bestScore = 0, total = 0;
        for (var i = 0; i < bins; i++)
        {
            total += weight[i];
            var score = weight[i] + 0.5 * (weight[(i + bins - 1) % bins] + weight[(i + 1) % bins]);
            if (score > bestScore)
            {
                bestScore = score;
                best = i;
            }
        }

        if (best < 0 || total < 0.015 * Math.Max(1, samples))
        {
            return plain == 0
                ? ((byte)0, (byte)0, (byte)0)
                : (ToByte(plainR / plain), ToByte(plainG / plain), ToByte(plainB / plain));
        }

        double wr = 0, wg = 0, wb = 0, ww = 0;
        for (var d = -1; d <= 1; d++)
        {
            var i = (best + d + bins) % bins;
            wr += sumR[i];
            wg += sumG[i];
            wb += sumB[i];
            ww += weight[i];
        }

        return (ToByte(wr / ww), ToByte(wg / ww), ToByte(wb / ww));
    }

    // ── Время ────────────────────────────────────────────────────────────────

    /// <summary>Кадр гифки в момент <paramref name="ms"/>: текущий, следующий и доля перехода (вторая половина кадра, ≤ 400 мс).</summary>
    private (int A, int B, double K, double Since, int Length) FrameAt(double ms)
    {
        var at = ms % LoopMs;
        if (at < 0) at += LoopMs;

        var i = Array.BinarySearch(_start, (int)at);
        if (i < 0) i = ~i - 1;
        i = Math.Clamp(i, 0, _start.Length - 1);

        var since = at - _start[i];
        var length = _length[i];
        // Переход — вторая половина кадра (до 400 мс): цвета перетекают, а не перескакивают.
        var fade = Math.Min(length * 0.5, 400);
        var k = since <= length - fade ? 0 : Smooth((since - (length - fade)) / fade);

        return (i, (i + 1) % _start.Length, k, since, length);
    }

    private static double Smooth(double t)
    {
        t = Math.Clamp(t, 0, 1);
        return t * t * (3 - 2 * t);
    }

    // ── Геометрия ────────────────────────────────────────────────────────────

    /// <summary>Участок кадра 128×128 под каждой клавишей: схема клавиатуры растянута на весь кадр,
    /// у клавиши берём и половину промежутков вокруг неё, чтобы в подсветку попала вся картинка.</summary>
    private static (int X0, int Y0, int X1, int Y1)[] SampleRects()
    {
        var minX = Keys.Min(k => k.X);
        var maxX = Keys.Max(k => k.X + k.W);
        var minY = Keys.Min(k => k.Y);
        var maxY = Keys.Max(k => k.Y + k.H);
        var size = Ak820Protocol.ScreenSize;
        const double pad = 5;

        int Map(double v, double min, double max) => (int)Math.Round(Math.Clamp((v - min) / (max - min), 0, 1) * size);

        var rects = new (int, int, int, int)[Keys.Length];
        for (var i = 0; i < Keys.Length; i++)
        {
            var k = Keys[i];
            var x0 = Map(k.X - pad, minX, maxX);
            var x1 = Math.Max(x0 + 1, Map(k.X + k.W + pad, minX, maxX));
            var y0 = Map(k.Y - pad, minY, maxY);
            var y1 = Math.Max(y0 + 1, Map(k.Y + k.H + pad, minY, maxY));
            rects[i] = (Math.Min(x0, size - 1), Math.Min(y0, size - 1), Math.Min(x1, size), Math.Min(y1, size));
        }

        return rects;
    }

    /// <summary>Расстояние каждой клавиши от экранчика, 0…1.</summary>
    private static double[] Distances()
    {
        var cx = KeyPaint.ScreenX + KeyPaint.ScreenSize / 2;
        var cy = KeyPaint.ScreenY + KeyPaint.ScreenSize / 2;

        var result = new double[Keys.Length];
        for (var i = 0; i < Keys.Length; i++)
        {
            var dx = Keys[i].X + Keys[i].W / 2 - cx;
            var dy = Keys[i].Y + Keys[i].H / 2 - cy;
            result[i] = Math.Sqrt(dx * dx + dy * dy);
        }

        var max = Math.Max(1, result.Max());
        for (var i = 0; i < result.Length; i++) result[i] /= max;
        return result;
    }

    /// <summary>Средний цвет участка: складываем в линейной яркости, как смешивает свет глаз.</summary>
    private static (byte R, byte G, byte B) AverageColor(ReadOnlySpan<byte> rgb565, (int X0, int Y0, int X1, int Y1) rect)
    {
        var size = Ak820Protocol.ScreenSize;
        double r = 0, g = 0, b = 0;
        var n = 0;

        for (var y = rect.Y0; y < rect.Y1; y++)
        {
            for (var x = rect.X0; x < rect.X1; x++)
            {
                var i = (y * size + x) * 2;
                if (i + 1 >= rgb565.Length) continue;

                var value = rgb565[i] | (rgb565[i + 1] << 8);
                r += Linear5[(value >> 11) & 0x1F];
                g += Linear6[(value >> 5) & 0x3F];
                b += Linear5[value & 0x1F];
                n++;
            }
        }

        if (n == 0) return (0, 0, 0);
        return (ToByte(Math.Pow(r / n, 1 / 2.2)), ToByte(Math.Pow(g / n, 1 / 2.2)), ToByte(Math.Pow(b / n, 1 / 2.2)));
    }

    // ── Цвет ─────────────────────────────────────────────────────────────────

    private static double[] Table(int max, bool linear)
    {
        var table = new double[max + 1];
        for (var v = 0; v <= max; v++) table[v] = linear ? Math.Pow(v / (double)max, 2.2) : v / (double)max;
        return table;
    }

    private static byte ToByte(double v) => (byte)Math.Clamp(Math.Round(v * 255), 0, 255);

    /// <summary>Цвет экрана → яркость светодиода: экран светит с гаммой, светодиод — линейно (как в раскраске).</summary>
    private static (byte R, byte G, byte B) ToLed((byte R, byte G, byte B) c) =>
        (ToByte(Math.Pow(c.R / 255.0, 2.2)), ToByte(Math.Pow(c.G / 255.0, 2.2)), ToByte(Math.Pow(c.B / 255.0, 2.2)));

    /// <summary>
    /// Пастель на светодиодах выглядит белой — поднимаем насыщенность: бледные цвета сильнее, сочные почти не трогаем
    /// (гамма светодиода и так их усиливает). Серое остаётся серым.
    /// </summary>
    private static (byte R, byte G, byte B) Vivid((byte R, byte G, byte B) c, double amount, double minV)
    {
        var (h, s, v) = ToHsv(c);
        if (s > 0.06) s = Math.Min(0.95, s + (1 - s) * (1 - s) * amount);
        return FromHsv(h, s, Math.Max(v, minV));
    }

    private static (byte R, byte G, byte B) WithValue((byte R, byte G, byte B) c, double v)
    {
        var (h, s, _) = ToHsv(c);
        return FromHsv(h, s, v);
    }

    private static double Hue(double r, double g, double b, double max, double delta)
    {
        if (delta <= 0) return 0;

        double h;
        if (max == r) h = 60 * ((g - b) / delta % 6);
        else if (max == g) h = 60 * ((b - r) / delta + 2);
        else h = 60 * ((r - g) / delta + 4);

        return h < 0 ? h + 360 : h;
    }

    private static (double H, double S, double V) ToHsv((byte R, byte G, byte B) c)
    {
        double r = c.R / 255.0, g = c.G / 255.0, b = c.B / 255.0;
        var max = Math.Max(r, Math.Max(g, b));
        var delta = max - Math.Min(r, Math.Min(g, b));
        return (Hue(r, g, b, max, delta), max <= 0 ? 0 : delta / max, max);
    }

    private static (byte R, byte G, byte B) FromHsv(double h, double s, double v)
    {
        h = (h % 360 + 360) % 360;
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

        return (ToByte(r + m), ToByte(g + m), ToByte(b + m));
    }

    private static void Store(byte[] target, int at, (byte R, byte G, byte B) c)
    {
        target[at] = c.R;
        target[at + 1] = c.G;
        target[at + 2] = c.B;
    }

    private static (byte R, byte G, byte B) Mix(byte[] source, int a, int b, double k, double scale) =>
        ((byte)Math.Clamp(Math.Round((source[a] + (source[b] - source[a]) * k) * scale), 0, 255),
         (byte)Math.Clamp(Math.Round((source[a + 1] + (source[b + 1] - source[a + 1]) * k) * scale), 0, 255),
         (byte)Math.Clamp(Math.Round((source[a + 2] + (source[b + 2] - source[a + 2]) * k) * scale), 0, 255));

    private static void Put(byte[] frame, byte key, (byte R, byte G, byte B) color)
    {
        var offset = key * 4;
        frame[offset] = key;
        frame[offset + 1] = color.R;
        frame[offset + 2] = color.G;
        frame[offset + 3] = color.B;
    }
}
