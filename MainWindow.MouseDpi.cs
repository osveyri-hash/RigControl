using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Shapes;
using RigControl.Mouse;

namespace RigControl;

/// <summary>
/// Мышь → «DPI»: число уровней, карточки уровней с цветом индикатора (как на мыши), ползунок со шкалой
/// 200…4200 на 70 % длины и дальше до максимума (как в M HUB), отдельная чувствительность по Y.
/// </summary>
public partial class MainWindow
{
    private sealed class DpiCard
    {
        public required Border Root;
        public required Border Badge;
        public required Slider X, Y;
        public required TextBox XBox, YBox;
        public required TextBlock XLabel, YLabel, MaxTick;
        public required Canvas Ticks;
    }

    /// <summary>Цвета индикатора DPI по инструкции: красный, синий, зелёный, жёлтый, голубой, фиолетовый.</summary>
    private static readonly Color[] DpiColors =
    {
        Color.FromRgb(0xE5, 0x48, 0x4D), Color.FromRgb(0x3E, 0x7B, 0xFA), Color.FromRgb(0x2F, 0xB3, 0x6B),
        Color.FromRgb(0xF2, 0xC2, 0x30), Color.FromRgb(0x22, 0xC3, 0xD6), Color.FromRgb(0x9B, 0x5C, 0xF6),
    };

    private static readonly int[] DpiTicks = { 200, 1200, 2200, 3200, 4200 };

    private readonly DpiCard[] _dpiCards = new DpiCard[6];
    private readonly int[] _dpiX = new int[6], _dpiY = new int[6];
    private int _dpiCount = 6, _dpiActive;
    private bool _dpiSeparate;

    private int MouseMaxDpi => _mouse?.MaxDpi(_mouseStatus) ?? 42000;

    private void InitMouseDpi()
    {
        for (var n = 1; n <= 6; n++)
        {
            var button = new RadioButton { Style = (Style)FindResource("SegButton"), GroupName = "MouseDpiCount", Tag = n,
                Content = n.ToString(), MinWidth = 40 };
            button.Checked += OnMouseDpiCount;
            MouseDpiCount.Children.Add(button);
        }

        for (var i = 0; i < 6; i++)
        {
            _dpiCards[i] = MakeDpiCard(i);
            MouseDpiCards.Children.Add(_dpiCards[i].Root);
        }
    }

