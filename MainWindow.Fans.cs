using System.Windows;
using System.Windows.Controls;

namespace RigControl;

/// <summary>Страница «Вентиляторы»: скорость вентиляторов на разъёмах платы (через LibreHardwareMonitor).</summary>
public partial class MainWindow
{
    private static readonly string[] FanModeNames =
        { "Авто (BIOS)", "Тихий", "Обычный", "Производительный", "Максимум", "Своя скорость", "Своя кривая" };

    private const int FanModeCurve = 6;

    /// <summary>С чего начинается «Своя кривая» — как «Обычный», но на пяти точках.</summary>
    private static readonly (double T, double P)[] DefaultFanCurve =
        { (35, 30), (50, 40), (65, 55), (75, 75), (85, 100) };

    /// <summary>Кривые «температура CPU → %» для режимов 1–3.</summary>
    private static readonly (double T, double P)[][] FanCurves =
    {
        Array.Empty<(double, double)>(),
        new[] { (40.0, 25.0), (60.0, 35.0), (75.0, 60.0), (85.0, 100.0) },
        new[] { (40.0, 35.0), (60.0, 50.0), (75.0, 75.0), (85.0, 100.0) },
        new[] { (40.0, 50.0), (60.0, 70.0), (75.0, 90.0), (80.0, 100.0) }
    };

    private const int FanMinPercent = 20;

    private sealed class FanRow
    {
        public required TextBlock Info;
        public required ComboBox Mode;
        public required Slider Percent;
        public required TextBlock PercentLabel;
        public required FrameworkElement PercentPanel;
        public required FrameworkElement CurvePanel;
        public required FanCurveEditor Editor;
    }

    private readonly Dictionary<string, FanRow> _fanRows = new();
    private readonly Dictionary<string, float> _fanLast = new();
    private bool _fanControlActive;
    private bool _fanRowsLoading;

    /// <summary>Датчики платы нужны: открыта страница или хотя бы один вентилятор не на «Авто».</summary>
    private bool FansWanted =>
        (NavFans?.IsChecked == true && WindowShown)
        || _fanControlActive
        || _settings.FanModeAll != 0
        || (_settings.FanModes?.Values.Any(m => m != 0) ?? false);

    /// <summary>Режим канала: свой, а если его не задавали — общий из профиля (0 — авто/BIOS).</summary>
    private int FanMode(string id) =>
        _settings.FanModes is { } modes && modes.TryGetValue(id, out var mode) ? mode : _settings.FanModeAll;

    private int FanPercent(string id) => _settings.FanPercent?.GetValueOrDefault(id, 60) ?? 60;

    private (double T, double P)[] FanCurveOf(string id)
    {
        if (_settings.FanCurves?.GetValueOrDefault(id) is { Length: >= 4 } flat && flat.Length % 2 == 0)
        {
            var points = new (double T, double P)[flat.Length / 2];
            for (var i = 0; i < points.Length; i++)
                points[i] = (Math.Clamp(flat[i * 2], 0, 110), Math.Clamp(flat[i * 2 + 1], FanMinPercent, 100));
            return points.OrderBy(p => p.T).ToArray();
        }

        return DefaultFanCurve.ToArray();
    }

    private void SaveFanCurve(string id, (double T, double P)[] points)
    {
        _settings.FanCurves ??= new Dictionary<string, int[]>();
        _settings.FanCurves[id] = points.SelectMany(p => new[] { (int)Math.Round(p.T), (int)Math.Round(p.P) }).ToArray();
    }

    private async Task UpdateFansAsync(Telemetry telemetry)
    {
        var fans = _snapshot.Fans;
        if (fans is null) return;

        // Цели: null — отдать плате.
        var temp = _snapshot.CpuTemp;
        var targets = new Dictionary<string, float?>();
        foreach (var fan in fans)
        {
            var target = FanTarget(FanMode(fan.Id), FanPercent(fan.Id), FanCurveOf(fan.Id), temp);
            if (target is { } t && _fanLast.TryGetValue(fan.Id, out var last) && t < last)
                target = Math.Max(t, last - 3);   // вниз — плавно, вверх — сразу

            if (target is { } v) _fanLast[fan.Id] = v;
            else _fanLast.Remove(fan.Id);
            targets[fan.Id] = target;
        }

        _fanCount = targets.Count;
        await Task.Run(() => telemetry.ApplyFans(targets));
        _fanControlActive = targets.Values.Any(v => v is not null);

        if (PageFans.IsVisible) DrawFans(fans, _snapshot.Board, telemetry.SensorsAvailable);
    }

