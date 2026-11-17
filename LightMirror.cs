namespace RigControl;

/// <summary>
/// Что сейчас горит на клавиатуре — для схемы на вкладке «Раскраска».
/// Устройства записывают сюда всё, что отправили (эффект прошивки, свою раскраску, живые кадры),
/// а схема по таймеру спрашивает цвета клавиш. Пишут фоновые потоки, читает окно: состояние —
/// неизменяемые объекты, которые подменяются целиком.
/// </summary>
public static class LightMirror
{
    private sealed record EffectState(int Mode, byte R, byte G, byte B, bool Rainbow,
        int Brightness, int Speed, int Direction, long Since);

    private static volatile EffectState? _effect;

    /// <summary>Идёт свой радужный эффект (живые кадры): что и с какого момента рисуется.</summary>
    private sealed record CustomState(int Mode, int Speed, int Direction, double Scale, long Origin);

    private static volatile CustomState? _custom;
    private static volatile byte[]? _user;   // таблица своей раскраски (576 байт, значения светодиодов)
    private static volatile byte[]? _live;   // последний живой кадр (512 байт)
    private static long _liveAt;

    /// <summary>Живые кадры перестали приходить — клавиатура сама вернулась к сохранённому свету.</summary>
    private const int LiveTimeoutMs = 1000;

    /// <summary>Светодиод светит линейно, экран — с гаммой: обратный пересчёт, чтобы на схеме было как вживую.</summary>
    private static readonly byte[] FromLed = BuildFromLed();

    /// <summary>Отправлен встроенный эффект прошивки (или быстрая смена его цвета).</summary>
    /// <param name="restart">Клавиатура точно начала анимацию заново (полный пакет / радио) — часы схемы тоже с нуля.</param>
    public static void Effect(int mode, byte r, byte g, byte b, bool rainbow, int brightness, int speed, int direction,
        bool restart = false)
    {
        var old = _effect;
        speed = Math.Clamp(speed, 0, 5);
        direction = Math.Clamp(direction, 0, 3);

        // Тот же эффект с другим цветом (синхронизация с гифкой) — анимация продолжается, а не начинается заново.
        var since = !restart && old is not null && old.Mode == mode && old.Speed == speed && old.Direction == direction
            ? old.Since
            : Clock.Ms;

        _effect = new EffectState(mode, r, g, b, rainbow, Math.Clamp(brightness, 0, 5), speed, direction, since);
        _user = null;
        _live = null;
        _custom = null;
    }

    /// <summary>Сохранена своя раскраска (таблица [код, R, G, B] по смещению код×4).</summary>
    public static void UserLight(byte[] bank)
    {
        _user = (byte[])bank.Clone();
        _live = null;
        _custom = null;
    }

    /// <summary>Ушёл живой кадр: записи [код, R, G, B] подряд.</summary>
    public static void Live(byte[] live)
    {
        _live = (byte[])live.Clone();
        Interlocked.Exchange(ref _liveAt, Clock.Ms);
    }

    /// <summary>
    /// Кадр своего радужного эффекта на момент <paramref name="seconds"/> от его начала:
    /// вентиляторы посчитают тот же эффект в своих точках. mode = 0 — живые кадры не из таких эффектов.
    /// </summary>
    public static void Custom(int mode, int speed, int direction, double scale, double seconds)
    {
        if (!LightEffects.IsRainbowFlow(mode))
        {
            _custom = null;
            return;
        }

        var origin = Clock.Ms - (long)Math.Round(seconds * 1000);
        var old = _custom;

        // Тот же эффект — часы не дёргаем: разница в пару миллисекунд от кадра к кадру не нужна.
        if (old is not null && old.Mode == mode && old.Speed == speed && old.Direction == direction
            && Math.Abs(old.Scale - scale) < 0.001 && Math.Abs(old.Origin - origin) < 40)
            return;

        _custom = new CustomState(mode, speed, direction, scale, origin);
    }

    /// <summary>
    /// Цвет своего радужного эффекта в точке вентилятора (как его видно глазом, с яркостью клавиатуры).
    /// false — такого эффекта сейчас нет (или кадры перестали идти).
    /// </summary>
    public static bool CustomSample(double x, double y, double ring, out byte r, out byte g, out byte b)
    {
        r = g = b = 0;
        if (_custom is not { } c) return false;
        if (_live is null || Clock.Ms - Interlocked.Read(ref _liveAt) >= LiveTimeoutMs) return false;

        var seconds = (Clock.Ms - c.Origin) / 1000.0;
        var (lr, lg, lb) = LightEffects.RainbowAt(c.Mode, x, y, ring, seconds, c.Speed, c.Direction);
        r = FromLed[(int)Math.Round(lr * c.Scale)];
        g = FromLed[(int)Math.Round(lg * c.Scale)];
        b = FromLed[(int)Math.Round(lb * c.Scale)];
        return true;
    }

