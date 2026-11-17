using System.Globalization;
using System.Windows;
using System.Windows.Input;
using System.Windows.Media;

namespace RigControl;

/// <summary>
/// Редактор кривой «температура CPU → скорость вентилятора»: точки тянутся мышью, между ними — прямые.
/// Скорость вдоль кривой только растёт: точка, поднятая выше соседней, тянет её за собой.
/// </summary>
public sealed class FanCurveEditor : FrameworkElement
{
    private static readonly double[] DashPattern = { 3.0, 3.0 };
    public const double MinT = 20, MaxT = 100, MinP = 0, MaxP = 100;
    private const double MinGapT = 3;

    private (double T, double P)[] _points = Array.Empty<(double, double)>();
    private int _drag = -1;
    private int _hover = -1;
    private double? _temp;
    private double _floor = 20;

    /// <summary>Отпустили точку — кривая поменялась.</summary>
    public event Action? Changed;

    public FanCurveEditor()
    {
        Height = 210;
        Focusable = false;
        Cursor = Cursors.Arrow;
        IsVisibleChanged += (_, _) => InvalidateVisual();
    }

    public (double T, double P)[] Points
    {
        get => _points.ToArray();
        set
        {
            _points = value.OrderBy(p => p.T).ToArray();
            InvalidateVisual();
        }
    }

    /// <summary>Текущая температура CPU — вертикальная линия и точка на кривой.</summary>
    public double? Temperature
    {
        get => _temp;
        set
        {
            if (_temp == value) return;
            _temp = value;
            InvalidateVisual();
        }
    }

    /// <summary>Ниже этой скорости вентилятор не опустится (рисуется штриховкой).</summary>
    public double Floor
    {
        get => _floor;
        set { _floor = value; InvalidateVisual(); }
    }

    // Поля под подписи осей.
    private const double PadL = 38, PadR = 12, PadT = 12, PadB = 24;

    private Rect Plot => new(PadL, PadT, Math.Max(10, ActualWidth - PadL - PadR), Math.Max(10, ActualHeight - PadT - PadB));

    private Point ToScreen(double t, double p)
    {
        var r = Plot;
        return new Point(r.Left + (t - MinT) / (MaxT - MinT) * r.Width, r.Bottom - (p - MinP) / (MaxP - MinP) * r.Height);
    }

    private (double T, double P) FromScreen(Point s)
    {
        var r = Plot;
        var t = MinT + (s.X - r.Left) / r.Width * (MaxT - MinT);
        var p = MinP + (r.Bottom - s.Y) / r.Height * (MaxP - MinP);
        return (t, p);
    }

    /// <summary>Скорость по кривой при температуре <paramref name="temp"/> (как считает программа).</summary>
    public static double Evaluate((double T, double P)[] points, double temp)
    {
        if (points.Length == 0) return 100;
        if (temp <= points[0].T) return points[0].P;
        for (var i = 1; i < points.Length; i++)
        {
            if (temp <= points[i].T)
            {
                var k = (temp - points[i - 1].T) / Math.Max(0.001, points[i].T - points[i - 1].T);
                return points[i - 1].P + (points[i].P - points[i - 1].P) * k;
            }
        }

        return points[^1].P;
    }

    private Brush Res(string key, Brush fallback) => TryFindResource(key) as Brush ?? fallback;

