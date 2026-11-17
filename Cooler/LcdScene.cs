using System.Globalization;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Documents;
using System.Windows.Media;
using System.Windows.Media.Imaging;

namespace RigControl;

/// <summary>
/// Сцена экрана помпы из обычных WPF-элементов. Одна и та же сборка используется и для интерактивного
/// предпросмотра в окне, и для кадра, который уходит на помпу, — поэтому на экране выходит ровно то, что видно в редакторе.
/// </summary>
public static class LcdScene
{
    private static readonly Brush Muted = Frozen(Color.FromRgb(0x8A, 0x90, 0xA0));

    /// <summary>Наполняет холст сценой и возвращает соответствие «объект → его текстовый элемент».</summary>
    public static Dictionary<LcdItem, FrameworkElement> Build(Canvas canvas, double width, double height,
        IEnumerable<LcdItem> items, Snapshot snapshot, DateTime now, BitmapSource? background, double brightness,
        string? bgTop = null, string? bgBottom = null)
    {
        canvas.Children.Clear();
        canvas.Width = width;
        canvas.Height = height;
        canvas.ClipToBounds = true;
        canvas.Background = Brushes.Black;

        var layer = new Grid
        {
            Width = width,
            Height = height,
            ClipToBounds = true,
            Opacity = Math.Clamp(brightness, 0, 1)
        };

        if (background is not null)
        {
            layer.Children.Add(new Image
            {
                Source = background,
                Stretch = Stretch.UniformToFill,
                Width = width,
                Height = height
            });
        }
        else
        {
            var top = TryParseColor(bgTop ?? "", out var topColor) ? topColor : Color.FromRgb(0x10, 0x12, 0x1A);
            var bottom = TryParseColor(bgBottom ?? "", out var bottomColor) ? bottomColor : Color.FromRgb(0x05, 0x06, 0x0A);
            layer.Background = new LinearGradientBrush(top, bottom, 90);
        }

        canvas.Children.Add(layer);

        var map = new Dictionary<LcdItem, FrameworkElement>();
        foreach (var item in items)
        {
            FrameworkElement block = item.IsBar() ? CreateBar(item, snapshot) : CreateText(item, snapshot, now);
            Canvas.SetLeft(block, item.X);
            Canvas.SetTop(block, item.Y);
            canvas.Children.Add(block);
            map[item] = block;
        }

        return map;
    }

    /// <summary>Рисует сцену «за кадром» в картинку width×height.</summary>
    public static BitmapSource Render(int width, int height, IEnumerable<LcdItem> items, Snapshot snapshot,
        DateTime now, BitmapSource? background, double brightness, string? bgTop = null, string? bgBottom = null)
    {
        var canvas = new Canvas();
        Build(canvas, width, height, items, snapshot, now, background, brightness, bgTop, bgBottom);

        canvas.Measure(new Size(width, height));
        canvas.Arrange(new Rect(0, 0, width, height));
        canvas.UpdateLayout();

        var bitmap = new RenderTargetBitmap(width, height, 96, 96, PixelFormats.Pbgra32);
        bitmap.Render(canvas);
        bitmap.Freeze();
        return bitmap;
    }

    /// <summary>
    /// Отпечаток содержимого сцены: если он не поменялся, кадр можно не перерисовывать.
    /// Учитывает и расстановку, и показываемые значения.
    /// </summary>
    public static string Signature(IEnumerable<LcdItem> items, Snapshot snapshot, DateTime now)
    {
        var sb = new System.Text.StringBuilder(256);
        foreach (var item in items)
        {
            sb.Append((int)item.Kind).Append('|').Append(item.X).Append('|').Append(item.Y).Append('|')
              .Append(item.Scale).Append('|').Append(item.Color).Append('|').Append(item.Font).Append('|')
              .Append(item.Bold).Append(item.Label).Append(item.Seconds).Append('|').Append(item.Text).Append('|');
            if (item.IsBar())
                sb.Append(Math.Round(BarValue(item, snapshot) ?? -1));
            else
                sb.Append(ValueText(item, snapshot, now));
            if (string.IsNullOrWhiteSpace(item.Color) && item.Kind is LcdKind.CpuTemp or LcdKind.GpuTemp)
                sb.Append(Heat(item.Kind == LcdKind.CpuTemp ? snapshot.CpuTemp : snapshot.GpuTemp));
            sb.Append(';');
        }
        return sb.ToString();
    }