    /// <summary>
    /// Цвета клавиш, как их видно глазом: таблица 576 байт, клавиша [код, R, G, B] по смещению код×4.
    /// null — программа ещё ничего не отправляла. <paramref name="approximate"/> — это эффект прошивки:
    /// его кадры рисует сама клавиатура, а здесь — похожая картинка.
    /// </summary>
    public static byte[]? Display(out bool approximate)
    {
        approximate = false;

        var live = _live;
        if (live is not null && Clock.Ms - Interlocked.Read(ref _liveAt) < LiveTimeoutMs)
            return Cached(live, FromLive);

        if (_user is { } user) return Cached(user, FromBank);

        if (_effect is { } effect)
        {
            // Ровные режимы (статичный цвет, выключено) показываются точно.
            approximate = effect.Mode is not (0 or 1 or 3) && effect.Brightness > 0;
            return Simulate(effect);
        }

        return null;
    }

    private static byte[] NewBank() => new byte[9 * 64];

    /// <summary>
    /// Таблицу из того же кадра не пересчитываем: её спрашивают и схема клавиатуры (20 раз в секунду),
    /// и вентиляторы (30 раз в секунду), а кадр меняется реже. Читатели таблицу только читают.
    /// </summary>
    private static (byte[] Source, byte[] Bank)? _cache;

    private static byte[] Cached(byte[] source, Func<byte[], byte[]> convert)
    {
        var cache = _cache;
        if (cache is { } c && ReferenceEquals(c.Source, source)) return c.Bank;

        var bank = convert(source);
        _cache = (source, bank);
        return bank;
    }

    private static void Put(byte[] bank, byte code, byte r, byte g, byte b)
    {
        var at = code * 4;
        if (at + 3 >= bank.Length) return;
        bank[at] = code;
        bank[at + 1] = r;
        bank[at + 2] = g;
        bank[at + 3] = b;
    }

    private static byte[] FromLive(byte[] live)
    {
        var bank = NewBank();
        for (var i = 0; i + 3 < live.Length; i += 4)
        {
            var code = live[i];
            if (code == 0) break;
            Put(bank, code, FromLed[live[i + 1]], FromLed[live[i + 2]], FromLed[live[i + 3]]);
        }
        return bank;
    }

    private static byte[] FromBank(byte[] source)
    {
        var bank = NewBank();
        for (var code = 1; code * 4 + 3 < Math.Min(source.Length, bank.Length); code++)
        {
            var at = code * 4;
            if (source[at] != code) continue;
            Put(bank, (byte)code, FromLed[source[at + 1]], FromLed[source[at + 2]], FromLed[source[at + 3]]);
        }
        return bank;
    }

    /// <summary>
    /// Эффекты прошивки клавиатура рисует сама, программе кадры не известны — показываем похожую картинку:
    /// тот же цвет (или радугу), яркость и примерно такой же рисунок движения.
    /// </summary>
    private static byte[] Simulate(EffectState e)
    {
        var bank = NewBank();
        foreach (var key in KeyPaint.Keys)
        {
            var x = (key.X + key.W / 2 - KeyPaint.OriginX) / KeyPaint.Width;   // 0 — левый край, 1 — правый
            var y = (key.Y + key.H / 2 - KeyPaint.OriginY) / KeyPaint.Height;  // 0 — верх, 1 — низ
            var (r, g, b) = EffectAt(e, x, y, Hash(key.Code));
            Put(bank, key.Code, r, g, b);
        }

        return bank;
    }

    /// <summary>
    /// Сейчас горит встроенный эффект прошивки (а не своя раскраска или живые кадры) — его номер, иначе null.
    /// </summary>
    public static int? ActiveEffectMode
    {
        get
        {
            if (_live is not null && Clock.Ms - Interlocked.Read(ref _liveAt) < LiveTimeoutMs) return null;
            if (_user is not null) return null;
            return _effect?.Mode;
        }
    }

    /// <summary>
    /// Цвет эффекта прошивки в любой точке (x, y — 0…1 по ширине и высоте клавиатуры), как его видно глазом.
    /// Для вентиляторов: им не обязательно стоять там, где клавиши. false — эффекта прошивки сейчас нет.
    /// </summary>
    public static bool EffectSample(double x, double y, double seed, out byte r, out byte g, out byte b)
    {
        r = g = b = 0;
        if (ActiveEffectMode is null || _effect is not { } e) return false;

        (r, g, b) = EffectAt(e, x, y, seed);
        return true;
    }

