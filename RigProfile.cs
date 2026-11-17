using System.Text.Json;

namespace RigControl;

/// <summary>
/// Профиль одной кнопкой: подсветка клавиатуры, подсветка СЖО, вентиляторы, экран помпы и профиль мыши.
/// Какие части профиль меняет, задают флажки Use*: остальное при переключении не трогается.
/// </summary>
public sealed class RigProfile
{
    public string Name { get; set; } = "Профиль";

    /// <summary>Значок из Segoe MDL2 Assets.</summary>
    public string Glyph { get; set; } = "";

    public bool UseKeyboard { get; set; } = true;
    public bool UseArgb { get; set; } = true;
    public bool UseFans { get; set; } = true;
    public bool UsePump { get; set; }
    public bool UseMouse { get; set; }

    // Клавиатура
    public int Effect { get; set; } = 1;
    public string Color { get; set; } = "#FFFFFF";
    public bool Rainbow { get; set; }
    public int Brightness { get; set; } = 5;
    public int Speed { get; set; } = 3;
    public int Direction { get; set; }
    public bool SyncLight { get; set; }
    public int SyncStyle { get; set; }
    public Dictionary<string, string>? KeyPaint { get; set; }
    public int PaintAnim { get; set; }
    public int PaintSpeed { get; set; } = 2;

    // Подсветка СЖО
    public int ArgbMode { get; set; }
    public string ArgbColor { get; set; } = "#00B4FF";
    public string ArgbColor2 { get; set; } = "#FF00A8";
    public string ArgbColor3 { get; set; } = "#8A2BFF";
    public int ArgbBrightness { get; set; } = 100;
    public int ArgbSpeed { get; set; } = 3;
    public bool ArgbRainbow { get; set; } = true;

    // Вентиляторы
    public Dictionary<string, int>? FanModes { get; set; }
    public Dictionary<string, int>? FanPercent { get; set; }
    public Dictionary<string, int[]>? FanCurves { get; set; }
    public int FanModeAll { get; set; }

    // Экран помпы
    public List<LcdItem>? CoolerLayout { get; set; }
    public string? CoolerBackground { get; set; }
    public string? CoolerBgTop { get; set; }
    public string? CoolerBgBottom { get; set; }
    public int CoolerBrightness { get; set; } = 100;
    public int CoolerFps { get; set; } = 2;
    public string? CoolerTheme { get; set; }

    // Мышь: встроенный профиль 0–2 (null — не переключать)
    public int? MouseProfile { get; set; }

    /// <summary>Значки, из которых выбирают в редакторе профиля.</summary>
    public static readonly (string Glyph, string Name)[] Glyphs =
    {
        ("", "Игры"),
        ("", "Работа"),
        ("", "Тишина"),
        ("", "Избранное"),
        ("", "Мощность"),
        ("", "Музыка"),
        ("", "Видео"),
        ("", "Подсветка"),
    };

    /// <summary>Глубокая копия (списки и словари не делятся с настройками).</summary>
    public RigProfile Clone() =>
        JsonSerializer.Deserialize<RigProfile>(JsonSerializer.Serialize(this)) ?? new RigProfile();

    public static List<LcdItem>? CloneLayout(List<LcdItem>? layout) =>
        layout is null ? null : JsonSerializer.Deserialize<List<LcdItem>>(JsonSerializer.Serialize(layout));
}
