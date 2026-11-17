using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Shapes;

namespace RigControl;

/// <summary>
/// Цветовой круг: оттенок — по кругу, насыщенность — от центра (белый) к краю,
/// под кругом — ползунок яркости. Колечко показывает выбранный цвет.
/// </summary>
public sealed class ColorWheel : StackPanel
{
    private static readonly Dictionary<int, BitmapSource> Cache = new();

    private readonly double _diameter;
    private readonly Ellipse _handle;
    private readonly Slider _value;
    private readonly Grid _wheel;
    private double _h, _s, _v = 1;
    private bool _dragging;
    private bool _quiet;

    public event Action<Color>? ColorChanged;

    public ColorWheel(double diameter = 160)
    {
        _diameter = diameter;

        _wheel = new Grid { Width = diameter, Height = diameter, Cursor = Cursors.Cross, Background = Brushes.Transparent };
        var image = new Image { Source = WheelBitmap((int)Math.Round(diameter * 2)), Width = diameter, Height = diameter };
        RenderOptions.SetBitmapScalingMode(image, BitmapScalingMode.HighQuality);
        _wheel.Children.Add(image);

        _handle = new Ellipse
        {
            Width = 14,
            Height = 14,
            Stroke = Brushes.White,
            StrokeThickness = 2.2,
            IsHitTestVisible = false,
            Effect = new System.Windows.Media.Effects.DropShadowEffect { BlurRadius = 3, ShadowDepth = 0, Opacity = 0.8 }
        };
        var overlay = new Canvas { IsHitTestVisible = false };
        overlay.Children.Add(_handle);
        _wheel.Children.Add(overlay);

        _wheel.MouseLeftButtonDown += (_, e) =>
        {
            _dragging = true;
            _wheel.CaptureMouse();
            PickAt(e.GetPosition(_wheel));
            e.Handled = true;
        };
        _wheel.MouseMove += (_, e) =>
        {
            if (_dragging) PickAt(e.GetPosition(_wheel));
        };
        _wheel.MouseLeftButtonUp += (_, _) =>
        {
            _dragging = false;
            _wheel.ReleaseMouseCapture();
        };
        _wheel.LostMouseCapture += (_, _) => _dragging = false;

        _value = new Slider { Minimum = 0, Maximum = 100, Value = 100, Width = diameter, Margin = new Thickness(0, 10, 0, 0) };
        _value.ValueChanged += (_, _) =>
        {
            _v = _value.Value / 100.0;
            if (!_quiet) Raise();
        };

        Children.Add(_wheel);
        Children.Add(_value);
        UpdateHandle();
    }

    public Color Color => FromHsv(_h, _s, _v);

    /// <summary>Поставить цвет снаружи (без события).</summary>
    public void SetColor(Color color)
    {
        var (h, s, v) = ToHsv(color);
        // У серого и чёрного оттенка нет — оставляем прошлый, чтобы колечко не прыгало.
        if (s > 0.001 && v > 0.001) _h = h;
        _s = v > 0.001 ? s : _s;
        _v = v;

        _quiet = true;
        _value.Value = Math.Round(v * 100);
        _quiet = false;
        UpdateHandle();
    }

    private void PickAt(Point point)
    {
        var r = _diameter / 2;
        var dx = point.X - r;
        var dy = point.Y - r;
        var dist = Math.Sqrt(dx * dx + dy * dy);

        _h = (Math.Atan2(-dy, dx) * 180 / Math.PI + 360) % 360;
        _s = Math.Clamp(dist / r, 0, 1);
        if (_v < 0.05)
        {
            // Выбор по кругу при «чёрной» яркости ничего бы не показал — поднимаем яркость.
            _v = 1;
            _quiet = true;
            _value.Value = 100;
            _quiet = false;
        }

        UpdateHandle();
        Raise();
    }

