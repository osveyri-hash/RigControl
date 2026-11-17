namespace RigControl;

/// <summary>
/// «Своя раскраска»: свой цвет на каждую клавишу (как «Пользовательская подсветка»
/// в официальном конфигураторе). Геометрия клавиш — из rgb-keyboard.xml AJAZZ.
/// </summary>
public static class KeyPaint
{
    /// <summary>Клавиша на схеме: код подсветки прошивки, подпись и прямоугольник (px схемы).</summary>
    public readonly record struct KeyCap(byte Code, string Label, double X, double Y, double W, double H);

    /// <summary>Сдвиг координат XML на холст схемы (644×276) — клавиатура в XML начинается не с нуля.</summary>
    public const double OriginX = 77, OriginY = 4;
    public const double Width = 644, Height = 276;

    /// <summary>Цвет колпачка на схеме — как у набора кейкапов на клавиатуре пользователя:
    /// жёлтый Esc, серый F-ряд, белые клавиши и чёрные справа (там, где рисунок тёмный).</summary>
    public static string CapColor(byte code) => code switch
    {
        102 => "#F7F6F2",
        _ => CapColorBase(code)
    };

    /// <summary>Клавиши с рисунком. Esc и F-ряд — однотонные (рисуются цветом с подписью), «→» — белая.</summary>
    public static bool HasArt(byte code) => code is not ((>= 1 and <= 13) or 102);

    /// <summary>Экранчик клавиатуры на схеме (между PgDn и «→»), координаты XML.</summary>
    public const double ScreenX = 674, ScreenY = 184, ScreenSize = 32;

    /// <summary>Колёсико (роллер) над правой колонкой (Home/PgUp/PgDn), на уровне F-ряда; координаты XML.</summary>
    public const double KnobX = 691, KnobY = 36, KnobRadius = 18;

    private static string CapColorBase(byte code) => code switch
    {
        1 => "#F5C21B",
        >= 6 and <= 9 => "#8C8D8B",
        >= 2 and <= 13 => "#D3D3D0",
        30 or 31 or 47 or 49 or 55 or 66 or 85 or 83 or 84 or 101
            or 96 or 98 or 99 or 100 or 102 or 117 or 118 or 121 => "#1C1C1C",
        _ => "#F7F6F2"
    };