    private DpiCard MakeDpiCard(int level)
    {
        var dot = new Ellipse { Width = 10, Height = 10, Fill = new SolidColorBrush(DpiColors[level]),
            Margin = new Thickness(0, 0, 8, 0), VerticalAlignment = VerticalAlignment.Center };
        var name = new TextBlock { Text = $"Уровень {level + 1}", FontWeight = FontWeights.SemiBold, VerticalAlignment = VerticalAlignment.Center };
        var badgeText = new TextBlock { Text = "сейчас", FontSize = 11, FontWeight = FontWeights.SemiBold };
        badgeText.SetResourceReference(TextBlock.ForegroundProperty, "AccentTextBrush");
        var badge = new Border { CornerRadius = new CornerRadius(8), Padding = new Thickness(8, 1, 8, 2), Margin = new Thickness(10, 0, 0, 0),
            VerticalAlignment = VerticalAlignment.Center, Child = badgeText, Visibility = Visibility.Collapsed };
        badge.SetResourceReference(Border.BackgroundProperty, "AccentBrush");
        var title = new StackPanel { Orientation = Orientation.Horizontal, Margin = new Thickness(0, 0, 0, 4), Children = { dot, name, badge } };

        var grid = new Grid();
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        for (var r = 0; r < 3; r++) grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });

        var (xLabel, x, xBox) = AddDpiRow(grid, 0, "X", level, axisY: false);
        var (yLabel, y, yBox) = AddDpiRow(grid, 1, "Y", level, axisY: true);

        var ticks = new Canvas { Height = 15, ClipToBounds = false, IsHitTestVisible = false };
        Grid.SetRow(ticks, 2);
        Grid.SetColumn(ticks, 1);
        grid.Children.Add(ticks);
        TextBlock? maxTick = null;
        foreach (var value in DpiTicks.Append(0))
        {
            var tick = new TextBlock { Text = value == 0 ? "42000" : value.ToString(), FontSize = 10.5, Tag = value };
            tick.SetResourceReference(TextBlock.ForegroundProperty, "MutedBrush");
            ticks.Children.Add(tick);
            if (value == 0) maxTick = tick;
        }
        ticks.SizeChanged += (_, _) => LayoutDpiTicks(ticks);

        var root = new Border
        {
            CornerRadius = new CornerRadius(12), BorderThickness = new Thickness(1.5), Padding = new Thickness(16, 12, 16, 8),
            Margin = new Thickness(0, 0, 14, 14), Cursor = Cursors.Hand,
            Child = new StackPanel { Children = { title, grid } },
            ToolTip = "Нажмите, чтобы сделать уровень текущим",
        };
        root.SetResourceReference(Border.BackgroundProperty, "CardBrush");
        root.SetResourceReference(Border.BorderBrushProperty, "LineBrush");
        // Клик по карточке делает уровень текущим; ползунки и поля ввода уровень не переключают (без скачков DPI).
        root.PreviewMouseLeftButtonDown += async (_, e) =>
        {
            if (!IsDpiEditor(e.OriginalSource as DependencyObject, root)) await SetActiveDpiAsync(level);
        };

        return new DpiCard { Root = root, Badge = badge, X = x, Y = y, XBox = xBox, YBox = yBox,
            XLabel = xLabel, YLabel = yLabel, MaxTick = maxTick!, Ticks = ticks };
    }

    private static bool IsDpiEditor(DependencyObject? node, DependencyObject root)
    {
        while (node is not null && !ReferenceEquals(node, root))
        {
            if (node is Slider or TextBox) return true;
            node = node is Visual or System.Windows.Media.Media3D.Visual3D ? VisualTreeHelper.GetParent(node) : LogicalTreeHelper.GetParent(node);
        }

        return false;
    }

    private (TextBlock, Slider, TextBox) AddDpiRow(Grid grid, int row, string axis, int level, bool axisY)
    {
        var label = new TextBlock { Text = axis, Width = 18, FontWeight = FontWeights.SemiBold, VerticalAlignment = VerticalAlignment.Center };
        label.SetResourceReference(TextBlock.ForegroundProperty, "MutedBrush");
        var slider = new Slider { Minimum = 0, Maximum = 1000, SmallChange = 5, LargeChange = 50, VerticalAlignment = VerticalAlignment.Center };
        slider.ValueChanged += (_, _) => OnDpiSlider(level, axisY);
        var box = new TextBox { Width = 76, TextAlignment = TextAlignment.Center, FontWeight = FontWeights.SemiBold,
            Padding = new Thickness(4, 6, 4, 6), Margin = new Thickness(10, 3, 0, 3), VerticalAlignment = VerticalAlignment.Center };
        box.KeyDown += (_, e) =>
        {
            if (e.Key != Key.Enter) return;
            e.Handled = true;
            CommitDpiBox(level, axisY);
        };
        box.LostKeyboardFocus += (_, _) => CommitDpiBox(level, axisY);

        Grid.SetRow(label, row);
        Grid.SetRow(slider, row);
        Grid.SetColumn(slider, 1);
        Grid.SetRow(box, row);
        Grid.SetColumn(box, 2);
        grid.Children.Add(label);
        grid.Children.Add(slider);
        grid.Children.Add(box);
        return (label, slider, box);
    }

    /// <summary>Подписи шкалы под центром бегунка (у ползунка по 11 px полей под бегунок).</summary>
    private void LayoutDpiTicks(Canvas ticks)
    {
        var width = ticks.ActualWidth;
        if (width <= 0) return;
        foreach (TextBlock tick in ticks.Children)
        {
            var value = (int)tick.Tag;
            var frac = value == 0 ? 1 : DpiToPos(value, MouseMaxDpi) / 1000;
            tick.Measure(new Size(double.PositiveInfinity, double.PositiveInfinity));
            var left = 11 + frac * (width - 22) - tick.DesiredSize.Width / 2;
            Canvas.SetLeft(tick, Math.Clamp(left, 0, Math.Max(0, width - tick.DesiredSize.Width)));
        }
    }

    private static double DpiToPos(int dpi, int max) => dpi <= 4200
        ? (Math.Max(dpi, 200) - 200) / 4000.0 * 700
        : 700 + (Math.Min(dpi, max) - 4200) / (double)Math.Max(1, max - 4200) * 300;

    private static int PosToDpi(double pos, int max)
    {
        var value = pos <= 700 ? 200 + pos / 700 * 4000 : 4200 + (pos - 700) / 300 * (max - 4200);
        var step = value < 4200 ? 50 : 100;
        return Math.Clamp((int)Math.Round(value / step) * step, 200, max);
    }

    private bool DpiEditing => _dpiCards.Any(c => c is not null && (c.XBox.IsKeyboardFocusWithin || c.YBox.IsKeyboardFocusWithin
                                                                   || c.X.IsMouseCaptureWithin || c.Y.IsMouseCaptureWithin));

    /// <summary>Из мыши — в карточки (если пользователь как раз не крутит ползунок и не вводит число).</summary>
    private void DrawMouseDpi(MouseConfig? cfg)
    {
        if (cfg is null || _dpiCards[0] is null) return;
        if (!MouseHoldUi && !DpiEditing)
        {
            Array.Copy(cfg.Dpi, _dpiX, 6);
            Array.Copy(cfg.DpiY, _dpiY, 6);
            _dpiCount = Math.Clamp(cfg.Count, 1, 6);
            _dpiActive = MouseCurrentLevel(cfg);
            _dpiSeparate |= cfg.SeparateY;
        }
        RenderDpi();
    }

    private void RenderDpi()
    {
        var max = MouseMaxDpi;
        _mouseUiUpdating = true;
        try
        {
            foreach (RadioButton button in MouseDpiCount.Children) button.IsChecked = (int)button.Tag == _dpiCount;
            MouseDpiXY.IsChecked = _dpiSeparate;
            MouseDpiResetButton.ToolTip = $"Как с завода: 400 · 800 · 1600 · 3200 · 6400 · {max}";

            for (var i = 0; i < 6; i++)
            {
                var card = _dpiCards[i];
                var active = i == _dpiActive;
                card.Root.Visibility = i < _dpiCount ? Visibility.Visible : Visibility.Collapsed;
                card.Root.SetResourceReference(Border.BorderBrushProperty, active ? "AccentBrush" : "LineBrush");
                card.Root.SetResourceReference(Border.BackgroundProperty, active ? "AccentDimBrush" : "CardBrush");
                card.Badge.Visibility = active ? Visibility.Visible : Visibility.Collapsed;
                card.XLabel.Visibility = card.YLabel.Visibility = _dpiSeparate ? Visibility.Visible : Visibility.Collapsed;
                card.Y.Visibility = card.YBox.Visibility = card.YLabel.Visibility;

                if (!card.X.IsMouseCaptureWithin) card.X.Value = DpiToPos(_dpiX[i], max);
                if (!card.Y.IsMouseCaptureWithin) card.Y.Value = DpiToPos(_dpiY[i], max);
                if (!card.XBox.IsKeyboardFocusWithin) card.XBox.Text = _dpiX[i].ToString();
                if (!card.YBox.IsKeyboardFocusWithin) card.YBox.Text = _dpiY[i].ToString();
                if (card.MaxTick.Text != max.ToString())
                {
                    card.MaxTick.Text = max.ToString();
                    LayoutDpiTicks(card.Ticks);
                }
            }
        }
        finally
        {
            _mouseUiUpdating = false;
        }
    }

    private void OnDpiSlider(int level, bool axisY)
    {
        if (_mouseUiUpdating || _mouseConfig is null) return;
        var card = _dpiCards[level];
        var value = PosToDpi((axisY ? card.Y : card.X).Value, MouseMaxDpi);
        SetDpiValue(level, axisY, value);
        (axisY ? card.YBox : card.XBox).Text = value.ToString();
        WriteDpiLater();
    }

    private void CommitDpiBox(int level, bool axisY)
    {
        if (_mouseConfig is null) return;
        var card = _dpiCards[level];
        var box = axisY ? card.YBox : card.XBox;
        var current = axisY ? _dpiY[level] : _dpiX[level];
        if (!int.TryParse(box.Text.Replace(" ", "").Trim(), out var value))
        {
            box.Text = current.ToString();
            return;
        }

        value = Math.Clamp((int)Math.Round(value / 50.0) * 50, 200, MouseMaxDpi);
        box.Text = value.ToString();
        if (value == current) return;
        SetDpiValue(level, axisY, value);
        _mouseUiUpdating = true;
        (axisY ? card.Y : card.X).Value = DpiToPos(value, MouseMaxDpi);
        _mouseUiUpdating = false;
        WriteDpiLater();
    }

    private void SetDpiValue(int level, bool axisY, int value)
    {
        if (axisY) _dpiY[level] = value;
        else _dpiX[level] = value;
        if (!_dpiSeparate) _dpiY[level] = _dpiX[level];
    }

    private (int[] X, int[] Y, int Count, int Active) DpiSnapshot() =>
        (_dpiX.ToArray(), _dpiSeparate ? _dpiY.ToArray() : _dpiX.ToArray(), _dpiCount, Math.Min(_dpiActive, _dpiCount - 1));

    private void WriteDpiLater()
    {
        if (_mouseConfig is not { } cfg) return;
        var (x, y, count, active) = DpiSnapshot();
        MouseSendLater(m => m.WriteDpi(cfg, x, y, count, active));
    }

    private async Task WriteDpiNowAsync(string done)
    {
        if (_mouseConfig is not { } cfg) return;
        var (x, y, count, active) = DpiSnapshot();
        RenderDpi();
        _mouseHoldUi = DateTime.UtcNow.AddSeconds(1);
        await MouseSendAsync(m => m.WriteDpi(cfg, x, y, count, active), done);
    }

    private async Task SetActiveDpiAsync(int level)
    {
        if (_mouseConfig is not { } cfg || level >= _dpiCount || (level == _dpiActive && level == MouseCurrentLevel(cfg))) return;
        _dpiActive = level;
        await WriteDpiNowAsync($"Уровень {level + 1}: {_dpiX[level]} DPI");
    }

    private async void OnMouseDpiCount(object sender, RoutedEventArgs e)
    {
        if (_mouseUiUpdating || sender is not RadioButton { Tag: int count } || _mouseConfig is null || count == _dpiCount) return;
        var defaults = _mouse?.DefaultDpi(_mouseStatus) ?? new[] { 400, 800, 1600, 3200, 6400, 42000 };
        for (var i = _dpiCount; i < count; i++)
        {
            if (_dpiX[i] is < 200 or > 50000) _dpiX[i] = defaults[i];
            if (_dpiY[i] is < 200 or > 50000) _dpiY[i] = _dpiX[i];
        }
        _dpiCount = count;
        _dpiActive = Math.Min(_dpiActive, count - 1);
        await WriteDpiNowAsync($"Уровней DPI: {count}");
    }

    private async void OnMouseDpiXY(object sender, RoutedEventArgs e)
    {
        if (_mouseConfig is null) return;
        _dpiSeparate = MouseDpiXY.IsChecked == true;
        if (_dpiSeparate)
        {
            RenderDpi();
            return;
        }
        Array.Copy(_dpiX, _dpiY, 6);
        await WriteDpiNowAsync("Одинаково по X и Y");
    }

    private async void OnMouseDpiReset(object sender, RoutedEventArgs e)
    {
        if (_mouseConfig is null || !ConfirmTwice(MouseDpiResetButton, "Точно?")) return;
        var defaults = _mouse?.DefaultDpi(_mouseStatus) ?? new[] { 400, 800, 1600, 3200, 6400, 42000 };
        Array.Copy(defaults, _dpiX, 6);
        Array.Copy(defaults, _dpiY, 6);
        _dpiCount = 6;
        _dpiActive = 2;
        _dpiSeparate = false;
        await WriteDpiNowAsync("DPI как с завода");
    }
}