    protected override void OnRender(DrawingContext dc)
    {
        var plot = Plot;
        var line = Res("LineBrush", Brushes.Gray);
        var muted = Res("MutedBrush", Brushes.Gray);
        var text = Res("TextBrush", Brushes.White);
        var accent = Res("AccentBrush", Brushes.Orange);
        var mint = Res("MintBrush", Brushes.MediumAquamarine);
        var dpi = VisualTreeHelper.GetDpi(this).PixelsPerDip;
        var face = new Typeface("Segoe UI");

        FormattedText Text(string s, Brush b, double size = 11) =>
            new(s, CultureInfo.CurrentUICulture, FlowDirection.LeftToRight, face, size, b, dpi);

        // Прозрачный фон — чтобы ловить мышь по всей площади.
        dc.DrawRectangle(Brushes.Transparent, null, new Rect(0, 0, ActualWidth, ActualHeight));

        var grid = new Pen(line, 1);
        grid.Freeze();

        for (var t = MinT; t <= MaxT + 0.1; t += 10)
        {
            var x = ToScreen(t, 0).X;
            dc.DrawLine(grid, new Point(x, plot.Top), new Point(x, plot.Bottom));
            var label = Text($"{t:0}°", muted);
            dc.DrawText(label, new Point(x - label.Width / 2, plot.Bottom + 5));
        }

        for (var p = MinP; p <= MaxP + 0.1; p += 20)
        {
            var y = ToScreen(MinT, p).Y;
            dc.DrawLine(grid, new Point(plot.Left, y), new Point(plot.Right, y));
            var label = Text($"{p:0}%", muted);
            dc.DrawText(label, new Point(plot.Left - label.Width - 6, y - label.Height / 2));
        }

        // Ниже минимума скорость не опускается — затемняем эту полосу.
        if (_floor > MinP)
        {
            var floorY = ToScreen(MinT, _floor).Y;
            var shade = muted.Clone();
            shade.Opacity = 0.12;
            shade.Freeze();
            dc.DrawRectangle(shade, null, new Rect(new Point(plot.Left, floorY), new Point(plot.Right, plot.Bottom)));
        }

        if (_points.Length == 0) return;

        // Кривая: от левого края до первой точки — ровно, потом по точкам, после последней — ровно до края.
        var poly = new List<Point> { ToScreen(MinT, Math.Max(_floor, _points[0].P)) };
        poly.AddRange(_points.Select(p => ToScreen(p.T, Math.Max(_floor, p.P))));
        poly.Add(ToScreen(MaxT, Math.Max(_floor, _points[^1].P)));

        var area = new StreamGeometry();
        using (var g = area.Open())
        {
            g.BeginFigure(new Point(plot.Left, plot.Bottom), true, true);
            g.PolyLineTo(poly, true, true);
            g.LineTo(new Point(plot.Right, plot.Bottom), true, true);
        }
        area.Freeze();

        var fill = accent.Clone();
        fill.Opacity = 0.14;
        fill.Freeze();
        dc.DrawGeometry(fill, null, area);

        var curve = new StreamGeometry();
        using (var g = curve.Open())
        {
            g.BeginFigure(poly[0], false, false);
            g.PolyLineTo(poly.Skip(1).ToList(), true, true);
        }
        curve.Freeze();
        dc.DrawGeometry(null, new Pen(accent, 2.2) { LineJoin = PenLineJoin.Round }, curve);

        // Текущая температура.
        if (_temp is { } temp)
        {
            var t = Math.Clamp(temp, MinT, MaxT);
            var p = Math.Max(_floor, temp >= 90 ? 100 : Evaluate(_points, temp));
            var at = ToScreen(t, p);
            var dash = new Pen(mint, 1.2) { DashStyle = new DashStyle(DashPattern, 0) };
            dc.DrawLine(dash, new Point(at.X, plot.Top), new Point(at.X, plot.Bottom));
            dc.DrawEllipse(mint, null, at, 4.5, 4.5);

            var label = Text($"{temp:0} °C → {p:0} %", mint, 11.5);
            var lx = at.X + 8 + label.Width > plot.Right ? at.X - 8 - label.Width : at.X + 8;
            dc.DrawText(label, new Point(lx, plot.Top + 2));
        }

        // Точки.
        var bg = Res("CardBrush", Brushes.Black);
        for (var i = 0; i < _points.Length; i++)
        {
            var at = ToScreen(_points[i].T, _points[i].P);
            var active = i == _drag || i == _hover;
            dc.DrawEllipse(bg, new Pen(accent, 2.2), at, active ? 7 : 5.5, active ? 7 : 5.5);
            if (active) dc.DrawEllipse(accent, null, at, 3, 3);

            if (i == _drag || (i == _hover && _drag < 0))
            {
                var label = Text($"{_points[i].T:0} °C · {_points[i].P:0} %", text, 11.5);
                var lx = Math.Clamp(at.X - label.Width / 2, plot.Left, plot.Right - label.Width);
                var ly = at.Y - 12 - label.Height < plot.Top ? at.Y + 12 : at.Y - 12 - label.Height;
                dc.DrawRectangle(bg, null, new Rect(lx - 4, ly - 1, label.Width + 8, label.Height + 2));
                dc.DrawText(label, new Point(lx, ly));
            }
        }
    }

    private int Hit(Point s)
    {
        var best = -1;
        var bestD = 14.0;
        for (var i = 0; i < _points.Length; i++)
        {
            var at = ToScreen(_points[i].T, _points[i].P);
            var d = (at - s).Length;
            if (d < bestD)
            {
                bestD = d;
                best = i;
            }
        }

        return best;
    }

    protected override void OnMouseLeftButtonDown(MouseButtonEventArgs e)
    {
        var hit = Hit(e.GetPosition(this));
        if (hit < 0) return;

        _drag = hit;
        CaptureMouse();
        e.Handled = true;
        InvalidateVisual();
    }

    protected override void OnMouseMove(MouseEventArgs e)
    {
        var pos = e.GetPosition(this);

        if (_drag < 0)
        {
            var hit = Hit(pos);
            if (hit != _hover)
            {
                _hover = hit;
                Cursor = hit >= 0 ? Cursors.Hand : Cursors.Arrow;
                InvalidateVisual();
            }
            return;
        }

        var (t, p) = FromScreen(pos);
        var i = _drag;

        var lo = i == 0 ? MinT : _points[i - 1].T + MinGapT;
        var hi = i == _points.Length - 1 ? MaxT : _points[i + 1].T - MinGapT;
        t = Math.Round(Math.Clamp(t, lo, Math.Max(lo, hi)));
        p = Math.Round(Math.Clamp(p, _floor, MaxP));

        _points[i] = (t, p);

        // Скорость с температурой не падает: соседей подтягиваем.
        for (var k = i + 1; k < _points.Length; k++)
            if (_points[k].P < p) _points[k] = (_points[k].T, p);
        for (var k = i - 1; k >= 0; k--)
            if (_points[k].P > p) _points[k] = (_points[k].T, p);

        InvalidateVisual();
    }

    protected override void OnMouseLeftButtonUp(MouseButtonEventArgs e)
    {
        if (_drag < 0) return;
        _drag = -1;
        ReleaseMouseCapture();
        InvalidateVisual();
        Changed?.Invoke();
    }

    protected override void OnLostMouseCapture(MouseEventArgs e)
    {
        if (_drag < 0) return;
        _drag = -1;
        InvalidateVisual();
        Changed?.Invoke();
    }

    protected override void OnMouseLeave(MouseEventArgs e)
    {
        if (_hover < 0) return;
        _hover = -1;
        Cursor = Cursors.Arrow;
        InvalidateVisual();
    }
}
