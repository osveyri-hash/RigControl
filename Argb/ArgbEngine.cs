using System.Windows.Media;

namespace RigControl;

/// <summary>Эффекты ARGB-разъёма. 0 — программа разъём не трогает (как настроил Armoury Crate / BIOS).</summary>
public enum ArgbMode
{
    NotControlled = 0,
    Off = 1,
    Static = 2,
    Breathing = 3,
    Spectrum = 4,
    RainbowWave = 5,
    Comet = 6,
    CpuTemp = 7,
    Keyboard = 8,
    ColorWave = 9
}

/// <param name="Target">0 — все разъёмы сразу (и RGB 12 В платы), 1… — один ARGB-разъём.</param>
/// <param name="BoardEffect">Эффект крутит сам контроллер платы (режим совместимости), а не программа.</param>
public sealed record ArgbConfig(ArgbMode Mode, Color Color, int Brightness, int Speed, int Leds, int Target,
    bool BoardEffect, int Fans = 3, bool Rainbow = true, Color Color2 = default, Color Color3 = default)
{
    public bool UsesColor => Mode is ArgbMode.Static or ArgbMode.Breathing or ArgbMode.Comet
        or ArgbMode.RainbowWave or ArgbMode.ColorWave;
    public bool Animated => Mode is not (ArgbMode.NotControlled or ArgbMode.Off or ArgbMode.Static);
}

/// <summary>
/// Фоновый поток: держит связь с контроллером ASUS Aura и рисует кадры (до 30 в секунду).
/// Сам контроллер анимаций программы не знает — каждый кадр приходит отсюда, поэтому эффекты
/// работают, пока запущена Rig Control.
/// </summary>
public sealed class ArgbEngine : IDisposable
{
    public const int MaxLeds = 120;
    private static readonly int[] FixedEffectChannel = { 0 };

    private readonly AutoResetEvent _wake = new(false);
    private readonly Thread _thread;
    private readonly Func<double> _cpuTemp;
    private volatile ArgbConfig _config;
    private volatile bool _stop;
    private volatile bool _reconnect;
    private volatile byte[]? _frame;

    private AuraUsb? _aura;
    private string? _prepared;   // для каких каналов и в каком способе контроллер уже настроен
    private long _nextFindAt;

    public event Action<string, bool>? StatusChanged;

    /// <summary>Сколько ARGB-разъёмов у найденного контроллера (0 — не найден).</summary>
    public int Headers { get; private set; }

    /// <summary>Последний отправленный кадр (R, G, B на светодиод) — для картинки в окне.</summary>
    public byte[]? Frame => _frame;

    public ArgbEngine(ArgbConfig config, Func<double> cpuTemp)
    {
        _config = config;
        _cpuTemp = cpuTemp;
        _thread = new Thread(Run) { IsBackground = true, Name = "ARGB", Priority = ThreadPriority.AboveNormal };
        _thread.Start();
    }

    public ArgbConfig Config
    {
        get => _config;
        set
        {
            _config = value;
            _wake.Set();
        }
    }

    /// <summary>Искать контроллер заново (кнопка, выход из сна).</summary>
    public void Reconnect()
    {
        _reconnect = true;
        _nextFindAt = 0;
        _wake.Set();
    }