    public static readonly KeyCap[] Keys =
    {
        new(1, "Esc", 95, 22, 28, 28),
        new(2, "F1", 141, 22, 28, 28),
        new(3, "F2", 179, 22, 28, 28),
        new(4, "F3", 217, 22, 28, 28),
        new(5, "F4", 255, 22, 28, 28),
        new(6, "F5", 302, 22, 28, 28),
        new(7, "F6", 339, 22, 28, 28),
        new(8, "F7", 377, 22, 28, 28),
        new(9, "F8", 415, 22, 28, 28),
        new(10, "F9", 462, 22, 28, 28),
        new(11, "F10", 500, 22, 28, 28),
        new(12, "F11", 538, 22, 28, 28),
        new(13, "F12", 576, 22, 28, 28),
        new(119, "Del", 623, 22, 28, 28),
        new(19, "`", 95, 69, 28, 28),
        new(20, "1", 132, 69, 28, 28),
        new(21, "2", 170, 69, 28, 28),
        new(22, "3", 208, 69, 28, 28),
        new(23, "4", 246, 69, 28, 28),
        new(24, "5", 283, 69, 28, 28),
        new(25, "6", 321, 69, 28, 28),
        new(26, "7", 359, 69, 28, 28),
        new(27, "8", 397, 69, 28, 28),
        new(28, "9", 434, 69, 28, 28),
        new(29, "0", 472, 69, 28, 28),
        new(30, "-", 510, 69, 28, 28),
        new(31, "=", 547, 69, 28, 28),
        new(103, "⌫", 585, 69, 66, 28),
        new(117, "Home", 677, 69, 28, 28),
        new(37, "Tab", 95, 108, 46, 28),
        new(38, "Q", 151, 108, 28, 28),
        new(39, "W", 189, 108, 28, 28),
        new(40, "E", 227, 108, 28, 28),
        new(41, "R", 265, 108, 28, 28),
        new(42, "T", 302, 108, 28, 28),
        new(43, "Y", 340, 108, 28, 28),
        new(44, "U", 378, 108, 28, 28),
        new(45, "I", 416, 108, 28, 28),
        new(46, "O", 453, 108, 28, 28),
        new(47, "P", 491, 108, 28, 28),
        new(48, "[", 529, 108, 28, 28),
        new(49, "]", 566, 108, 28, 28),
        new(67, "\\", 604, 108, 47, 28),
        new(118, "PgUp", 677, 108, 28, 28),
        new(55, "Caps", 95, 145, 55, 28),
        new(56, "A", 160, 145, 28, 28),
        new(57, "S", 198, 145, 28, 28),
        new(58, "D", 236, 145, 28, 28),
        new(59, "F", 274, 145, 28, 28),
        new(60, "G", 312, 145, 28, 28),
        new(61, "H", 350, 145, 28, 28),
        new(62, "J", 387, 145, 28, 28),
        new(63, "K", 425, 145, 28, 28),
        new(64, "L", 462, 145, 28, 28),
        new(65, ";", 500, 145, 28, 28),
        new(66, "'", 538, 145, 28, 28),
        new(85, "Enter", 576, 145, 75, 28),
        new(121, "PgDn", 677, 145, 28, 28),
        new(73, "Shift", 95, 183, 75, 28),
        new(74, "Z", 179, 183, 28, 28),
        new(75, "X", 217, 183, 28, 28),
        new(76, "C", 255, 183, 28, 28),
        new(77, "V", 292, 183, 28, 28),
        new(78, "B", 330, 183, 28, 28),
        new(79, "N", 368, 183, 28, 28),
        new(80, "M", 406, 183, 28, 28),
        new(81, ",", 443, 183, 28, 28),
        new(82, ".", 481, 183, 28, 28),
        new(83, "/", 519, 183, 28, 28),
        new(84, "Shift", 557, 183, 55, 28),
        new(101, "↑", 631, 192, 28, 28),
        new(91, "Ctrl", 95, 221, 37, 28),
        new(92, "Win", 142, 221, 37, 28),
        new(93, "Alt", 189, 221, 37, 28),
        new(94, "", 236, 221, 225, 28),
        new(95, "Alt", 472, 221, 28, 28),
        new(96, "Fn", 510, 221, 28, 28),
        new(98, "Ctrl", 548, 221, 28, 28),
        new(99, "←", 594, 230, 28, 28),
        new(100, "↓", 631, 230, 28, 28),
        new(102, "→", 669, 230, 28, 28),
    };

    /// <summary>Готовые пастельные наборы: имя → функция «клавиша → цвет».</summary>
    public static readonly (string Name, Func<KeyCap, string> Color)[] Presets =
    {
        ("Сакура", k => k.Y < 60 ? "#FF2D7A" : "#FF7EB6"),
        ("Зефир", k => Lerp("#FF3D9A", "#7A3DFF", (k.X - OriginX) / Width)),
        ("Мята", k => Lerp("#00FFB0", "#00A0FF", (k.Y - OriginY) / Height)),
        ("Закат", k => Lerp("#FF9A00", "#FF0050", (k.X - OriginX) / Width)),
        ("Радуга", k => Hue((k.X - OriginX) / Width * 300)),
        ("WASD", k => k.Label is "W" or "A" or "S" or "D" ? "#FF0050" : "#6A3DFF"),
    };

    /// <summary>Режим прошивки «пользовательская подсветка» — как в официальной программе (снято с USB-лога).</summary>
    public const int UserLightMode = 0x80;

    /// <summary>
    /// Таблица цветов, как её шлёт официальная программа: 576 байт, на клавишу [код, R, G, B] по смещению код×4,
    /// в конце 0xAA 0x55. Код ставится только у настоящих клавиш, незакрашенные — честный 0,0,0.
    /// Яркость 0–5 применяем к самим цветам.
    /// </summary>
    public static byte[] BuildFrame(IReadOnlyDictionary<string, string>? paint, int brightness)
    {
        var scale = Math.Clamp(brightness, 0, 5) / 5.0;

        var frame = new byte[9 * 64];
        frame[^2] = 0xAA;
        frame[^1] = 0x55;

        foreach (var key in Keys)
        {
            var at = key.Code * 4;
            frame[at] = key.Code;

            if (paint is not null && paint.TryGetValue(key.Code.ToString(), out var hex))
            {
                var (r, g, b) = Ak820Protocol.ParseColor(hex, (0, 0, 0));
                frame[at + 1] = Led(r, scale);
                frame[at + 2] = Led(g, scale);
                frame[at + 3] = Led(b, scale);
            }
        }

        return frame;
    }