    /// <summary>Подгонка скорости под настоящую клавиатуру: «режим:скорость» → множитель (по замеру пользователя).</summary>
    private static volatile Dictionary<string, double> _rates = new();

    public static void SetRates(Dictionary<string, double>? rates) => _rates = new Dictionary<string, double>(rates ?? new());

    private static double RateFor(int mode, int speed) => _rates.TryGetValue($"{mode}:{speed}", out var k) ? k : DefaultRate(mode);

    /// <summary>Подгонка по замеру на настоящей клавиатуре (AK820 Pro): «Течение» — ×0,92.</summary>
    private static double DefaultRate(int mode) => mode switch
    {
        16 => 0.92,
        _ => 1
    };

    /// <summary>Скорость и режим эффекта, который горит сейчас (для подгонки), иначе null.</summary>
    public static (int Mode, int Speed)? ActiveEffect =>
        ActiveEffectMode is not null && _effect is { } e ? (e.Mode, e.Speed) : null;

    /// <summary>
    /// Сколько «фаз» схемы занимает один круг эффекта в одной точке (цвет или яркость повторяются).
    /// null — эффект не повторяется сам (статичный, от нажатий).
    /// </summary>
    private static double? CirclePhase(int mode) => mode switch
    {
        8 => 360.0 / 140,
        6 => 360.0 / 60,
        9 or 10 or 11 or 12 or 16 or 18 => 360.0 / 120,
        4 => 1 / 1.7,
        5 => 1 / 1.4,
        7 => 1 / 0.8,
        17 => 1 / 1.3,
        19 => 2 / 0.8,
        _ => null
    };

    public static bool CanCalibrate(int mode) => CirclePhase(mode) is not null;

    /// <summary>
    /// Замер: на клавиатуре прошло <paramref name="circles"/> полных кругов за <paramref name="seconds"/> с.
    /// Возвращает новый множитель скорости для текущего «режим:скорость».
    /// </summary>
    public static double? Calibrate(double seconds, int circles)
    {
        if (_effect is not { } e || CirclePhase(e.Mode) is not { } phases || circles < 1 || seconds / circles < 0.5) return null;

        var basePeriod = phases / (0.15 + 0.12 * e.Speed);   // секунд на круг при множителе 1
        var rate = Math.Round(Math.Clamp(basePeriod * circles / seconds, 0.1, 10), 3);
        SetRate(e.Mode, e.Speed, rate);
        return rate;
    }

    /// <summary>Начать круг «Спектра» заново (клавиатура сейчас синяя).</summary>
    public static void RestartEffectClock()
    {
        if (_effect is { } e) _effect = e with { Since = Clock.Ms };
    }

    public static Dictionary<string, double> Rates => new(_rates);

    public static double RateOf(int mode, int speed) => RateFor(mode, speed);

    /// <summary>Ручная подгонка множителя скорости для «режим:скорость»; цвета не прыгают — фаза сохраняется.</summary>
    public static void SetRate(int mode, int speed, double rate)
    {
        rate = Math.Clamp(rate, 0.1, 10);
        var old = RateFor(mode, speed);
        var rates = new Dictionary<string, double>(_rates);
        if (Math.Abs(rate - DefaultRate(mode)) < 0.0005) rates.Remove($"{mode}:{speed}");
        else rates[$"{mode}:{speed}"] = rate;
        _rates = rates;

        if (_effect is { } e && e.Mode == mode && e.Speed == speed && Math.Abs(old - rate) > 0.0001)
        {
            var now = Clock.Ms;
            var elapsed = (now - e.Since) * old / rate;
            _effect = e with { Since = now - (long)Math.Round(elapsed) };
        }
    }

    /// <summary>С какого цвета клавиатура начинает «Спектр» после включения эффекта (240° — синий).</summary>
    private const double SpectrumStartHue = 240;