    private void Run()
    {
        var started = Clock.Ms;
        long nextFrame = 0;
        var hiRes = false;
        var rgb = new byte[MaxLeds * 3];
        var output = new byte[MaxLeds * 3];
        var last = new byte[MaxLeds * 3];
        long lastSent = 0;
        var state = new EffectState();
        string? status = null;
        AuraUsb? channelAura = null;
        ArgbConfig? channelConfig = null;
        var headersCache = Array.Empty<int>();
        var effectChannelsCache = Array.Empty<int>();
        var withFixedCache = false;

        void Report(string text, bool ok)
        {
            if (text == status) return;
            status = text;
            StatusChanged?.Invoke(text, ok);
        }

        while (!_stop)
        {
            var config = _config;

            try
            {
                if (_reconnect)
                {
                    _reconnect = false;
                    CloseDevice();
                }

                if (config.Mode == ArgbMode.NotControlled)
                {
                    // Разъём не наш: контроллер не держим, ничего не шлём.
                    CloseDevice();
                    _frame = null;
                    Report("Не управляется — подсветка как в Armoury Crate / BIOS", false);
                    _wake.WaitOne(2000);
                    continue;
                }

                if (_aura is null)
                {
                    var now = Environment.TickCount64;
                    if (now < _nextFindAt)
                    {
                        _wake.WaitOne((int)Math.Min(1000, _nextFindAt - now));
                        continue;
                    }

                    _aura = AuraUsb.Find();
                    if (_aura is null)
                    {
                        Headers = 0;
                        _nextFindAt = Environment.TickCount64 + 5000;
                        Report("Контроллер ASUS Aura не найден", false);
                        continue;
                    }

                    Headers = _aura.ArgbHeaders;
                    _prepared = null;
                    Log($"Найден {_aura.Firmware}, PID {_aura.ProductId:X4}, ARGB-разъёмов {_aura.ArgbHeaders}, " +
                        $"светодиодов платы {_aura.MainboardLeds}, таблица: {Convert.ToHexString(_aura.ConfigTable)}", reset: true);
                }

                var aura = _aura;
                // Каналы не меняются на каждом кадре. Раньше здесь создавались
                // несколько массивов LINQ до 80 раз в секунду, из-за чего ARGB
                // создавал лишний мусор и мог подёргивать интерфейс на слабых ПК.
                if (!ReferenceEquals(channelAura, aura) || !ReferenceEquals(channelConfig, config))
                {
                    withFixedCache = config.Target <= 0;
                    headersCache = withFixedCache
                        ? Enumerable.Range(0, aura.ArgbHeaders).ToArray()
                        : new[] { Math.Clamp(config.Target - 1, 0, aura.ArgbHeaders - 1) };

                    effectChannelsCache = new int[headersCache.Length + (withFixedCache ? 1 : 0)];
                    var offset = 0;
                    if (withFixedCache) effectChannelsCache[offset++] = FixedEffectChannel[0];
                    for (var i = 0; i < headersCache.Length; i++)
                        effectChannelsCache[offset + i] = headersCache[i] + 1;

                    channelAura = aura;
                    channelConfig = config;
                }

                var headers = headersCache;
                var effectChannels = effectChannelsCache;
                var withFixed = withFixedCache;

                var leds = Math.Clamp(config.Leds, 1, MaxLeds);
                var t = (Clock.Ms - started) / 1000.0;
                Render(config, leds, t, rgb, state);
                var span = rgb.AsSpan(0, leds * 3);

                // Светодиоды ARGB светят линейно: без гамма-коррекции розовый выглядит почти белым.
                // Яркость — уже после гаммы, в «линейной» шкале светодиода: иначе на малой яркости
                // слабые каналы округляются в ноль и розовый превращается в красный или мигает.
                var bri = Math.Clamp(config.Brightness, 0, 100) / 100.0;
                for (var i = 0; i < leds * 3; i++) output[i] = Out(rgb[i], bri);
                var send = output.AsSpan(0, leds * 3);

                // Картинка для окна — новый массив только когда кадр поменялся (не 30 раз в секунду впустую).
                byte[] Preview()
                {
                    var seen = Math.Pow(bri, 1 / 2.2);
                    var preview = new byte[leds * 3];
                    for (var i = 0; i < leds * 3; i++) preview[i] = (byte)Math.Round(rgb[i] * seen);
                    return preview;
                }

                if (config.BoardEffect)
                {
                    // Встроенный эффект: шлём, только когда что-то поменялось.
                    var (mode, r, g, b) = BoardEffectFor(config, span);
                    (r, g, b) = (Out(r, bri), Out(g, bri), Out(b, bri));
                    var key = $"fx:{string.Join(",", effectChannels)}:{mode}:{r}:{g}:{b}";
                    if (_prepared != key)
                    {
                        aura.SetEffect(effectChannels, mode, r, g, b);
                        _prepared = key;
                        Log($"Эффект платы {mode} цвет {r},{g},{b} на каналы {string.Join(",", effectChannels)}");
                    }

                    _frame = Preview();
                    Report($"{aura.Firmware} · ARGB-разъёмов: {aura.ArgbHeaders} · эффект платы", true);
                    _wake.WaitOne(config.Mode is ArgbMode.CpuTemp or ArgbMode.Keyboard ? 2000 : 500);
                    continue;
                }

                var directKey = "direct:" + string.Join(",", effectChannels);
                if (_prepared != directKey)
                {
                    aura.EnterDirect(effectChannels);
                    _prepared = directKey;
                    lastSent = 0;
                    Log($"Прямой режим на каналы эффектов {string.Join(",", effectChannels)}, светодиодов {leds}");
                }

                var tick = Environment.TickCount64;

                // Неизменный кадр повторяем раз в секунду: если плату перехватили (сон, Armoury Crate) — вернётся.
                var changed = !send.SequenceEqual(last.AsSpan(0, leds * 3));
                if (changed || tick - lastSent > 1000)
                {
                    foreach (var h in headers) aura.SendDirect(h, send, leds);
                    if (withFixed && aura.MainboardLeds > 0)
                        aura.SendDirect(AuraUsb.FixedDirectChannel, send, Math.Min(leds, aura.MainboardLeds));

                    send.CopyTo(last);
                    lastSent = tick;
                    if (changed || _frame is null || _frame.Length != leds * 3) _frame = Preview();
                }

                Report($"{aura.Firmware} · ARGB-разъёмов: {aura.ArgbHeaders}", true);
                if (config.Animated)
                {
                    // Ровно 60 кадров в секунду: ждём до следующего срока, а не «33 мс после работы»
                    // (с обычным таймером Windows это было 31–47 мс — неровный шаг и видимые рывки).
                    if (!hiRes) { TimerResolution.Begin(); hiRes = true; }
                    var nowMs = Clock.Ms;
                    nextFrame = nextFrame == 0 || nowMs - nextFrame > 50 ? nowMs + FrameMs : nextFrame + FrameMs;
                    var wait = (int)Math.Max(1, nextFrame - nowMs);
                    _wake.WaitOne(wait);
                }
                else
                {
                    if (hiRes) { TimerResolution.End(); hiRes = false; }
                    nextFrame = 0;
                    _wake.WaitOne(250);
                }
            }
            catch (Exception ex)
            {
                Log("Ошибка: " + ex.Message);
                CloseDevice();
                _nextFindAt = Environment.TickCount64 + 3000;
                Report("Связь с контроллером потеряна — ищу снова…", false);
                _wake.WaitOne(1000);
            }
        }

        if (hiRes) TimerResolution.End();
        CloseDevice();
    }