    /// <summary>Анимации поверх раскраски (индекс = Settings.PaintAnim). 0 — без анимации, хранится в прошивке.</summary>
    public static readonly string[] Animations =
    {
        "Без анимации", "Дыхание", "Волна", "Переливы", "Мерцание", "Пульс из центра"
    };

    /// <summary>
    /// Кадр раскраски с анимацией в момент <paramref name="seconds"/>. Рисует программа и шлёт кадрами,
    /// как свои эффекты, — поэтому работает только по кабелю. Клавиши не гаснут полностью.
    /// </summary>
    public static byte[] Animate(IReadOnlyDictionary<string, string>? paint, int brightness, int animation,
        double seconds, int speed)
    {
        var scale = Math.Clamp(brightness, 0, 5) / 5.0;
        var rate = 0.15 + 0.12 * Math.Clamp(speed, 0, 5);
        var phase = seconds * rate;

        var frame = new byte[9 * 64];
        frame[^2] = 0xAA;
        frame[^1] = 0x55;

        foreach (var key in Keys)
        {
            var at = key.Code * 4;
            frame[at] = key.Code;
            if (paint is null || !paint.TryGetValue(key.Code.ToString(), out var hex)) continue;

            var (r, g, b) = Ak820Protocol.ParseColor(hex, (0, 0, 0));
            var x = (key.X + key.W / 2 - OriginX) / Width;
            var y = (key.Y + key.H / 2 - OriginY) / Height;
            double k = 1;

            switch (animation)
            {
                case 1: // дыхание — вся раскраска плавно дышит
                    k = Wave01(phase);
                    break;
                case 2: // волна яркости слева направо
                    k = Wave01(x * 1.2 - phase);
                    break;
                case 3: // переливы — цвета плывут по кругу оттенков
                {
                    var (h, sat, val) = ColorWheel.ToHsv(System.Windows.Media.Color.FromRgb(r, g, b));
                    var c = ColorWheel.FromHsv(h + phase * 120 + x * 40, sat, val);
                    (r, g, b) = (c.R, c.G, c.B);
                    break;
                }
                case 4: // мерцание — у каждой клавиши свой ритм
                    k = Wave01(phase * 1.7 + Hash(key.Code));
                    break;
                case 5: // пульс расходится кругами от центра
                {
                    var dx = (x - 0.5) * 2.3;
                    var dy = y - 0.5;
                    k = Wave01(Math.Sqrt(dx * dx + dy * dy) * 1.4 - phase);
                    break;
                }
            }

            var level = scale * (animation is 0 or 3 ? 1 : 0.22 + 0.78 * k);
            frame[at + 1] = Led(r, level);
            frame[at + 2] = Led(g, level);
            frame[at + 3] = Led(b, level);
        }

        return frame;
    }

    /// <summary>Плавная волна 0…1 с периодом 1.</summary>
    private static double Wave01(double t) => 0.5 + 0.5 * Math.Cos(2 * Math.PI * t);

    private static double Hash(byte code) => (code * 0.61803398875) % 1.0;

    /// <summary>
    /// Цвет экрана → яркость светодиода. Экран показывает цвета с гамма-коррекцией, а светодиод светит
    /// линейно: без пересчёта средние значения (например, 0x7E в розовом #FF7EB6) горят почти в полную силу,
    /// и цвета выглядят бледными, почти белыми. Гамма 2,2 — та же, что у sRGB.
    /// </summary>
    private static byte Led(byte value, double scale) =>
        (byte)Math.Round(255 * Math.Pow(value / 255.0, 2.2) * scale);

    private static string Lerp(string from, string to, double t)
    {
        t = Math.Clamp(t, 0, 1);
        var a = Ak820Protocol.ParseColor(from);
        var b = Ak820Protocol.ParseColor(to);

        static byte Mix(byte x, byte y, double t) => (byte)Math.Round(x + (y - x) * t);
        return $"#{Mix(a.R, b.R, t):X2}{Mix(a.G, b.G, t):X2}{Mix(a.B, b.B, t):X2}";
    }

    /// <summary>Чистый оттенок по кругу (насыщенность и яркость 100 %).</summary>
    private static string Hue(double h)
    {
        h = (h % 360 + 360) % 360;
        const double s = 1.0, v = 1.0;
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

        return $"#{(byte)Math.Round((r + m) * 255):X2}{(byte)Math.Round((g + m) * 255):X2}{(byte)Math.Round((b + m) * 255):X2}";
    }
}
