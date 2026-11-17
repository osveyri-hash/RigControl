using System.IO;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Shapes;

namespace RigControl.Devices;

/// <summary>
/// Картинки устройств для плиток. Порядок: свой файл рядом с программой (Devices\mouse.png и т. п.),
/// затем картинка, вшитая в программу (Assets\Devices), иначе — аккуратный значок.
/// </summary>
public static class DeviceArt
{
    public const string Mouse = "mouse", Keyboard = "keyboard", Pump = "pump", Argb = "argb", Fans = "fans";

    public static FrameworkElement Picture(string key, string glyph)
    {
        var image = LoadImage(key);
        if (image is not null)
        {
            var img = new Image { Source = image, Stretch = Stretch.Uniform, MaxHeight = 164, SnapsToDevicePixels = true };
            RenderOptions.SetBitmapScalingMode(img, BitmapScalingMode.HighQuality);
            return img;
        }

        return Badge(glyph);
    }

    // Одна декодированная картинка на устройство: плитки, шапка и пустой экран мыши делят один BitmapSource.
    private static readonly Dictionary<string, BitmapSource?> Cache = new();

    // Копии картинок из папки Devices прошлых версий (2.15.x) — теперь они вшиты и уменьшены, большие файлы не грузим.
    private static readonly HashSet<long> OldCopies = new() { 391805, 247677, 1131496, 1020563, 1681914 };

    private static BitmapSource? LoadImage(string key)
    {
        if (Cache.TryGetValue(key, out var cached)) return cached;
        var image = LoadExternal(key) ?? LoadEmbedded(key);
        Cache[key] = image;
        return image;
    }

    private static BitmapImage? LoadExternal(string key)
    {
        foreach (var ext in new[] { ".png", ".webp", ".jpg" })
        {
            try
            {
                var file = new FileInfo(System.IO.Path.Combine(AppContext.BaseDirectory, "Devices", key + ext));
                if (!file.Exists || (ext == ".png" && OldCopies.Contains(file.Length))) continue;
                return Decode(file.OpenRead(), 400);   // своя картинка может быть огромной — декодируем сразу в нужном размере
            }
            catch (Exception ex)
            {
                // своя картинка не открылась — возьмём встроенную
                QuietLog.Write(ex);
            }
        }

        return null;
    }

    private static BitmapImage? LoadEmbedded(string key)
    {
        try
        {
            var info = Application.GetResourceStream(new Uri($"pack://application:,,,/Assets/Devices/{key}.png"));
            if (info is not null) return Decode(info.Stream, 0);
        }
        catch (Exception ex)
        {
            QuietLog.Write(ex);
        }

        return null;
    }

    private static BitmapImage Decode(Stream stream, int height)
    {
        using (stream)
        {
            var bitmap = new BitmapImage();
            bitmap.BeginInit();
            bitmap.CacheOption = BitmapCacheOption.OnLoad;
            if (height > 0) bitmap.DecodePixelHeight = height;
            bitmap.StreamSource = stream;
            bitmap.EndInit();
            bitmap.Freeze();
            return bitmap;
        }
    }

    /// <summary>Значок в круге (пока нет картинки).</summary>
    private static Viewbox Badge(string glyph)
    {
        var grid = new Grid { Width = 150, Height = 150 };
        var circle = new Ellipse();
        circle.SetResourceReference(Shape.FillProperty, "AccentDimBrush");
        grid.Children.Add(circle);

        var icon = new TextBlock
        {
            Text = glyph, FontFamily = new FontFamily("Segoe MDL2 Assets"), FontSize = 64,
            HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center,
        };
        icon.SetResourceReference(TextBlock.ForegroundProperty, "AccentBrush");
        grid.Children.Add(icon);

        return new Viewbox { Child = grid, Stretch = Stretch.Uniform, MaxHeight = 140 };
    }
}
