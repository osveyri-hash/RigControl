using System.Diagnostics;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using System.Windows.Shapes;
using System.Windows.Threading;
using Microsoft.Win32;

namespace RigControl;

/// <summary>Страница «Подсветка»: ARGB-разъём материнской платы ASUS (вентиляторы СЖО).</summary>
public partial class MainWindow
{
    private const string ArmouryService = "LightingService";

    private ArgbEngine? _argb;
    private DispatcherTimer? _argbPreviewTimer;
    private byte[]? _argbShownFrame;
    private DispatcherTimer? _argbSaveTimer;
    private ColorWheel? _argbWheel;
    private bool _argbColorFromWheel;
    private bool _argbLoading = true;
    private double _argbCpuTemp = double.NaN;
    private readonly List<SolidColorBrush> _argbDots = new();

    /// <summary>Эффект «по температуре» — таймер датчиков должен работать и без экрана помпы.</summary>
    private bool ArgbWantsCpuTemp => (ArgbMode)_settings.ArgbMode == ArgbMode.CpuTemp;

    private void InitArgb()
    {
        _argbLoading = true;
        LightMirror.SetRates(_settings.EffectRates);

        foreach (var hex in PaintPalette)
        {
            var swatch = new Border
            {
                Width = 26,
                Height = 26,
                CornerRadius = new CornerRadius(6),
                Margin = new Thickness(0, 0, 8, 8),
                Background = new SolidColorBrush((Color)ColorConverter.ConvertFromString(hex)),
                BorderThickness = new Thickness(2),
                Cursor = System.Windows.Input.Cursors.Hand,
                Tag = hex,
                ToolTip = hex
            };
            swatch.SetResourceReference(Border.BorderBrushProperty, "LineBrush");
            swatch.MouseLeftButtonUp += (_, _) => ArgbColorBox.Text = hex;
            ArgbSwatches.Children.Add(swatch);
        }

        _argbWheel = new ColorWheel(140);
        _argbWheel.ColorChanged += color =>
        {
            _argbColorFromWheel = true;
            ArgbColorBox.Text = ColorWheel.Hex(color);
            _argbColorFromWheel = false;
        };
        ArgbWheelHost.Content = _argbWheel;

        ArgbModeCombo.SelectedIndex = Math.Clamp(_settings.ArgbMode, 0, ArgbModeCombo.Items.Count - 1);
        ArgbColorBox.Text = _settings.ArgbColor;
        ArgbColor2Box.Text = _settings.ArgbColor2;
        ArgbColor3Box.Text = _settings.ArgbColor3;
        ArgbRainbowCheck.IsChecked = _settings.ArgbRainbow;
        ArgbBrightness.Value = Math.Clamp(_settings.ArgbBrightness, 0, 100);
        ArgbSpeed.Value = Math.Clamp(_settings.ArgbSpeed, 1, 5);
        ArgbLeds.Value = Math.Clamp(_settings.ArgbLeds, 1, ArgbEngine.MaxLeds);
        ArgbFans.Value = Math.Clamp(_settings.ArgbFans, 1, 6);
        ArgbMethodCombo.SelectedIndex = _settings.ArgbBoardEffect ? 1 : 0;
        FillArgbHeaders(Math.Max(1, _settings.ArgbTarget));

        _argbLoading = false;
        UpdateArgbUi();

        _argb = new ArgbEngine(ArgbConfigFromUi(), () => _argbCpuTemp);
        _argb.StatusChanged += (text, ok) => Dispatcher.BeginInvoke(() => OnArgbStatus(text, ok));

        _argbPreviewTimer = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(40) };
        _argbPreviewTimer.Tick += (_, _) => DrawArgbPreview();
        IsVisibleChanged += (_, _) => UpdateArgbPreviewTimer();
        StateChanged += (_, _) => UpdateArgbPreviewTimer();