    private static float? FanTarget(int mode, int percent, (double T, double P)[] curve, double? temp)
    {
        if (mode <= 0) return null;
        if (temp >= 90) return 100;   // защита от перегрева

        double value = mode switch
        {
            4 => 100,
            5 => percent,
            FanModeCurve => temp is { } c ? FanCurveEditor.Evaluate(curve, c) : 80,
            _ => temp is { } t ? Curve(FanCurves[Math.Clamp(mode, 1, 3)], t) : 80
        };

        return (float)Math.Clamp(value, FanMinPercent, 100);
    }

    private static double Curve((double T, double P)[] points, double temp)
    {
        if (temp <= points[0].T) return points[0].P;
        for (var i = 1; i < points.Length; i++)
        {
            if (temp <= points[i].T)
            {
                var k = (temp - points[i - 1].T) / (points[i].T - points[i - 1].T);
                return points[i - 1].P + (points[i].P - points[i - 1].P) * k;
            }
        }

        return points[^1].P;
    }

    private void DrawFans(IReadOnlyList<FanInfo> fans, string? board, bool sensorsAvailable)
    {
        if (string.IsNullOrWhiteSpace(board) || board.Trim().Equals("Unknown", StringComparison.OrdinalIgnoreCase)) board = null;
        if (fans.Count == 0)
        {
            FansBoardText.Text = !sensorsAvailable
                ? "Датчики недоступны. Нужен запуск от администратора и драйвер PawnIO (pawnio.eu) — его же использует LibreHardwareMonitor."
                : $"{board ?? "Плата"}: каналов управления не найдено. Возможно, не установлен драйвер PawnIO (pawnio.eu) или чип платы не поддерживается.";
            return;
        }

        FansBoardText.Text = (board ?? "Материнская плата") + $" · CPU {(_snapshot.CpuTemp is { } t ? $"{t:0} °C" : "—")}";

        // Набор каналов поменялся — строим строки заново.
        if (!fans.Select(f => f.Id).ToHashSet().SetEquals(_fanRows.Keys))
        {
            _fanRows.Clear();
            FansList.Children.Clear();
            foreach (var fan in fans) FansList.Children.Add(BuildFanRow(fan));
        }

        foreach (var fan in fans)
        {
            if (!_fanRows.TryGetValue(fan.Id, out var row)) continue;

            var rpm = fan.Rpm is { } r && r > 0 ? $"{r:0} об/мин" : "0 об/мин (не подключён или стоит)";
            var pct = fan.Percent is { } p ? $" · {p:0} %" : "";
            row.Info.Text = rpm + pct + (fan.Controlled ? " · управляет программа" : "");
            row.Editor.Temperature = _snapshot.CpuTemp;
        }
    }