    private static double? BarValue(LcdItem item, Snapshot s) => item.Kind switch
    {
        LcdKind.CpuBar => s.CpuLoad,
        LcdKind.GpuBar => s.GpuLoad,
        LcdKind.RamBar => s.RamLoad,
        _ => null
    };

    /// <summary>Полоска загрузки: дорожка, заполнение и подпись с процентом.</summary>
    private static StackPanel CreateBar(LcdItem item, Snapshot snapshot)
    {
        var scale = Math.Max(0.3, item.Scale);
        var width = 140 * scale;
        var height = 14 * scale;
        var value = Math.Clamp(BarValue(item, snapshot) ?? 0, 0, 100);
        var brush = ItemBrush(item, snapshot);
        var family = AppFonts.Resolve(item.Font);

        var panel = new StackPanel { Orientation = Orientation.Horizontal };

        if (item.Label)
        {
            panel.Children.Add(new TextBlock
            {
                Text = item.LabelText(),
                FontFamily = family,
                FontSize = Math.Max(6, height * 0.95),
                Foreground = Muted,
                VerticalAlignment = VerticalAlignment.Center,
                Margin = new Thickness(0, 0, 6 * scale, 0)
            });
        }

        var track = new Grid { Width = width, Height = height, VerticalAlignment = VerticalAlignment.Center };
        track.Children.Add(new Border
        {
            CornerRadius = new CornerRadius(height / 2),
            Background = Frozen(Color.FromArgb(0x40, 0xFF, 0xFF, 0xFF))
        });
        track.Children.Add(new Border
        {
            Width = Math.Max(height, width * value / 100.0),
            HorizontalAlignment = HorizontalAlignment.Left,
            CornerRadius = new CornerRadius(height / 2),
            Background = brush,
            Opacity = value <= 0 ? 0 : 1
        });
        panel.Children.Add(track);

        panel.Children.Add(new TextBlock
        {
            Text = BarValue(item, snapshot) is null ? "—" : $"{value:0}%",
            FontFamily = family,
            FontSize = Math.Max(6, height * 0.95),
            FontWeight = item.Bold ? FontWeights.SemiBold : FontWeights.Normal,
            Foreground = brush,
            VerticalAlignment = VerticalAlignment.Center,
            Margin = new Thickness(6 * scale, 0, 0, 0)
        });

        return panel;
    }

    private static TextBlock CreateText(LcdItem item, Snapshot snapshot, DateTime now)
    {
        var size = Math.Max(6, item.BaseSize() * item.Scale);
        var family = AppFonts.Resolve(item.Font);

        var block = new TextBlock
        {
            FontFamily = family,
            FontSize = size,
            FontWeight = item.Bold ? FontWeights.SemiBold : FontWeights.Normal,
            Foreground = ItemBrush(item, snapshot),
            Background = Brushes.Transparent,
            TextWrapping = TextWrapping.NoWrap
        };

        if (item.HasLabel() && item.Label)
        {
            block.Inlines.Add(new Run(item.LabelText() + " ")
            {
                FontSize = Math.Max(6, size * 0.5),
                FontWeight = FontWeights.Normal,
                Foreground = Muted
            });
        }

        block.Inlines.Add(new Run(ValueText(item, snapshot, now)));
        return block;
    }