    /// <summary>Шаг кадров прямой подсветки (≈80 кадров/с): движение заметно плавнее, но USB не забивается.</summary>
    private const int FrameMs = 12;

    private void CloseDevice()
    {
        _aura?.Dispose();
        _aura = null;
        _prepared = null;
    }

    /// <summary>
    /// Что включить встроенным эффектом платы. Свои эффекты (волна, огонёк, температура, клавиатура)
    /// контроллер не умеет — для них берётся ближайший: радуга, цвет или текущий цвет одним тоном.
    /// </summary>
    private static (byte Mode, byte R, byte G, byte B) BoardEffectFor(ArgbConfig config, ReadOnlySpan<byte> frame)
    {
        static byte Scale(byte v) => v;   // яркость добавляется потом, вместе с гаммой
        var c = config.Color;

        return config.Mode switch
        {
            ArgbMode.Off => (0, 0, 0, 0),
            ArgbMode.Static or ArgbMode.Comet => (1, Scale(c.R), Scale(c.G), Scale(c.B)),
            ArgbMode.Breathing => (2, Scale(c.R), Scale(c.G), Scale(c.B)),
            ArgbMode.Spectrum => (4, 0, 0, 0),
            ArgbMode.RainbowWave => (5, 0, 0, 0),
            ArgbMode.ColorWave when config.Rainbow => (5, 0, 0, 0),
            ArgbMode.ColorWave => (1, c.R, c.G, c.B),
            // Температура и клавиатура: ровный текущий цвет, огрублённый, чтобы не слать на плату каждую мелочь.
            _ => (1, (byte)(frame[0] & 0xF0), (byte)(frame[1] & 0xF0), (byte)(frame[2] & 0xF0))
        };
    }