    private void UpdateHandle()
    {
        var r = _diameter / 2;
        var angle = _h * Math.PI / 180;
        var x = r + Math.Cos(angle) * _s * (r - 1);
        var y = r - Math.Sin(angle) * _s * (r - 1);
        Canvas.SetLeft(_handle, x - _handle.Width / 2);
        Canvas.SetTop(_handle, y - _handle.Height / 2);
        _handle.Stroke = _v > 0.55 && _s < 0.35 ? Brushes.DimGray : Brushes.White;
    }

    private void Raise() => ColorChanged?.Invoke(Color);

    private static BitmapSource WheelBitmap(int size)
    {
        if (Cache.TryGetValue(size, out var cached)) return cached;

        var pixels = new byte[size * size * 4];
        var r = size / 2.0;
        for (var y = 0; y < size; y++)
        {
            for (var x = 0; x < size; x++)
            {
                var dx = x + 0.5 - r;
                var dy = y + 0.5 - r;
                var dist = Math.Sqrt(dx * dx + dy * dy);
                var alpha = Math.Clamp(r - dist, 0, 1);       // сглаженный край
                if (alpha <= 0) continue;

                var hue = (Math.Atan2(-dy, dx) * 180 / Math.PI + 360) % 360;
                var c = FromHsv(hue, Math.Min(1, dist / r), 1);
                var i = (y * size + x) * 4;
                pixels[i] = (byte)(c.B * alpha);
                pixels[i + 1] = (byte)(c.G * alpha);
                pixels[i + 2] = (byte)(c.R * alpha);
                pixels[i + 3] = (byte)(255 * alpha);
            }
        }

        var bitmap = BitmapSource.Create(size, size, 96, 96, PixelFormats.Pbgra32, null, pixels, size * 4);
        bitmap.Freeze();
        Cache[size] = bitmap;
        return bitmap;
    }

    public static Color FromHsv(double h, double s, double v)
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
        return Color.FromRgb((byte)Math.Round((r + m) * 255), (byte)Math.Round((g + m) * 255), (byte)Math.Round((b + m) * 255));
    }

    public static (double H, double S, double V) ToHsv(Color color)
    {
        double r = color.R / 255.0, g = color.G / 255.0, b = color.B / 255.0;
        var max = Math.Max(r, Math.Max(g, b));
        var min = Math.Min(r, Math.Min(g, b));
        var d = max - min;
        double h = 0;
        if (d > 0)
        {
            if (max == r) h = 60 * ((g - b) / d % 6);
            else if (max == g) h = 60 * ((b - r) / d + 2);
            else h = 60 * ((r - g) / d + 4);
        }
        return ((h + 360) % 360, max <= 0 ? 0 : d / max, max);
    }

    public static string Hex(Color c) => $"#{c.R:X2}{c.G:X2}{c.B:X2}";

    /// <summary>
    /// Всплывающий круг рядом с элементом: цвет меняется сразу, пока тянете колечко.
    /// </summary>
    public static Popup ShowPopup(FrameworkElement target, Color initial, Action<Color> changed)
    {
        var wheel = new ColorWheel(170);
        wheel.SetColor(initial);
        wheel.ColorChanged += changed;

        var border = new Border
        {
            Padding = new Thickness(14),
            Margin = new Thickness(6, 6, 14, 14),
            CornerRadius = new CornerRadius(8),
            BorderThickness = new Thickness(1),
            Child = wheel
        };
        border.SetResourceReference(Border.BackgroundProperty, "CardBrush");
        border.SetResourceReference(Border.BorderBrushProperty, "LineBrush");
        border.SetResourceReference(EffectProperty, "SoftShadow");

        var popup = new Popup
        {
            PlacementTarget = target,
            Placement = PlacementMode.Bottom,
            StaysOpen = false,
            AllowsTransparency = true,
            PopupAnimation = PopupAnimation.Fade,
            Child = border
        };
        popup.IsOpen = true;
        return popup;
    }
}
