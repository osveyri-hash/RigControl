using System.Windows.Media;

namespace RigControl;

/// <summary>Шрифты программы: встроенный круглый kawaii-шрифт и кэш системных семейств.</summary>
public static class AppFonts
{
    /// <summary>Имя встроенного шрифта (Fonts\MPLUSRounded1c-*.ttf, SIL OFL) — так оно хранится в настройках.</summary>
    public const string Kawaii = "Rounded Mplus 1c";

    private static readonly Dictionary<string, FontFamily> Cache = new(StringComparer.OrdinalIgnoreCase);

    /// <summary>
    /// Имя из настроек → семейство. Встроенный шрифт берём из ресурсов сборки, остальные — из системы.
    /// Семейства кэшируем: экран помпы перерисовывается несколько раз в секунду.
    /// </summary>
    public static FontFamily Resolve(string? name)
    {
        if (string.IsNullOrWhiteSpace(name)) name = "Segoe UI";

        lock (Cache)
        {
            if (Cache.TryGetValue(name, out var cached)) return cached;

            var family = string.Equals(name, Kawaii, StringComparison.OrdinalIgnoreCase)
                ? new FontFamily(new Uri("pack://application:,,,/"), "./Fonts/#" + Kawaii)
                : new FontFamily(name);

            Cache[name] = family;
            return family;
        }
    }
}