    private static void Log(string text, bool reset = false)
    {
        var line = $"{DateTime.Now:HH:mm:ss} {text}{Environment.NewLine}";
        LogFile.Append("argb.log", line, maxBytes: 200_000, reset: reset);
    }

    // ── Эффекты ──────────────────────────────────────────────────────────────

    private sealed class EffectState
    {
        public double Temp = double.NaN;
        public double[] Smooth = new double[MaxLeds * 3];
        public double LastT = double.NaN;
        public KeyboardSampler? Sampler;
    }

    /// <summary>Гамма 2,2: цвет на вентиляторах — как на экране и на клавиатуре (у неё своя коррекция в прошивке).</summary>
    private static readonly double[] Linear = Enumerable.Range(0, 256)
        .Select(v => Math.Pow(v / 255.0, 2.2))
        .ToArray();

    private static byte Out(byte value, double brightness) =>
        (byte)Math.Clamp(Math.Round(255 * Linear[value] * brightness), 0, 255);

    private static readonly double[] SpeedFactor = { 0.35, 0.6, 1.0, 1.6, 2.5 };

    private static (byte R, byte G, byte B) PaletteColor(ArgbConfig config, double position)
    {
        var second = config.Color2.A == 0 ? config.Color : config.Color2;
        var third = config.Color3.A == 0 ? second : config.Color3;
        position = (position % 1 + 1) % 1;

        // Три участка замыкаются в круг: третий цвет плавно возвращается в первый,
        // поэтому на стыке цикла нет скачка и эффект выглядит как обычная радуга.
        if (position < 1.0 / 3)
            return Blend(config.Color, second, position * 3);

        if (position < 2.0 / 3)
            return Blend(second, third, (position - 1.0 / 3) * 3);

        return Blend(third, config.Color, (position - 2.0 / 3) * 3);
    }

    private static (byte R, byte G, byte B) Blend(Color a, Color b, double amount)
    {
        amount = Math.Clamp(amount, 0, 1);
        return (
            (byte)Math.Round(a.R + (b.R - a.R) * amount),
            (byte)Math.Round(a.G + (b.G - a.G) * amount),
            (byte)Math.Round(a.B + (b.B - a.B) * amount));
    }