    private static (byte R, byte G, byte B) EffectAt(EffectState e, double x, double y, double seed)
    {
        var seconds = (Clock.Ms - e.Since) / 1000.0;
        var phase = seconds * (0.15 + 0.12 * e.Speed) * RateFor(e.Mode, e.Speed);
        var level = e.Mode == 0 ? 0 : Math.Pow(e.Brightness / 5.0, 1 / 2.2);
        var rainbow = e.Rainbow || e.Mode is 6 or 8;

        var d = Along(x, y, e.Direction);
        double k = 1;
        var hue = x * 300 - phase * 90;

        switch (e.Mode)
        {
            case 0: // выключено
                k = 0;
                break;
            case 2 or 13 or 14 or 15: // горят от нажатий — в покое почти темно
                k = 0.12;
                break;
            case 4: // мерцание
                k = 0.2 + 0.8 * Math.Pow(Wave(phase * 1.7 + seed), 3);
                break;
            case 5: // падение сверху вниз
                k = 0.12 + 0.88 * Math.Pow(Wave(y * 1.2 - phase * 1.4 + seed * 0.35), 4);
                break;
            case 6: // разноцветный
                hue = x * 360 + y * 60 - phase * 60;
                break;
            case 7: // дыхание
                k = 0.06 + 0.94 * Wave(phase * 0.8);
                break;
            case 8: // спектр — вся клавиатура меняет цвет: синий → бирюзовый → зелёный → жёлтый → красный → фиолетовый
                hue = SpectrumStartHue - phase * 140;
                break;
            case 9: // волна от центра
            {
                var r = Radius(x, y);
                k = 0.2 + 0.8 * Wave(r * 1.5 - phase);
                hue = r * 300 - phase * 120;
                break;
            }
            case 10 or 16: // прокрутка, течение
                k = e.Mode == 10 ? 0.25 + 0.75 * Wave(d - phase) : 0.55 + 0.45 * Wave(d - phase);
                hue = d * 360 - phase * 120;
                break;
            case 11 or 18: // перекат, наклон — диагональные полосы
            {
                var t = x * 0.75 + y * 0.45;
                k = 0.25 + 0.75 * Wave(t * 1.3 - phase);
                hue = t * 300 - phase * 120;
                break;
            }
            case 12: // вращение вокруг центра
            {
                var angle = Math.Atan2((y - 0.5) * 0.45, x - 0.5) / (2 * Math.PI);
                k = 0.25 + 0.75 * Wave(angle - phase);
                hue = angle * 360 - phase * 120;
                break;
            }
            case 17: // пульсация
                k = 0.15 + 0.85 * Math.Pow(Wave(phase * 1.3), 2);
                break;
            case 19: // шаттл — полоса ходит туда-обратно
            {
                var at = 1 - Math.Abs(phase * 0.8 % 2 - 1);
                k = Math.Max(0.12, 1 - Math.Abs(d - at) * 4);
                break;
            }
            default: // статичный цвет, «одна клавиша гаснет» и прочие ровные режимы
                hue = x * 300;
                break;
        }

        var (r0, g0, b0) = rainbow ? Hue(hue) : (e.R, e.G, e.B);
        var scale = level * Math.Clamp(k, 0, 1);
        return ((byte)Math.Round(r0 * scale), (byte)Math.Round(g0 * scale), (byte)Math.Round(b0 * scale));
    }

    /// <summary>Положение клавиши вдоль направления эффекта (0…1), направления — как в прошивке (0–3).</summary>
    private static double Along(double x, double y, int direction) => direction switch
    {
        1 => 1 - x,
        2 => y,
        3 => 1 - y,
        _ => x
    };

    private static double Radius(double x, double y)
    {
        var dx = (x - 0.5) * 2;
        var dy = (y - 0.5) * 0.9;
        return Math.Sqrt(dx * dx + dy * dy);
    }

    /// <summary>Плавная волна 0…1 с периодом 1.</summary>
    private static double Wave(double t) => 0.5 + 0.5 * Math.Cos(2 * Math.PI * t);

    private static double Hash(byte code) => code * 0.61803398875 % 1.0;

    private static (byte R, byte G, byte B) Hue(double h)
    {
        h = (h % 360 + 360) % 360;
        var x = 1 - Math.Abs(h / 60 % 2 - 1);
        var (r, g, b) = h switch
        {
            < 60 => (1d, x, 0d),
            < 120 => (x, 1d, 0d),
            < 180 => (0d, 1d, x),
            < 240 => (0d, x, 1d),
            < 300 => (x, 0d, 1d),
            _ => (1d, 0d, x)
        };
        return ((byte)Math.Round(r * 255), (byte)Math.Round(g * 255), (byte)Math.Round(b * 255));
    }

    private static byte[] BuildFromLed()
    {
        var table = new byte[256];
        for (var v = 0; v < 256; v++) table[v] = (byte)Math.Round(255 * Math.Pow(v / 255.0, 1 / 2.2));
        return table;
    }
}
