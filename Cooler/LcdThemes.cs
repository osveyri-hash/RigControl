namespace RigControl;

/// <summary>
/// Готовое оформление экрана помпы: фон-градиент и расстановка объектов.
/// <see cref="Items"/> — для горизонтального экрана 320×240, <see cref="Portrait"/> — для вертикального 240×320
/// (поворот 90°/270°). Без отдельной вертикальной раскладки горизонтальная растягивается.
/// </summary>
/// <param name="Image">Встроенная фоновая картинка темы (pack://…) или null — только градиент.</param>
/// <param name="Brightness">Яркость фона, которую ставит тема (0–100); null — не трогать.</param>
public sealed record LcdTheme(string Name, string Top, string Bottom, Func<List<LcdItem>> Items,
    Func<List<LcdItem>>? Portrait = null, string? Image = null, int? Brightness = null);

public static class LcdThemes
{
    private static LcdItem Make(LcdKind kind, double x, double y, double scale, string color,
        string font = "Segoe UI", bool bold = true, bool label = true, bool seconds = false)
    {
        var item = LcdItem.Create(kind);
        item.X = x;
        item.Y = y;
        item.Scale = scale;
        item.Color = color;
        item.Font = font;
        item.Bold = kind is LcdKind.Date or LcdKind.Weekday ? false : bold;
        item.Label = label;
        item.Seconds = seconds;
        return item;
    }

    /// <summary>Та же «классика» для вертикального экрана 240×320.</summary>
    private static List<LcdItem> PortraitClassic(string time, string date, string cpu, string gpu, string ram,
        string font = "Segoe UI", string? temp = "") => new()
    {
        Make(LcdKind.Time, 18, 12, 1.45, time, font),
        Make(LcdKind.Date, 22, 90, 1.0, date, font),
        Make(LcdKind.Weekday, 22, 114, 0.9, date, font),
        Make(LcdKind.CpuLoad, 18, 156, 0.95, cpu, font),
        Make(LcdKind.CpuTemp, 126, 156, 0.95, temp ?? "", font),
        Make(LcdKind.GpuLoad, 18, 202, 0.95, gpu, font),
        Make(LcdKind.GpuTemp, 126, 202, 0.95, temp ?? "", font),
        Make(LcdKind.Ram, 18, 258, 0.85, ram, font)
    };

    /// <summary>
    /// Расстановка по умолчанию (бывшая «Классика»): для «Сбросить» и для смены ориентации,
    /// если прошлой темы больше нет в списке. В списке тем её нет.
    /// </summary>
    public static readonly LcdTheme Default = new("Классика", "#10121A", "#05060A", LcdItem.DefaultLayout,
        () => PortraitClassic("#FFFFFF", "#8A90A0", "#4DA3FF", "#7BD36B", "#C084FF"));

    public static readonly LcdTheme[] All =
    {
        // В стиле ASCII-арта: моноширинный шрифт, серый «терминал» и розовые акценты.
        new("ASCII", "#F4F4F4", "#DCDCDC", () => Ascii("#262626", "#6E6E6E", "#D94C86", "#3A3A3A"),
            () => AsciiPortrait("#262626", "#6E6E6E", "#D94C86", "#3A3A3A"),
            "pack://application:,,,/Assets/AsciiArt.jpg", 100),

        new("ASCII ночь", "#1C1C1C", "#0C0C0C", () => Ascii("#F2F2F2", "#9A9A9A", "#FF8FB8", "#D6D6D6"),
            () => AsciiPortrait("#F2F2F2", "#9A9A9A", "#FF8FB8", "#D6D6D6"),
            "pack://application:,,,/Assets/AsciiArtDark.jpg", 70),

        new("Терминал", "#161616", "#0A0A0A", () => Ascii("#E6E6E6", "#7A7A7A", "#FF8FB8", "#BDBDBD"),
            () => AsciiPortrait("#E6E6E6", "#7A7A7A", "#FF8FB8", "#BDBDBD")),

        // Ещё два ASCII-арта пользователя: светлый текст поверх приглушённой картинки.
        new("Манга", "#141414", "#080808", () => Ascii("#FFFFFF", "#B4B4B4", "#FF8FB8", "#E6E6E6"),
            () => AsciiPortrait("#FFFFFF", "#B4B4B4", "#FF8FB8", "#E6E6E6"),
            "pack://application:,,,/Assets/ArtMangaDark.jpg", 55),

        new("Точки", "#141414", "#080808", () => Ascii("#FFFFFF", "#B4B4B4", "#FF8FB8", "#E6E6E6"),
            () => AsciiPortrait("#FFFFFF", "#B4B4B4", "#FF8FB8", "#E6E6E6"),
            "pack://application:,,,/Assets/ArtDotsDark.jpg", 60),

        // Аниме-темы: картинка или гифка на фоне, текст тёмный поверх светлой вуали.
        new("Хина", "#8E6A7E", "#5A3E4F", () => Anime("#5E4652", "#A2798A", "#D9608D"),
            () => AnimePortrait("#5E4652", "#A2798A", "#D9608D"),
            "pack://application:,,,/Assets/AnimeHina.gif", 100),

        new("Кружево", "#8C7A82", "#5A4A52", () => Anime("#57474F", "#9C8894", "#B0798F"),
            () => AnimePortrait("#57474F", "#9C8894", "#B0798F"),
            "pack://application:,,,/Assets/AnimeLace.gif", 100),

        new("Взгляд", "#8C7A82", "#5A4A52", () => Anime("#57474F", "#9C8894", "#B0798F"),
            () => AnimePortrait("#57474F", "#9C8894", "#B0798F"),
            "pack://application:,,,/Assets/AnimeEyes.gif", 100),

        new("Ушки", "#B08A76", "#6B4A3F", () => Anime("#6B4A3F", "#A98476", "#E0798F"),
            () => AnimePortrait("#6B4A3F", "#A98476", "#E0798F"),
            "pack://application:,,,/Assets/AnimeEars.gif", 100),

        new("Мейд", "#8A8492", "#4E4550", () => Anime("#4E4550", "#908494", "#8D7A95"),
            () => AnimePortrait("#4E4550", "#908494", "#8D7A95"),
            "pack://application:,,,/Assets/AnimeMaid.gif", 100),

        new("Тепло", "#7A5A4E", "#402822", () => Anime("#FFF3EA", "#E9CEC2", "#FFC1A0"),
            () => AnimePortrait("#FFF3EA", "#E9CEC2", "#FFC1A0"),
            "pack://application:,,,/Assets/AnimeWarm.jpg", 100)
    };