    private Border BuildFanRow(FanInfo fan)
    {
        var name = new TextBlock { Text = fan.Name, FontWeight = FontWeights.SemiBold };
        var info = new TextBlock { Margin = new Thickness(0, 2, 0, 0) };
        info.SetResourceReference(StyleProperty, "Cap");

        var mode = new ComboBox { Width = 210, VerticalAlignment = VerticalAlignment.Center };
        foreach (var m in FanModeNames) mode.Items.Add(m);

        var label = new TextBlock { Margin = new Thickness(0, 12, 0, 2) };
        var slider = new Slider
        {
            Minimum = FanMinPercent,
            Maximum = 100,
            TickFrequency = 5,
            IsSnapToTickEnabled = true,
            SmallChange = 5,
            LargeChange = 10
        };
        var percentPanel = new StackPanel();
        percentPanel.Children.Add(label);
        percentPanel.Children.Add(slider);

        var header = new Grid();
        header.ColumnDefinitions.Add(new ColumnDefinition());
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        var text = new StackPanel();
        text.Children.Add(name);
        text.Children.Add(info);
        header.Children.Add(text);
        Grid.SetColumn(mode, 1);
        header.Children.Add(mode);

        // Своя кривая: точки тянутся мышью.
        var editor = new FanCurveEditor { Floor = FanMinPercent, Margin = new Thickness(0, 14, 0, 0) };
        var curveHint = new TextBlock
        {
            Text = "Перетаскивайте точки: по горизонтали — температура CPU, по вертикали — скорость. Между точками скорость меняется плавно.",
            TextWrapping = TextWrapping.Wrap,
            Margin = new Thickness(0, 6, 0, 0)
        };
        curveHint.SetResourceReference(StyleProperty, "Cap");

        var reset = new Button { Content = "Сбросить кривую", Margin = new Thickness(0, 0, 8, 0) };
        var copy = new Button
        {
            Content = "Скопировать в другие",
            ToolTip = "Такая же кривая будет у остальных вентиляторов (включится, когда выберете у них «Своя кривая»)."
        };
        var buttons = new StackPanel { Orientation = Orientation.Horizontal, Margin = new Thickness(0, 10, 0, 0) };
        buttons.Children.Add(reset);
        buttons.Children.Add(copy);

        var curvePanel = new StackPanel();
        curvePanel.Children.Add(editor);
        curvePanel.Children.Add(curveHint);
        curvePanel.Children.Add(buttons);

        var content = new StackPanel();
        content.Children.Add(header);
        content.Children.Add(percentPanel);
        content.Children.Add(curvePanel);

        var card = new Border { Margin = new Thickness(0, 0, 0, 10), Child = content };
        card.SetResourceReference(StyleProperty, "Card");

        var row = new FanRow
        {
            Info = info, Mode = mode, Percent = slider, PercentLabel = label, PercentPanel = percentPanel,
            CurvePanel = curvePanel, Editor = editor
        };
        _fanRows[fan.Id] = row;

        _fanRowsLoading = true;
        mode.SelectedIndex = Math.Clamp(FanMode(fan.Id), 0, FanModeNames.Length - 1);
        slider.Value = Math.Clamp(FanPercent(fan.Id), FanMinPercent, 100);
        editor.Points = FanCurveOf(fan.Id);
        editor.Temperature = _snapshot.CpuTemp;
        _fanRowsLoading = false;
        UpdateFanRow(row);

        mode.SelectionChanged += (_, _) => OnFanChanged(fan.Id, row);
        slider.ValueChanged += (_, _) => OnFanChanged(fan.Id, row);
        editor.Changed += () =>
        {
            SaveFanCurve(fan.Id, editor.Points);
            OnFanChanged(fan.Id, row);
        };
        reset.Click += (_, _) =>
        {
            editor.Points = DefaultFanCurve.ToArray();
            SaveFanCurve(fan.Id, editor.Points);
            OnFanChanged(fan.Id, row);
        };
        copy.Click += (_, _) =>
        {
            foreach (var (id, other) in _fanRows)
            {
                if (id == fan.Id) continue;
                other.Editor.Points = editor.Points;
                SaveFanCurve(id, editor.Points);
                _fanLast.Remove(id);
            }

            _argbSaveTimer?.Stop();
            _argbSaveTimer?.Start();
        };
        return card;
    }

    private static void UpdateFanRow(FanRow row)
    {
        row.PercentPanel.Visibility = row.Mode.SelectedIndex == 5 ? Visibility.Visible : Visibility.Collapsed;
        row.CurvePanel.Visibility = row.Mode.SelectedIndex == FanModeCurve ? Visibility.Visible : Visibility.Collapsed;
        row.PercentLabel.Text = $"Скорость: {(int)Math.Round(row.Percent.Value)} %";
    }

    private void OnFanChanged(string id, FanRow row)
    {
        UpdateFanRow(row);
        if (_fanRowsLoading) return;

        _settings.FanModes ??= new Dictionary<string, int>();
        _settings.FanPercent ??= new Dictionary<string, int>();
        _settings.FanModes[id] = Math.Max(0, row.Mode.SelectedIndex);
        _settings.FanPercent[id] = (int)Math.Round(row.Percent.Value);

        // Своя скорость сразу, без плавного спуска.
        _fanLast.Remove(id);

        _argbSaveTimer?.Stop();
        _argbSaveTimer?.Start();
    }
}