        _argbSaveTimer = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(800) };
        _argbSaveTimer.Tick += (_, _) =>
        {
            _argbSaveTimer!.Stop();
            _settings.Save();
        };

        // После сна плата возвращает свою подсветку — подключаемся заново.
        SystemEvents.PowerModeChanged += OnArgbPowerMode;

        UpdateArmouryPanel();
    }

    private void ArgbShutdown()
    {
        SystemEvents.PowerModeChanged -= OnArgbPowerMode;
        _argbPreviewTimer?.Stop();
        _argb?.Dispose();
        _argb = null;
    }

    private void OnArgbPowerMode(object? sender, PowerModeChangedEventArgs e)
    {
        if (e.Mode == PowerModes.Resume) _argb?.Reconnect();
    }

    private ArgbConfig ArgbConfigFromUi()
    {
        var (r, g, b) = Ak820Protocol.ParseColor(ArgbColorBox.Text, (0, 180, 255));
        var (r2, g2, b2) = Ak820Protocol.ParseColor(ArgbColor2Box.Text, (r, g, b));
        var (r3, g3, b3) = Ak820Protocol.ParseColor(ArgbColor3Box.Text, (r2, g2, b2));
        return new ArgbConfig(
            (ArgbMode)Math.Max(0, ArgbModeCombo.SelectedIndex),
            Color.FromRgb(r, g, b),
            ArgbLedBrightness((int)Math.Round(ArgbBrightness.Value)),
            (int)Math.Round(ArgbSpeed.Value),
            (int)Math.Round(ArgbLeds.Value),
            Math.Max(0, ArgbHeaderCombo.SelectedIndex),
            ArgbMethodCombo.SelectedIndex == 1,
            (int)Math.Round(ArgbFans.Value),
            ArgbRainbowCheck.IsChecked != false,
            Color.FromRgb(r2, g2, b2),
            Color.FromRgb(r3, g3, b3));
    }

    /// <summary>Список: «Все разъёмы», ARGB 1 … ARGB count.</summary>
    /// <summary>
    /// Ползунок 0–100 → мощность светодиодов: 0 — выключено, 1 — 10 %, 100 — 100 %.
    /// Ниже 10 % слабые каналы пропадают и цвет уходит в красный.
    /// </summary>
    private static int ArgbLedBrightness(int slider) =>
        slider <= 0 ? 0 : (int)Math.Round(10 + (slider - 1) * 90 / 99.0);

    private void FillArgbHeaders(int count)
    {
        var selected = ArgbHeaderCombo.SelectedIndex >= 0 ? ArgbHeaderCombo.SelectedIndex : _settings.ArgbTarget;
        count = Math.Max(count, 1);
        if (ArgbHeaderCombo.Items.Count == count + 1) return;

        var wasLoading = _argbLoading;
        _argbLoading = true;
        ArgbHeaderCombo.Items.Clear();
        ArgbHeaderCombo.Items.Add("Все разъёмы");
        for (var i = 0; i < count; i++) ArgbHeaderCombo.Items.Add($"ARGB {i + 1}");
        ArgbHeaderCombo.SelectedIndex = Math.Clamp(selected, 0, count);
        _argbLoading = wasLoading;
    }

    private void OnArgbChanged(object sender, SelectionChangedEventArgs e) => ArgbSettingsChanged();

    private void OnArgbSlider(object sender, RoutedPropertyChangedEventArgs<double> e) => ArgbSettingsChanged();

    private void OnArgbColorText(object sender, TextChangedEventArgs e) => ArgbSettingsChanged();

    private void OnArgbRainbowChanged(object sender, RoutedEventArgs e) => ArgbSettingsChanged();

    private void OnArgbColor2Click(object sender, System.Windows.Input.MouseButtonEventArgs e)
    {
        PickArgbPaletteColor(ArgbColor2Box, ArgbColor2Swatch);
        e.Handled = true;
    }

    private void OnArgbColor3Click(object sender, System.Windows.Input.MouseButtonEventArgs e)
    {
        PickArgbPaletteColor(ArgbColor3Box, ArgbColor3Swatch);
        e.Handled = true;
    }

    private static void PickArgbPaletteColor(TextBox box, FrameworkElement anchor)
    {
        var (r, g, b) = Ak820Protocol.ParseColor(box.Text, (0, 180, 255));
        ColorWheel.ShowPopup(anchor, Color.FromRgb(r, g, b), color => box.Text = ColorWheel.Hex(color));
    }

    private void ArgbSettingsChanged()
    {
        if (_argbLoading || ArgbHeaderCombo is null || ArgbLeds is null || ArgbColorBox is null
            || ArgbColor2Box is null || ArgbColor3Box is null || ArgbMethodCombo is null
            || ArgbFans is null) return;

        UpdateArgbUi();

        var config = ArgbConfigFromUi();
        var modeChanged = _settings.ArgbMode != (int)config.Mode;
        _settings.ArgbMode = (int)config.Mode;
        _settings.ArgbColor = ColorWheel.Hex(config.Color);
        _settings.ArgbColor2 = ColorWheel.Hex(config.Color2);
        _settings.ArgbColor3 = ColorWheel.Hex(config.Color3);
        _settings.ArgbBrightness = (int)Math.Round(ArgbBrightness.Value);
        _settings.ArgbSpeed = config.Speed;
        _settings.ArgbLeds = config.Leds;
        _settings.ArgbFans = config.Fans;
        _settings.ArgbTarget = config.Target;
        _settings.ArgbBoardEffect = config.BoardEffect;
        _settings.ArgbRainbow = config.Rainbow;

        if (_argb is not null) _argb.Config = config;
        if (modeChanged) UpdateArmouryPanel();

        _argbSaveTimer?.Stop();
        _argbSaveTimer?.Start();
    }

    private void UpdateArgbUi()
    {
        var mode = (ArgbMode)Math.Max(0, ArgbModeCombo.SelectedIndex);
        var config = new ArgbConfig(mode, Colors.White, 100, 3, 1, 0, false);

        ArgbColorPanel.Visibility = config.UsesColor ? Visibility.Visible : Visibility.Collapsed;
        var paletteMode = mode is ArgbMode.RainbowWave or ArgbMode.ColorWave;
        ArgbRainbowCheck.Visibility = paletteMode ? Visibility.Visible : Visibility.Collapsed;
        ArgbPalettePanel.Visibility = paletteMode ? Visibility.Visible : Visibility.Collapsed;
        ArgbColor2Box.IsEnabled = paletteMode && ArgbRainbowCheck.IsChecked != true;
        ArgbColor3Box.IsEnabled = paletteMode && ArgbRainbowCheck.IsChecked != true;
        var board = ArgbMethodCombo.SelectedIndex == 1;
        ArgbSpeedPanel.Visibility = !board && mode is
            (ArgbMode.Breathing or ArgbMode.Spectrum or ArgbMode.RainbowWave or ArgbMode.Comet or ArgbMode.ColorWave)
            ? Visibility.Visible
            : Visibility.Collapsed;

        var level = (int)Math.Round(ArgbBrightness.Value);
        ArgbBrightnessLabel.Text = level == 0 ? "Яркость: выключено" : $"Яркость: {level}%";
        ArgbSpeedLabel.Text = $"Скорость: {(int)Math.Round(ArgbSpeed.Value)}";
        ArgbLedsLabel.Text = $"Светодиодов: {(int)Math.Round(ArgbLeds.Value)}";
        ArgbFansLabel.Text = $"Вентиляторов в цепочке: {(int)Math.Round(ArgbFans.Value)}";

        ArgbModeHint.Text = mode switch
        {
            ArgbMode.NotControlled => "Программа не трогает разъём: светится так, как настроено в Armoury Crate или BIOS. Если до этого цвета задавала программа, плата вернёт свои после перезагрузки.",
            ArgbMode.CpuTemp => "Голубой — холодно (до 40 °C), зелёный — 60 °C, жёлтый — 75 °C, красный — от 90 °C.",
            ArgbMode.Keyboard => "Каждый вентилятор повторяет свою часть клавиатуры (слева направо): эффекты, волна от центра и раскраска переходят на вентиляторы.",
            ArgbMode.RainbowWave or ArgbMode.ColorWave => "Обычная плавная волна. Включите «Радуга» для спектра или выключите её и задайте до трёх цветов — они плавно смешаются без тёмных провалов.",
            _ => ""
        };
        ArgbModeHint.Visibility = ArgbModeHint.Text.Length > 0 ? Visibility.Visible : Visibility.Collapsed;

        // Цвет: круг, палитра и поле #RRGGBB — как у подсветки клавиатуры.
        var (r, g, b) = Ak820Protocol.ParseColor(ArgbColorBox.Text, (0, 180, 255));
        var color = Color.FromRgb(r, g, b);
        ArgbColorSwatch.Background = new SolidColorBrush(color);
        var color2 = ParseArgbPaletteColor(ArgbColor2Box.Text, color);
        var color3 = ParseArgbPaletteColor(ArgbColor3Box.Text, color2);
        ArgbColor2Box.Background = SolidBrush(color2);
        ArgbColor3Box.Background = SolidBrush(color3);
        ArgbColor2Box.Foreground = TextBrush(color2);
        ArgbColor3Box.Foreground = TextBrush(color3);
        ArgbColor2Swatch.Background = SolidBrush(color2);
        ArgbColor3Swatch.Background = SolidBrush(color3);
        ArgbColor2Swatch.BorderBrush = new SolidColorBrush(ContrastBorder(color2));
        ArgbColor3Swatch.BorderBrush = new SolidColorBrush(ContrastBorder(color3));
        if (!_argbColorFromWheel) _argbWheel?.SetColor(color);

        var hex = ColorWheel.Hex(color);
        foreach (Border swatch in ArgbSwatches.Children)
        {
            swatch.SetResourceReference(Border.BorderBrushProperty,
                string.Equals(swatch.Tag as string, hex, StringComparison.OrdinalIgnoreCase) ? "AccentDeepBrush" : "LineBrush");
        }
    }

    private static Color ParseArgbPaletteColor(string text, Color fallback)
    {
        var (r, g, b) = Ak820Protocol.ParseColor(text, (fallback.R, fallback.G, fallback.B));
        return Color.FromRgb(r, g, b);
    }

    private static SolidColorBrush SolidBrush(Color color)
    {
        var brush = new SolidColorBrush(color);
        brush.Freeze();
        return brush;
    }

    private static SolidColorBrush TextBrush(Color color) =>
        0.299 * color.R + 0.587 * color.G + 0.114 * color.B > 150 ? Brushes.Black : Brushes.White;

    private static Color ContrastBorder(Color color) =>
        0.299 * color.R + 0.587 * color.G + 0.114 * color.B > 150
            ? Color.FromArgb(180, 0, 0, 0)
            : Color.FromArgb(180, 255, 255, 255);

    private void OnArgbStatus(string text, bool ok)
    {
        ArgbStatusText.Text = text;
        _argbOk = ok;
        ArgbStatusDot.SetResourceReference(Shape.FillProperty, ok ? "GoodBrush" : "BadBrush");

        if (ok && _argb is { Headers: > 0 } argb) FillArgbHeaders(argb.Headers);
        UpdateArmouryPanel();
    }

    private void OnArgbFind(object sender, RoutedEventArgs e)
    {
        _argb?.Reconnect();
        UpdateArmouryPanel();
    }

    // ── Картинка «Сейчас горит» ──────────────────────────────────────────────

    private void UpdateArgbPreviewTimer()
    {
        if (_argbPreviewTimer is null) return;

        if (NavArgb.IsChecked == true && WindowShown) _argbPreviewTimer.Start();
        else _argbPreviewTimer.Stop();
    }

    private long _spectrumCalStart;
    private int _calCircles;
    private (int Mode, int Speed)? _calFor;

    private (int Mode, int Speed)? _rateShownFor;
    private bool _rateSync;

    private const string CalHint =
        "Точно: выберите клавишу и цвет (например, Esc — фиолетовый). Нажмите «Старт», когда она станет этого цвета, " +
        "и «Круг» каждый раз, когда цвет вернётся. Чем больше кругов, тем точнее.";

    /// <summary>
    /// Панель подгонки — когда на клавиатуре встроенный эффект прошивки: ползунок скорости (схема и вентиляторы)
    /// и замер кругов кнопкой.
    /// </summary>
    private void UpdateSpectrumCalPanel()
    {
        var active = LightMirror.ActiveEffect;
        var show = active is not null;
        SpectrumCalPanel.Visibility = show ? Visibility.Visible : Visibility.Collapsed;
        SpectrumCalButtons.Visibility = active is { } m && LightMirror.CanCalibrate(m.Mode) ? Visibility.Visible : Visibility.Collapsed;
        if (active is { } a && _rateShownFor != a) ShowEffectRate(a.Mode, a.Speed);
        if (!show) _rateShownFor = null;

        // Сменился эффект или скорость — замер начинаем заново.
        if (_calFor != active) ResetCal(CalHint);
    }

    private void ResetCal(string text)
    {
        _spectrumCalStart = 0;
        _calCircles = 0;
        _calFor = LightMirror.ActiveEffect;
        SpectrumCalButton.Content = "Старт";
        SpectrumCalText.Text = text;
    }

    private void ShowEffectRate(int mode, int speed)
    {
        _rateShownFor = (mode, speed);
        var rate = LightMirror.RateOf(mode, speed);
        _rateSync = true;
        EffectRateSlider.Value = Math.Clamp(rate, EffectRateSlider.Minimum, EffectRateSlider.Maximum);
        _rateSync = false;
        UpdateEffectRateLabel(mode, speed, rate);
    }

    private void UpdateEffectRateLabel(int mode, int speed, double rate)
    {
        var name = Ak820Protocol.Effects.FirstOrDefault(x => x.Mode == mode).Name ?? $"режим {mode}";
        EffectRateLabel.Text = $"{name}, скорость {speed}: ×{rate.ToString("0.000", System.Globalization.CultureInfo.InvariantCulture)} " +
                               "(меньше — медленнее). Двигайте, пока схема и вентиляторы не совпадут с клавиатурой.";
    }

    private void OnEffectRateChanged(object sender, RoutedPropertyChangedEventArgs<double> e)
    {
        if (_rateSync || EffectRateLabel is null || LightMirror.ActiveEffect is not { } a) return;

        var rate = Math.Round(e.NewValue, 2);
        LightMirror.SetRate(a.Mode, a.Speed, rate);
        UpdateEffectRateLabel(a.Mode, a.Speed, rate);
        _settings.EffectRates = LightMirror.Rates;
        _argbSaveTimer?.Stop();
        _argbSaveTimer?.Start();
    }

    /// <summary>Эффект клавиатуры заново — вместе с ним с нуля идут схема и вентиляторы.</summary>
    private void OnEffectRateToggle(object sender, RoutedEventArgs e) =>
        EffectRateTools.Visibility = EffectRateToggle.IsChecked == true ? Visibility.Visible : Visibility.Collapsed;

    private async void OnEffectRestart(object sender, RoutedEventArgs e)
    {
        EffectRestartButton.IsEnabled = false;
        try { await ApplyLightingAsync(); }
        catch { /* ошибку покажет журнал подсветки */ }
        finally { EffectRestartButton.IsEnabled = true; }
    }

    private void OnSpectrumCal(object sender, RoutedEventArgs e)
    {
        var now = Environment.TickCount64;

        if (_spectrumCalStart == 0)
        {
            _spectrumCalStart = now;
            _calCircles = 0;
            _calFor = LightMirror.ActiveEffect;

            // «Спектр» на схеме начинается с синего: если клавиатура сейчас синяя — круги совпадут сразу.
            if (_calFor is { Mode: 8 }) LightMirror.RestartEffectClock();

            SpectrumCalButton.Content = "Круг";
            SpectrumCalText.Text = "Засекаю… Нажмите «Круг», когда цвет вернётся.";
            return;
        }

        _calCircles++;
        var seconds = (now - _spectrumCalStart) / 1000.0;
        if (LightMirror.Calibrate(seconds, _calCircles) is not { } rate)
        {
            _calCircles--;
            SpectrumCalText.Text = "Слишком быстро — нажимайте, когда цвет вернётся через полный круг.";
            return;
        }

        _settings.EffectRates = LightMirror.Rates;
        _argbSaveTimer?.Stop();
        _argbSaveTimer?.Start();
        _rateShownFor = null;
        SpectrumCalText.Text = $"Кругов: {_calCircles}, круг клавиатуры — {seconds / _calCircles:0.00} с → " +
                               $"×{rate.ToString("0.000", System.Globalization.CultureInfo.InvariantCulture)}. " +
                               "Можно жать «Круг» дальше для точности, затем «Начать одновременно».";
    }

    private void OnSpectrumCalReset(object sender, RoutedEventArgs e)
    {
        _settings.EffectRates = null;
        LightMirror.SetRates(null);
        _settings.Save();
        _rateShownFor = null;
        ResetCal("Подгонка сброшена. " + CalHint);
    }

    private void DrawArgbPreview()
    {
        UpdateSpectrumCalPanel();

        if (!WindowShown || NavArgb.IsChecked != true)
        {
            _argbPreviewTimer?.Stop();
            return;
        }

        var frame = _argb?.Frame;
        var count = frame is null ? 0 : frame.Length / 3;
        ArgbPreviewCard.Visibility = count > 0 ? Visibility.Visible : Visibility.Collapsed;   // пустую карточку не показываем

        // Кадр тот же — перерисовывать нечего.
        if (ReferenceEquals(frame, _argbShownFrame) && _argbDots.Count == count) return;
        _argbShownFrame = frame;

        while (_argbDots.Count < count)
        {
            var brush = new SolidColorBrush(Colors.Black);
            _argbDots.Add(brush);
            ArgbPreview.Children.Add(new Ellipse
            {
                Width = 14,
                Height = 14,
                Margin = new Thickness(0, 0, 5, 5),
                Fill = brush,
                Stroke = new SolidColorBrush(Color.FromArgb(60, 128, 128, 128)),
                StrokeThickness = 1
            });
        }

        while (_argbDots.Count > count)
        {
            _argbDots.RemoveAt(_argbDots.Count - 1);
            ArgbPreview.Children.RemoveAt(ArgbPreview.Children.Count - 1);
        }

        for (var i = 0; i < count; i++)
        {
            var c = Color.FromRgb(frame![i * 3], frame[i * 3 + 1], frame[i * 3 + 2]);
            if (_argbDots[i].Color != c) _argbDots[i].Color = c;
        }
    }

    // ── Armoury Crate ────────────────────────────────────────────────────────

    private static bool ArmouryRunning()
    {
        try
        {
            var processes = Process.GetProcessesByName(ArmouryService);
            foreach (var p in processes) p.Dispose();
            return processes.Length > 0;
        }
        catch
        {
            return false;
        }
    }

    private void UpdateArmouryPanel()
    {
        var controlled = _settings.ArgbMode != (int)ArgbMode.NotControlled;
        var running = ArmouryRunning();

        ArmouryPanel.Visibility = controlled && running ? Visibility.Visible : Visibility.Collapsed;
        ArmouryOffPanel.Visibility = _settings.ArmouryStopped && !running ? Visibility.Visible : Visibility.Collapsed;
    }

    private async void OnArmouryStop(object sender, RoutedEventArgs e)
    {
        var answer = MessageBox.Show(this,
            "Остановить службу подсветки Armoury Crate (LightingService)?\n\n" +
            "Пока она остановлена, Armoury Crate не меняет подсветку платы и других устройств ASUS. " +
            "Вернуть можно кнопкой «Вернуть службу» на этой странице.",
            "Rig Control", MessageBoxButton.YesNo, MessageBoxImage.Question);
        if (answer != MessageBoxResult.Yes) return;

        ArmouryStopButton.IsEnabled = false;
        await Task.Run(() =>
        {
            RunSc($"config {ArmouryService} start= demand");
            RunSc($"stop {ArmouryService}");
        });
        await Task.Delay(1500);

        _settings.ArmouryStopped = true;
        _settings.Save();
        ArmouryStopButton.IsEnabled = true;

        _argb?.Reconnect();
        UpdateArmouryPanel();
    }

    private async void OnArmouryStart(object sender, RoutedEventArgs e)
    {
        ArmouryStartButton.IsEnabled = false;
        await Task.Run(() =>
        {
            RunSc($"config {ArmouryService} start= auto");
            RunSc($"start {ArmouryService}");
        });
        await Task.Delay(1500);

        _settings.ArmouryStopped = false;
        _settings.Save();
        ArmouryStartButton.IsEnabled = true;
        UpdateArmouryPanel();
    }

    private static void RunSc(string arguments)
    {
        try
        {
            using var process = Process.Start(new ProcessStartInfo("sc.exe", arguments)
            {
                CreateNoWindow = true,
                UseShellExecute = false
            });
            process?.WaitForExit(10_000);
        }
        catch (Exception ex)
        {
            // Нет службы или прав — кнопка просто ничего не сделает.
            QuietLog.Write(ex);
        }
    }
}