    /// <summary>Раскладка «аниме»: время сверху, датчики столбиком — так картинка на фоне остаётся видна.</summary>
    /// <summary>Раскладка «аниме»: время сверху, датчики столбиком — так картинка на фоне остаётся видна.</summary>
    private static List<LcdItem> Anime(string time, string muted, string accent) => new()
    {
        Make(LcdKind.Time, 12, 4, 1.25, time, AppFonts.Kawaii),
        Make(LcdKind.Date, 14, 66, 0.85, muted, AppFonts.Kawaii),
        Make(LcdKind.Weekday, 112, 66, 0.8, muted, AppFonts.Kawaii),
        Make(LcdKind.CpuLoad, 10, 96, 0.78, accent, AppFonts.Kawaii),
        Make(LcdKind.CpuTemp, 168, 96, 0.78, accent, AppFonts.Kawaii),
        Make(LcdKind.GpuLoad, 10, 124, 0.78, accent, AppFonts.Kawaii),
        Make(LcdKind.GpuTemp, 168, 124, 0.78, accent, AppFonts.Kawaii),
        Make(LcdKind.Ram, 10, 152, 0.72, time, AppFonts.Kawaii)
    };

    private static List<LcdItem> AnimePortrait(string time, string muted, string accent) => new()
    {
        Make(LcdKind.Time, 10, 6, 1.25, time, AppFonts.Kawaii),
        Make(LcdKind.Date, 12, 64, 0.85, muted, AppFonts.Kawaii),
        Make(LcdKind.Weekday, 100, 64, 0.8, muted, AppFonts.Kawaii),
        Make(LcdKind.CpuLoad, 8, 92, 0.75, accent, AppFonts.Kawaii),
        Make(LcdKind.CpuTemp, 128, 92, 0.75, accent, AppFonts.Kawaii),
        Make(LcdKind.GpuLoad, 8, 118, 0.75, accent, AppFonts.Kawaii),
        Make(LcdKind.GpuTemp, 128, 118, 0.75, accent, AppFonts.Kawaii),
        Make(LcdKind.Ram, 8, 144, 0.7, time, AppFonts.Kawaii)
    };

    private const string Mono = "Consolas";

    private static List<LcdItem> Ascii(string time, string muted, string accent, string text) => new()
    {
        Make(LcdKind.Time, 16, 6, 1.4, time, Mono),
        Make(LcdKind.Date, 20, 82, 0.9, muted, Mono),
        Make(LcdKind.Weekday, 126, 82, 0.9, muted, Mono),
        Make(LcdKind.CpuLoad, 16, 112, 0.85, accent, Mono),
        Make(LcdKind.CpuTemp, 168, 112, 0.85, "", Mono),
        Make(LcdKind.GpuLoad, 16, 146, 0.85, accent, Mono),
        Make(LcdKind.GpuTemp, 168, 146, 0.85, "", Mono),
        Make(LcdKind.Ram, 16, 182, 0.75, text, Mono),
        Make(LcdKind.NetDown, 16, 214, 0.6, muted, Mono),
        Make(LcdKind.NetUp, 168, 214, 0.6, muted, Mono)
    };

    private static List<LcdItem> AsciiPortrait(string time, string muted, string accent, string text) => new()
    {
        Make(LcdKind.Time, 14, 10, 1.25, time, Mono),
        Make(LcdKind.Date, 18, 80, 0.9, muted, Mono),
        Make(LcdKind.Weekday, 18, 104, 0.9, muted, Mono),
        Make(LcdKind.CpuLoad, 14, 142, 0.85, accent, Mono),
        Make(LcdKind.CpuTemp, 124, 142, 0.85, "", Mono),
        Make(LcdKind.GpuLoad, 14, 180, 0.85, accent, Mono),
        Make(LcdKind.GpuTemp, 124, 180, 0.85, "", Mono),
        Make(LcdKind.Ram, 14, 222, 0.75, text, Mono),
        Make(LcdKind.NetDown, 14, 262, 0.65, muted, Mono),
        Make(LcdKind.NetUp, 14, 288, 0.65, muted, Mono)
    };
}