    private void Render(ArgbConfig config, int leds, double t, byte[] rgb, EffectState state)
    {
        const double bri = 1;   // кадр — в полную силу, яркость применяется при отправке
        var speed = SpeedFactor[Math.Clamp(config.Speed, 1, 5) - 1];
        var c = config.Color;

        void Put(int i, double r, double g, double b)
        {
            rgb[i * 3] = (byte)Math.Clamp(Math.Round(r * bri), 0, 255);
            rgb[i * 3 + 1] = (byte)Math.Clamp(Math.Round(g * bri), 0, 255);
            rgb[i * 3 + 2] = (byte)Math.Clamp(Math.Round(b * bri), 0, 255);
        }

        void Fill(double r, double g, double b)
        {
            for (var i = 0; i < leds; i++) Put(i, r, g, b);
        }

        switch (config.Mode)
        {
            case ArgbMode.Off:
                Array.Clear(rgb);
                break;

            case ArgbMode.Static:
                Fill(c.R, c.G, c.B);
                break;

            case ArgbMode.Breathing:
            {
                var phase = t * speed / 4.0 * 2 * Math.PI;
                var k = (1 - Math.Cos(phase)) / 2;
                k = 0.03 + 0.97 * k * k;
                Fill(c.R * k, c.G * k, c.B * k);
                break;
            }

            case ArgbMode.Spectrum:
            {
                var hue = (t * 36 * speed) % 360;
                var color = ColorWheel.FromHsv(hue, 1, 1);
                Fill(color.R, color.G, color.B);
                break;
            }

            case ArgbMode.RainbowWave:
            case ArgbMode.ColorWave:
                if (!config.Rainbow)
                {
                    for (var i = 0; i < leds; i++)
                    {
                        var position = (double)i / Math.Max(1, leds - 1);
                        // Та же скорость, что у радуги: при скорости 3 палитра делает
                        // полный круг примерно за секунду, а не меняется по одному LED.
                        var color = PaletteColor(config, position - t * speed / 3.0);
                        // Все цвета остаются полной яркости: волна движется цветом, а не тёмными провалами.
                        Put(i, color.R, color.G, color.B);
                    }
                    break;
                }

                for (var i = 0; i < leds; i++)
                {
                    var hue = (i * 360.0 / Math.Min(leds, 24) - t * 120 * speed) % 360;
                    if (hue < 0) hue += 360;
                    var color = ColorWheel.FromHsv(hue, 1, 1);
                    Put(i, color.R, color.G, color.B);
                }
                break;

            case ArgbMode.Comet:
            {
                var tail = Math.Max(3.0, leds / 4.0);
                var head = (t * speed * 18) % leds;
                for (var i = 0; i < leds; i++)
                {
                    var d = head - i;
                    if (d < 0) d += leds;
                    var k = d < tail ? Math.Pow(1 - d / tail, 2) : 0;
                    Put(i, c.R * k, c.G * k, c.B * k);
                }
                break;
            }

            case ArgbMode.CpuTemp:
            {
                var temp = _cpuTemp();
                if (double.IsNaN(temp))
                {
                    // Датчика нет — тихое белое дыхание, чтобы было видно, что программа работает.
                    var k = 0.15 + 0.1 * Math.Sin(t * 2);
                    Fill(255 * k, 255 * k, 255 * k);
                    break;
                }

                // Плавно догоняем показание, без скачков раз в секунду.
                state.Temp = double.IsNaN(state.Temp) ? temp : state.Temp + (temp - state.Temp) * 0.026;   // ≈ как 0,05 при 30 кадрах/с
                var color = ColorWheel.FromHsv(TempHue(state.Temp), 1, 1);
                Fill(color.R, color.G, color.B);
                break;
            }

            case ArgbMode.Keyboard:
            {
                // Каждый вентилятор — свой кусок клавиатуры (слева направо), светодиоды по кругу:
                // волна от центра, переливы и раскраска переходят с клавиатуры на вентиляторы.
                var fans = Math.Clamp(config.Fans, 1, 6);
                if (state.Sampler is not { } sampler || sampler.Leds != leds || sampler.Fans != fans)
                    state.Sampler = sampler = new KeyboardSampler(leds, fans);

                // Эффект прошивки считаем прямо в точках вентиляторов. У «волны от центра» точки сдвинуты
                // ближе к центру: иначе крайние вентиляторы отстают ровно на целую волну и кажется,
                // что она идёт с краёв.
                // Сглаживание по времени, а не по кадрам: при 60 кадрах/с — та же мягкость, что была при 30.
                var dt = double.IsNaN(state.LastT) ? 0.033 : Math.Clamp(t - state.LastT, 0, 0.25);
                state.LastT = t;
                var follow = 1 - Math.Pow(0.65, dt / 0.033);
                var effect = LightMirror.ActiveEffectMode;
                var squeeze = effect == 9 ? 0.25 : 1.0;
                var table = effect is null ? LightMirror.Display(out _) : null;
                for (var i = 0; i < leds; i++)
                {
                    // Свой радужный эффект — считаем его прямо в точке светодиода, без сглаживания: ровно в такт.
                    {
                        var (cx, cy) = sampler.Position(i);
                        if (LightMirror.CustomSample(cx, cy, sampler.Ring(i), out var cr, out var cg, out var cb))
                        {
                            state.Smooth[i * 3] = cr;
                            state.Smooth[i * 3 + 1] = cg;
                            state.Smooth[i * 3 + 2] = cb;
                            Put(i, cr, cg, cb);
                            continue;
                        }
                    }

                    (double, double, double) Fx()
                    {
                        var (x, y) = sampler.Position(i);
                        LightMirror.EffectSample(0.5 + (x - 0.5) * squeeze, 0.5 + (y - 0.5) * squeeze, i * 0.618 % 1,
                            out var er, out var eg, out var eb);
                        return (er, eg, eb);
                    }

                    var (r, g, b) = effect is not null ? Fx()
                        : table is null ? (0.0, 0.0, 0.0)
                        : sampler.Sample(table, i);
                    var sm = state.Smooth;
                    sm[i * 3] += (r - sm[i * 3]) * follow;
                    sm[i * 3 + 1] += (g - sm[i * 3 + 1]) * follow;
                    sm[i * 3 + 2] += (b - sm[i * 3 + 2]) * follow;
                    Put(i, sm[i * 3], sm[i * 3 + 1], sm[i * 3 + 2]);
                }
                break;
            }
        }
    }