    private static string ValueText(LcdItem item, Snapshot s, DateTime now) => item.Kind switch
    {
        LcdKind.Time => now.ToString(item.Seconds ? "HH:mm:ss" : "HH:mm", CultureInfo.InvariantCulture),
        LcdKind.Date => now.ToString("dd.MM.yyyy", CultureInfo.InvariantCulture),
        LcdKind.CpuLoad => $"{s.CpuLoad:0}%",
        LcdKind.CpuTemp => s.CpuTemp is { } cpuTemp ? $"{cpuTemp:0}°" : "—",
        LcdKind.GpuLoad => s.GpuLoad is { } gpuLoad ? $"{gpuLoad:0}%" : "—",
        LcdKind.GpuTemp => s.GpuTemp is { } gpuTemp ? $"{gpuTemp:0}°" : "—",
        LcdKind.Ram => s.RamTotalGb > 0 ? $"{s.RamUsedGb:0.0}/{s.RamTotalGb:0} ГБ" : "—",
        LcdKind.Weekday => Ru.DateTimeFormat.GetDayName(now.DayOfWeek) is { Length: > 0 } day
            ? char.ToUpper(day[0]) + day[1..]
            : "",
        LcdKind.CpuClock => s.CpuClockMhz is { } mhz ? $"{mhz / 1000:0.00} ГГц" : "—",
        LcdKind.CpuPower => s.CpuPowerW is { } cpuW ? $"{cpuW:0} Вт" : "—",
        LcdKind.GpuPower => s.GpuPowerW is { } gpuW ? $"{gpuW:0} Вт" : "—",
        LcdKind.Vram => s.VramUsedGb is { } vram
            ? s.VramTotalGb is { } vramTotal and > 0 ? $"{vram:0.0}/{vramTotal:0} ГБ" : $"{vram:0.0} ГБ"
            : "—",
        LcdKind.NetDown => Speed(s.NetDownBps),
        LcdKind.NetUp => Speed(s.NetUpBps),
        LcdKind.Disk => s.DiskFreeGb > 0 ? $"{s.DiskUsedPct:0}% · своб. {s.DiskFreeGb:0} ГБ" : "—",
        LcdKind.Uptime => s.Uptime.TotalDays >= 1
            ? $"{(int)s.Uptime.TotalDays} д {s.Uptime.Hours} ч"
            : $"{s.Uptime.Hours} ч {s.Uptime.Minutes:00} мин",
        _ => item.Text ?? ""
    };

    private static readonly CultureInfo Ru = CultureInfo.GetCultureInfo("ru-RU");

    private static string Speed(double bytesPerSecond)
    {
        var bits = bytesPerSecond * 8;
        return bits switch
        {
            >= 1e9 => $"{bits / 1e9:0.0} Гбит/с",
            >= 1e6 => $"{bits / 1e6:0.0} Мбит/с",
            >= 1e3 => $"{bits / 1e3:0} Кбит/с",
            _ => "0 Кбит/с"
        };
    }

    private static SolidColorBrush ItemBrush(LcdItem item, Snapshot s)
    {
        if (string.IsNullOrWhiteSpace(item.Color))
        {
            return item.Kind switch
            {
                LcdKind.CpuTemp => Frozen(Heat(s.CpuTemp)),
                LcdKind.GpuTemp => Frozen(Heat(s.GpuTemp)),
                _ => Brushes.White
            };
        }

        return TryParseColor(item.Color, out var color) ? Frozen(color) : Brushes.White;
    }

    public static bool TryParseColor(string text, out Color color)
    {
        color = Colors.White;
        if (string.IsNullOrWhiteSpace(text)) return false;

        try
        {
            if (ColorConverter.ConvertFromString(text.Trim()) is Color parsed)
            {
                color = parsed;
                return true;
            }
        }
        catch
        {
            // не цвет — отвечаем false
        }

        return false;
    }

    private static Color Heat(double? temp) => temp switch
    {
        null => Color.FromRgb(0x6B, 0x70, 0x80),
        < 60 => Color.FromRgb(0x5F, 0xD3, 0x8D),
        < 80 => Color.FromRgb(0xFF, 0xC1, 0x4D),
        _ => Color.FromRgb(0xFF, 0x5C, 0x5C)
    };

    private static SolidColorBrush Frozen(Color color)
    {
        var brush = new SolidColorBrush(color);
        brush.Freeze();
        return brush;
    }
}