    /// <summary>≤ 40 °C — голубой, 60 — зелёный, 75 — жёлтый, ≥ 90 — красный.</summary>
    private static double TempHue(double temp)
    {
        (double T, double H)[] points = { (40, 195), (60, 120), (75, 55), (90, 0) };

        if (temp <= points[0].T) return points[0].H;
        for (var i = 1; i < points.Length; i++)
        {
            if (temp <= points[i].T)
            {
                var k = (temp - points[i - 1].T) / (points[i].T - points[i - 1].T);
                return points[i - 1].H + (points[i].H - points[i - 1].H) * k;
            }
        }

        return 0;
    }

    public void Dispose()
    {
        _stop = true;
        _wake.Set();
        _thread.Join(1500);
    }
}

/// <summary>
/// Какие клавиши «видит» каждый светодиод вентиляторов в режиме «как клавиатура»:
/// вентиляторы делят клавиатуру на части слева направо, светодиоды вентилятора — кольцо внутри своей части.
/// </summary>
public sealed class KeyboardSampler
{
    public int Leds { get; }
    public int Fans { get; }

    private readonly (byte Code, double W)[][] _taps;
    private readonly (double X, double Y)[] _points;
    private readonly double[] _ring;

    /// <summary>Место светодиода на кольце своего вентилятора: 0 — верх, дальше по часовой (0…1).</summary>
    public double Ring(int led) => _ring[led];

    /// <summary>Где на клавиатуре «стоит» светодиод: x, y — 0…1.</summary>
    public (double X, double Y) Position(int led) => _points[led];

    public KeyboardSampler(int leds, int fans)
    {
        Leds = leds;
        Fans = fans;
        _taps = new (byte, double)[leds][];
        _points = new (double, double)[leds];
        _ring = new double[leds];

        var perFan = Math.Max(1, leds / fans);
        var keys = KeyPaint.Keys
            .Select(k => (k.Code, X: k.X + k.W / 2 - KeyPaint.OriginX, Y: k.Y + k.H / 2 - KeyPaint.OriginY))
            .ToArray();

        for (var i = 0; i < leds; i++)
        {
            var fan = Math.Min(fans - 1, i / perFan);
            var j = i - fan * perFan;
            var n = fan == fans - 1 ? leds - fan * perFan : perFan;

            var angle = 2 * Math.PI * j / Math.Max(1, n) - Math.PI / 2;
            var cellW = KeyPaint.Width / fans;
            var radius = Math.Min(cellW, KeyPaint.Height) * 0.38;
            var px = cellW * (fan + 0.5) + radius * Math.Cos(angle);
            var py = KeyPaint.Height / 2 + radius * Math.Sin(angle);
            _points[i] = (px / KeyPaint.Width, py / KeyPaint.Height);
            _ring[i] = (double)j / Math.Max(1, n);

            _taps[i] = keys
                .Select(k => (k.Code, D: Math.Sqrt((k.X - px) * (k.X - px) + (k.Y - py) * (k.Y - py))))
                .OrderBy(k => k.D)
                .Take(3)
                .Select(k => (k.Code, 1 / (k.D + 12)))
                .ToArray();
        }
    }

    /// <summary>Цвет светодиода по таблице LightMirror.Display ([код, R, G, B] по смещению код×4).</summary>
    public (double R, double G, double B) Sample(byte[] table, int led)
    {
        double r = 0, g = 0, b = 0, sum = 0;
        foreach (var (code, w) in _taps[led])
        {
            var at = code * 4;
            if (at + 3 >= table.Length) continue;
            r += table[at + 1] * w;
            g += table[at + 2] * w;
            b += table[at + 3] * w;
            sum += w;
        }

        return sum > 0 ? (r / sum, g / sum, b / sum) : (0, 0, 0);
    }
}
