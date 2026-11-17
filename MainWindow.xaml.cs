using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Interop;
using System.Windows.Media;
using System.Windows.Media.Animation;
using System.Windows.Threading;
using Microsoft.Win32;

namespace RigControl;

public partial class MainWindow : Window
{
    private readonly Settings _settings = Settings.Load();

    private Ak820Device? _device;
    private Ak820Dongle? _dongle;
    private System.Windows.Threading.DispatcherTimer? _linkTimer;
    private bool _linkBusy;
    private System.Windows.Threading.DispatcherTimer? _autoApplyTimer;
    private bool _lightBusy;
    private bool _lightOff;
    private long _lastLightRecover;
    private System.Windows.Threading.DispatcherTimer? _lightTimer;
    private List<ScreenFrame>? _frames;
    private string? _imagePath;
    private bool _loading = true;
    private bool _busy;
    private CancellationTokenSource? _uploadCts;
    private CancellationTokenSource? _lightSyncCts;

    private readonly bool _startHidden;
    private bool _exiting;
    private bool _trayHintShown;
    private TrayIcon? _tray;
    private MenuItem? _trayToggle;

    public MainWindow()
    {
        _startHidden = Environment.GetCommandLineArgs().Skip(1)
            .Any(a => a.Equals("--tray", StringComparison.OrdinalIgnoreCase));

        InitializeComponent();
        AddHandler(RadioButton.CheckedEvent, new RoutedEventHandler(OnAnySegmentChecked));

        if (_startHidden)
        {
            ShowActivated = false;
            WindowState = WindowState.Minimized;
            ShowInTaskbar = false;
        }

        // Кортежи нельзя показать через DisplayMemberPath (у них поля, а не свойства) — отдаём имена.
        EffectCombo.ItemsSource = Ak820Protocol.Effects.Select(e => e.Name).ToList();
        ApplySettingsToUi();
        SetupPumpUi();
        SetupKeyPaintUi();
        SetupLightColor();
        CreateTray();
        StartMemoryWatch();

        _autoApplyTimer = new System.Windows.Threading.DispatcherTimer
        {
            Interval = TimeSpan.FromMilliseconds(300)
        };
        _autoApplyTimer.Tick += async (_, _) =>
        {
            _autoApplyTimer!.Stop();
            await ApplyLightingAsync();
        };
        _autoApplyTimer.Stop();

        Closing += OnWindowClosing;
        Application.Current.SessionEnding += (_, _) => _exiting = true;

        Loaded += (_, _) => _ = InitializeAfterShowAsync();

        Closed += (_, _) =>
        {
            StopLinkWatch();
            _batteryTimer?.Stop();
            _devicesTimer?.Stop();
            StopMouseWatch();
            StopLightWatch();
            StopLightSync();
            CollectCoolerSettings();
            _settings.Save();
            _device?.Dispose();
            _dongle?.Dispose();
            CoolerShutdown();
            ArgbShutdown();

            _tray?.Dispose();
            _tray = null;
        };
    }

    private async Task InitializeAfterShowAsync()
    {
        try
        {
            _loading = false;

            if (_startHidden) HideToTray(silent: true);

            // Сначала отдаём Windows первый кадр окна, затем запускаем HID и помпу.
            await Dispatcher.Yield(DispatcherPriority.ApplicationIdle);

            InitDevicesPage();
            await InitSettingsPageAsync();

            if (!string.IsNullOrWhiteSpace(_settings.LastImage) && File.Exists(_settings.LastImage))
                LoadImage(_settings.LastImage, quiet: true);

            await ConnectAsync();
            StartLinkWatch();
            StartBatteryWatch();
            StartLightWatch();
            await InitCoolerAsync();
            InitArgb();
            StartMouseWatch();
            InitProfiles();
            AnimateNavGlass(GetActiveNavButton());

            // Прошлый запуск закрыли аварийно, пока программа управляла вентиляторами, — предупреждаем.
            if (Telemetry.LeftoverFromCrash)
                _tray?.ShowBalloon("Вентиляторы платы",
                    "Прошлый запуск Rig Control закрылся аварийно. Если ПК с тех пор не перезагружали, вентиляторы могут крутиться на последней заданной скорости — перезагрузите его, чтобы управление вернулось к BIOS.");

            // Запуск сразу в трей: после загрузки всего отдаём лишнюю память.
            if (!WindowShown) ScheduleMemoryTrim();
        }
        catch (Exception ex)
        {
            QuietLog.Write(ex);
            try
            {
                DevicesSummary.Text = "Запуск остановлен — подробности записаны в error.log.";
                MessageBox.Show(this,
                    "Rig Control не смогла завершить запуск.\n\n" +
                    "Подробности сохранены в error.log рядом с настройками программы.\n\n" +
                    ex.Message,
                    "Ошибка запуска Rig Control",
                    MessageBoxButton.OK,
                    MessageBoxImage.Error);
            }
            catch
            {
                // Если ошибка произошла ещё до полной загрузки интерфейса, журнал уже записан.
            }
        }
    }

    // ── Оформление окна ──────────────────────────────────────────────────────

    [DllImport("dwmapi.dll")]
    private static extern int DwmSetWindowAttribute(IntPtr hwnd, int attribute, ref int value, int size);

    private const int DwmUseImmersiveDarkMode = 20;
    private const int DwmCornerPreference = 33;

    [StructLayout(LayoutKind.Sequential)]
    private struct Rect32 { public int Left, Top, Right, Bottom; }

    [StructLayout(LayoutKind.Sequential)]
    private struct MonitorInfo { public int Size; public Rect32 Monitor; public Rect32 Work; public int Flags; }

    [DllImport("user32.dll")]
    private static extern IntPtr MonitorFromWindow(IntPtr hwnd, int flags);

    [DllImport("user32.dll")]
    private static extern bool GetMonitorInfo(IntPtr monitor, ref MonitorInfo info);

    [DllImport("user32.dll")]
    private static extern bool GetWindowRect(IntPtr hwnd, out Rect32 rect);

    protected override void OnSourceInitialized(EventArgs e)
    {
        base.OnSourceInitialized(e);

        ApplyWindowFrameTheme();
        try
        {
            // Windows 11: скруглённые углы и у окна со своим заголовком.
            var round = 2;
            _ = DwmSetWindowAttribute(new WindowInteropHelper(this).Handle, DwmCornerPreference, ref round, sizeof(int));
        }
        catch
        {
            // старая Windows
        }

        StateChanged += (_, _) => UpdateMaximizedLayout();
        SizeChanged += (_, _) => { if (WindowState == WindowState.Maximized) UpdateMaximizedLayout(); };
        LocationChanged += (_, _) => { if (WindowState == WindowState.Maximized) UpdateMaximizedLayout(); };
        UpdateMaximizedLayout();
    }

    /// <summary>Тёмная рамка окна (Windows 11) — под чёрную тему.</summary>
    private void ApplyWindowFrameTheme()
    {
        try
        {
            var hwnd = new WindowInteropHelper(this).Handle;
            if (hwnd == IntPtr.Zero) return;
            var dark = 1;
            _ = DwmSetWindowAttribute(hwnd, DwmUseImmersiveDarkMode, ref dark, sizeof(int));
        }
        catch
        {
            // старая Windows
        }
    }

    /// <summary>
    /// Развёрнутое окно со своим заголовком вылезает за край экрана на ширину рамки —
    /// отступаем ровно на столько, сколько окно вылезло за рабочую область.
    /// </summary>
    private void UpdateMaximizedLayout()
    {
        MaximizeButton.Content = WindowState == WindowState.Maximized ? "\uE923" : "\uE922";
        MaximizeButton.ToolTip = WindowState == WindowState.Maximized ? "Свернуть в окно" : "Развернуть";

        if (WindowState != WindowState.Maximized)
        {
            RootGrid.Margin = new Thickness(0);
            return;
        }

        try
        {
            var hwnd = new WindowInteropHelper(this).Handle;
            if (hwnd == IntPtr.Zero || !GetWindowRect(hwnd, out var w)) return;

            var info = new MonitorInfo { Size = Marshal.SizeOf<MonitorInfo>() };
            if (!GetMonitorInfo(MonitorFromWindow(hwnd, 2), ref info)) return;

            var dpi = VisualTreeHelper.GetDpi(this);
            RootGrid.Margin = new Thickness(
                Math.Max(0, info.Work.Left - w.Left) / dpi.DpiScaleX,
                Math.Max(0, info.Work.Top - w.Top) / dpi.DpiScaleY,
                Math.Max(0, w.Right - info.Work.Right) / dpi.DpiScaleX,
                Math.Max(0, w.Bottom - info.Work.Bottom) / dpi.DpiScaleY);
        }
        catch
        {
            RootGrid.Margin = new Thickness(7);
        }
    }

    private void OnMinimizeClick(object sender, RoutedEventArgs e) => WindowState = WindowState.Minimized;

    private void OnMaximizeClick(object sender, RoutedEventArgs e) =>
        WindowState = WindowState == WindowState.Maximized ? WindowState.Normal : WindowState.Maximized;

    private void OnCloseClick(object sender, RoutedEventArgs e) => Close();

    // ── Навигация ────────────────────────────────────────────────────────────

    private void OnNavChecked(object sender, RoutedEventArgs e)
    {
        if (PagePump is null || PageKeyboard is null || PageSettings is null || PageArgb is null || PageFans is null
            || PageDevices is null || PageMouse is null || PageSystem is null) return;

        var pages = new[]
        {
            PageDevices, PageMouse, PageSystem, PagePump, PageKeyboard,
            PageSettings, PageArgb, PageFans
        };
        foreach (var page in pages)
            page.Visibility = Visibility.Collapsed;

        var active = NavDevices.IsChecked == true ? PageDevices
            : NavMouse.IsChecked == true ? PageMouse
            : NavSystem.IsChecked == true ? PageSystem
            : NavPump.IsChecked == true ? PagePump
            : NavKeyboard.IsChecked == true ? PageKeyboard
            : NavSettings.IsChecked == true ? PageSettings
            : NavArgb.IsChecked == true ? PageArgb
            : PageFans;

        active.Visibility = Visibility.Visible;
        AnimatePageIn(active);
        AnimateNavGlass(sender as RadioButton);
        UpdateTelemetryTimerState();
        UpdateArgbPreviewTimer();
        if (NavPump.IsChecked == true) RefreshCoolerLog();
        if (NavSystem.IsChecked == true) ShowSystemPage();
    }

    private void AnimatePageIn(UIElement page)
    {
        if (!IsLoaded)
        {
            page.Opacity = 1;
            page.RenderTransform = null;
            return;
        }

        page.Opacity = 0;
        page.RenderTransform = new TranslateTransform(0, 10);

        var storyboard = new Storyboard();
        var fade = new DoubleAnimation(0, 1, TimeSpan.FromMilliseconds(180))
        {
            EasingFunction = new CubicEase { EasingMode = EasingMode.EaseOut }
        };
        var slide = new DoubleAnimation(10, 0, TimeSpan.FromMilliseconds(220))
        {
            EasingFunction = new CubicEase { EasingMode = EasingMode.EaseOut }
        };
        Storyboard.SetTarget(fade, page);
        Storyboard.SetTargetProperty(fade, new PropertyPath(UIElement.OpacityProperty));
        Storyboard.SetTarget(slide, page);
        Storyboard.SetTargetProperty(slide,
            new PropertyPath("(UIElement.RenderTransform).(TranslateTransform.Y)"));
        storyboard.Children.Add(fade);
        storyboard.Children.Add(slide);
        storyboard.Completed += (_, _) => page.RenderTransform = null;
        storyboard.Begin();
    }

    private void OnNavScrollChanged(object sender, ScrollChangedEventArgs e) =>
        AnimateNavGlass(GetActiveNavButton(), immediate: true);

    private void OnNavHostSizeChanged(object sender, SizeChangedEventArgs e) =>
        AnimateNavGlass(GetActiveNavButton(), immediate: true);

    private bool _navDragging;
    private bool _navDragMoved;
    private Point _navDragStart;

    private RadioButton[] NavButtons() =>
    [
        NavDevices, NavKeyboard, NavMouse, NavPump, NavArgb, NavFans, NavSystem, NavSettings
    ];

    private void OnNavPreviewMouseLeftButtonDown(object sender, MouseButtonEventArgs e)
    {
        var selected = GetActiveNavButton();
        if (selected is null || NavHost is null)
            return;

        var point = e.GetPosition(NavHost);
        var origin = selected.TransformToAncestor(NavHost).Transform(new Point(0, 0));
        var bounds = new Rect(origin.X, origin.Y, selected.ActualWidth, selected.ActualHeight);
        if (!bounds.Contains(point))
            return;

        _navDragging = true;
        _navDragMoved = false;
        _navDragStart = point;
        NavHost.CaptureMouse();
    }

    private void OnNavPreviewMouseMove(object sender, System.Windows.Input.MouseEventArgs e)
    {
        if (!_navDragging || NavHost is null ||
            e.LeftButton != System.Windows.Input.MouseButtonState.Pressed)
            return;

        var point = e.GetPosition(NavHost);
        if (!_navDragMoved &&
            (Math.Abs(point.X - _navDragStart.X) > 4 || Math.Abs(point.Y - _navDragStart.Y) > 4))
            _navDragMoved = true;

        if (!_navDragMoved)
            return;

        var hovered = NavButtons().FirstOrDefault(button =>
        {
            if (!button.IsLoaded || button.Visibility != Visibility.Visible) return false;
            var origin = button.TransformToAncestor(NavHost).Transform(new Point(0, 0));
            return new Rect(origin.X, origin.Y, button.ActualWidth, button.ActualHeight).Contains(point);
        });

        if (hovered is not null && hovered.IsChecked != true)
            hovered.IsChecked = true;

        e.Handled = true;
    }

    private void OnNavPreviewMouseLeftButtonUp(object sender, MouseButtonEventArgs e)
    {
        if (!_navDragging)
            return;

        var moved = _navDragMoved;
        StopNavDrag();
        if (moved)
            e.Handled = true;
    }

    private void OnNavLostMouseCapture(object sender, MouseEventArgs e) => StopNavDrag();

    private void StopNavDrag()
    {
        _navDragging = false;
        _navDragMoved = false;
        if (NavHost?.IsMouseCaptured == true)
            NavHost.ReleaseMouseCapture();
    }

    private RadioButton? GetActiveNavButton() =>
        NavDevices?.IsChecked == true ? NavDevices
        : NavKeyboard?.IsChecked == true ? NavKeyboard
        : NavMouse?.IsChecked == true ? NavMouse
        : NavPump?.IsChecked == true ? NavPump
        : NavArgb?.IsChecked == true ? NavArgb
        : NavFans?.IsChecked == true ? NavFans
        : NavSystem?.IsChecked == true ? NavSystem
        : NavSettings?.IsChecked == true ? NavSettings
        : null;

    private void AnimateNavGlass(RadioButton? selected, bool immediate = false)
    {
        if (selected is null || NavHost is null || NavGlassHighlight is null || !selected.IsLoaded)
            return;

        Dispatcher.BeginInvoke(DispatcherPriority.Loaded, new Action(() =>
        {
            if (!selected.IsLoaded || selected.Visibility != Visibility.Visible)
                return;

            if (NavHost.ActualWidth <= 0 || NavHost.ActualHeight <= 0)
                return;

            Point point;
            try
            {
                point = selected.TransformToAncestor(NavHost).Transform(new Point(0, 0));
            }
            catch (InvalidOperationException)
            {
                return;
            }

            var width = Math.Min(selected.ActualWidth + 2, NavHost.ActualWidth);
            var height = Math.Min(selected.ActualHeight + 2, NavHost.ActualHeight);
            var left = Math.Clamp(point.X - 1, 0, Math.Max(0, NavHost.ActualWidth - width));
            var top = Math.Clamp(point.Y - 1, 0, Math.Max(0, NavHost.ActualHeight - height));

            NavGlassHighlight.Visibility = Visibility.Visible;
            if (immediate || NavGlassHighlight.Width <= 1)
            {
                NavGlassHighlight.BeginAnimation(Canvas.LeftProperty, null);
                NavGlassHighlight.BeginAnimation(Canvas.TopProperty, null);
                NavGlassHighlight.BeginAnimation(FrameworkElement.WidthProperty, null);
                NavGlassHighlight.BeginAnimation(FrameworkElement.HeightProperty, null);
                Canvas.SetLeft(NavGlassHighlight, left);
                Canvas.SetTop(NavGlassHighlight, top);
                NavGlassHighlight.Width = width;
                NavGlassHighlight.Height = height;
                return;
            }

            var storyboard = new Storyboard();
            var duration = new Duration(TimeSpan.FromMilliseconds(240));
            var easing = new CubicEase { EasingMode = EasingMode.EaseOut };
            AddNavGlassAnimation(storyboard, Canvas.LeftProperty, left, duration, easing);
            AddNavGlassAnimation(storyboard, Canvas.TopProperty, top, duration, easing);
            AddNavGlassAnimation(storyboard, FrameworkElement.WidthProperty, width, duration, easing);
            AddNavGlassAnimation(storyboard, FrameworkElement.HeightProperty, height, duration, easing);
            storyboard.Begin(NavGlassHighlight, HandoffBehavior.SnapshotAndReplace, true);
        }));
    }

    private static void AddNavGlassAnimation(Storyboard storyboard, DependencyProperty property,
        double value, Duration duration, IEasingFunction easing)
    {
        var animation = new DoubleAnimation(value, duration) { EasingFunction = easing };
        Storyboard.SetTargetProperty(animation, new PropertyPath(property));
        storyboard.Children.Add(animation);
    }

    private void OnAnySegmentChecked(object sender, RoutedEventArgs e)
    {
        if (sender is not RadioButton button ||
            !ReferenceEquals(button.Style, TryFindResource("SegButton")))
            return;

        // У SegButton стекло находится внутри ControlTemplate, поэтому XAML EventTrigger
        // с TargetName ломается на некоторых версиях WPF. Анимация от объекта к объекту
        // безопасна и заодно работает для всех сегментированных переключателей приложения.
        Dispatcher.BeginInvoke(DispatcherPriority.Render,
            new Action(() => AnimateSegmentGlass(button)));
    }

    private void AnimateSegmentGlass(RadioButton button)
    {
        if (!button.IsLoaded || button.IsChecked != true)
            return;

        var glass = FindNamedVisual<Border>(button, "Glass");
        var shine = FindNamedVisual<Border>(button, "GlassShine");
        if (glass is null)
            return;

        var scale = glass.RenderTransform as ScaleTransform;
        if (scale is null)
        {
            scale = new ScaleTransform(1, 1);
            glass.RenderTransform = scale;
        }

        scale.ScaleX = 0.96;
        scale.ScaleY = 0.96;
        glass.Opacity = 0.2;
        if (shine is not null) shine.Opacity = 0;

        var easing = new CubicEase { EasingMode = EasingMode.EaseOut };
        var duration = TimeSpan.FromMilliseconds(220);
        glass.BeginAnimation(UIElement.OpacityProperty,
            new DoubleAnimation(0.2, 1, duration) { EasingFunction = easing });
        scale.BeginAnimation(ScaleTransform.ScaleXProperty,
            new DoubleAnimation(0.96, 1, duration) { EasingFunction = easing });
        scale.BeginAnimation(ScaleTransform.ScaleYProperty,
            new DoubleAnimation(0.96, 1, duration) { EasingFunction = easing });

        if (shine is not null)
        {
            shine.BeginAnimation(UIElement.OpacityProperty,
                new DoubleAnimation(0, 1, TimeSpan.FromMilliseconds(180))
                {
                    EasingFunction = easing
                });
        }
    }

    private static T? FindNamedVisual<T>(DependencyObject? root, string name)
        where T : FrameworkElement
    {
        if (root is T element && element.Name == name)
            return element;

        if (root is null)
            return null;

        for (var i = 0; i < VisualTreeHelper.GetChildrenCount(root); i++)
        {
            var found = FindNamedVisual<T>(VisualTreeHelper.GetChild(root, i), name);
            if (found is not null)
                return found;
        }

        return null;
    }

    private void OnKbdTabChecked(object sender, RoutedEventArgs e)
    {
        if (KbdScreenPanel is null) return;

        KbdScreenPanel.Visibility = KbdTabScreen.IsChecked == true ? Visibility.Visible : Visibility.Collapsed;
        if (KbdPaintPanel is not null)
            KbdPaintPanel.Visibility = KbdTabPaint.IsChecked == true ? Visibility.Visible : Visibility.Collapsed;
    }

    // ── Трей ─────────────────────────────────────────────────────────────────

    private void CreateTray()
    {
        try
        {
            _tray = new TrayIcon("Rig Control") { MenuProvider = BuildTrayMenu };
            _tray.DoubleClick += ShowFromTray;
        }
        catch
        {
            // без значка окно всё равно работает
            _tray = null;
        }
    }

    /// <summary>Меню значка в трее (строится заново при каждом открытии — в текущей теме).</summary>
    private ContextMenu BuildTrayMenu()
    {
        var menu = new ContextMenu { Style = TryFindResource("TrayMenu") as Style };
        var itemStyle = TryFindResource("TrayMenuItem") as Style;

        MenuItem Add(string header, Action click)
        {
            var item = new MenuItem { Header = header, Style = itemStyle };
            item.Click += (_, _) => click();
            menu.Items.Add(item);
            return item;
        }

        Add("Открыть Rig Control", ShowFromTray);
        _trayToggle = Add(CoolerRunning ? "Остановить экран помпы" : "Запустить экран помпы", () => _ = ToggleCoolerAsync());
        menu.Items.Add(new Separator { Style = TryFindResource("TrayMenuSeparator") as Style });
        AddTrayProfiles(menu, itemStyle);
        Add("Выход", ExitApp);
        return menu;
    }

    private void HideToTray(bool silent = false)
    {
        Hide();
        ScheduleMemoryTrim();
        if (CoolerRunning) PumpLog.Write("окно убрано в трей (экран помпы продолжает работать)");

        if (!silent && !_trayHintShown && _tray is not null)
        {
            _trayHintShown = true;
            _tray.ShowBalloon("Rig Control", "Программа работает в трее. Выход — в меню значка.");
        }
    }

    /// <summary>Показывает окно из трея (вызывается и второй копией программы).</summary>
    public void ShowFromTray()
    {
        ShowInTaskbar = true;
        Show();

        if (WindowState == WindowState.Minimized) WindowState = WindowState.Normal;

        Activate();
        if (CoolerRunning) PumpLog.Write("окно возвращено из трея");
    }

    private void ExitApp()
    {
        _exiting = true;
        Close();
    }

    private void OnWindowClosing(object? sender, CancelEventArgs e)
    {
        if (_exiting || !_settings.CloseToTray) return;

        e.Cancel = true;
        HideToTray();
    }

    // ── Страница «Настройки» ─────────────────────────────────────────────────

    private async Task InitSettingsPageAsync()
    {
        SwitchTray.IsChecked = _settings.CloseToTray;
        SwitchAutoOutput.IsChecked = _settings.AutoOutput;

        var version = System.Reflection.Assembly.GetExecutingAssembly().GetName().Version;
        AboutText.Text = $"Rig Control {version?.ToString(3)}\nAJAZZ AK820 Pro · MCHOSE A7 Ultra · Thermalright LCD · ASUS Aura ARGB";

        SwitchAutostart.IsEnabled = false;
        var (enabled, repaired, needsConfirm, exposed) = await Task.Run(() =>
        {
            var on = Autostart.IsEnabledRepair(out var fixedPath, out var confirm);
            var unsafeFolder = on && !confirm && !Autostart.IsProtectedLocation(Environment.ProcessPath);
            return (on, fixedPath, confirm, unsafeFolder);
        });
        SwitchAutostart.IsChecked = enabled;
        if (repaired) SetAutostartNote("Программа перенесена — автозапуск обновлён на новую папку.");
        else if (needsConfirm) SetAutostartNote("Автозапуск открывает копию программы из другой папки. Чтобы он открывал эту, выключите и снова включите переключатель.");
        else if (exposed) SetAutostartNote(Autostart.UnsafeLocationNote);
        SwitchAutostart.IsEnabled = true;
    }

    private async void OnSettingAutostart(object sender, RoutedEventArgs e)
    {
        var want = SwitchAutostart.IsChecked == true;
        SwitchAutostart.IsEnabled = false;
        SetAutostartNote("");

        // Задача запустит exe с правами администратора без UAC. Если exe или его папку могут менять обычные программы,
        // любая из них подменит файл и получит права администратора, — предупреждаем и спрашиваем.
        var exe = Environment.ProcessPath;
        var exposed = want && !await Task.Run(() => Autostart.IsProtectedLocation(exe));
        if (exposed)
        {
            var answer = MessageBox.Show(this,
                "RigControl.exe лежит там, где его могут менять программы без прав администратора:\n" +
                System.IO.Path.GetDirectoryName(exe) + "\n\n" +
                "Автозапуск открывает Rig Control с правами администратора без запроса UAC. Отсюда любая программа " +
                "может подменить RigControl.exe и при следующем входе в Windows получить эти права.\n\n" +
                "Безопаснее скопировать RigControl.exe в C:\\Program Files\\RigControl (именно скопировать, а не перенести) " +
                "и включить автозапуск оттуда.\n\n" +
                "Всё равно включить автозапуск отсюда?",
                "Rig Control", MessageBoxButton.YesNo, MessageBoxImage.Warning, MessageBoxResult.No);

            if (answer != MessageBoxResult.Yes)
            {
                SwitchAutostart.IsChecked = false;
                SetAutostartNote(Autostart.UnsafeLocationNote);
                SwitchAutostart.IsEnabled = true;
                return;
            }
        }

        var (ok, message) = await Task.Run(() =>
        {
            var result = Autostart.Set(want, out var text);
            return (result, text);
        });

        if (!ok)
        {
            SwitchAutostart.IsChecked = !want;
            SetAutostartNote(message);
        }
        else
        {
            SetAutostartNote(!want ? "Выключено." : exposed ? "Включено. " + Autostart.UnsafeLocationNote : "Включено.");
        }

        SwitchAutostart.IsEnabled = true;
    }

    private void SetAutostartNote(string text)
    {
        AutostartNote.Text = text;
        AutostartNote.Visibility = string.IsNullOrEmpty(text) ? Visibility.Collapsed : Visibility.Visible;
    }

    private void OnSettingTray(object sender, RoutedEventArgs e)
    {
        _settings.CloseToTray = SwitchTray.IsChecked == true;
        _settings.Save();
    }

    private void OnSettingAutoOutput(object sender, RoutedEventArgs e)
    {
        _settings.AutoOutput = SwitchAutoOutput.IsChecked == true;
        _settings.Save();

        // Включили автовывод — сразу пробуем запустить, не дожидаясь следующей проверки.
        if (_settings.AutoOutput) _userStopped = false;
    }

    // ── Настройки ↔ интерфейс ────────────────────────────────────────────────

    private void ApplySettingsToUi()
    {
        _loading = true;

        FitCombo.SelectedIndex = _settings.Crop ? 0 : 1;
        SelectMaxFrames(_settings.MaxFrames);
        EffectCombo.SelectedIndex = Math.Clamp(_settings.Effect, 0,
            Ak820Protocol.Effects.Length - 1);
        ColorBox.Text = _settings.Color;

        // Для всегда-радужных эффектов галочка стоит и заблокирована.
        var forced = Ak820Protocol.Effects[Math.Clamp(_settings.Effect, 0,
            Ak820Protocol.Effects.Length - 1)].Mode is 6 or 8;
        RainbowCheck.IsChecked = forced || _settings.Rainbow;
        _rainbowAuto = forced;

        SyncLightCheck.IsChecked = _settings.SyncLight;
        SyncStyleCombo.SelectedIndex = Math.Clamp(_settings.SyncStyle, 0, 2);
        BrightnessSlider.Value = Math.Clamp(_settings.Brightness, 0, 5);
        SpeedSlider.Value = Math.Clamp(_settings.Speed, 0, 5);
        DirectionCombo.SelectedIndex = Math.Clamp(_settings.Direction, 0, 3);

        UpdateBrightnessLabel();
        UpdateSpeedLabel();
        UpdateColorSwatch();
        UpdateSyncUi();

        _loading = false;
    }

    private int SelectedMaxFrames() =>
        MaxFramesCombo.SelectedItem is ComboBoxItem { Tag: string tag } && int.TryParse(tag, out var n) ? n : 0;

    private void SelectMaxFrames(int value)
    {
        foreach (ComboBoxItem item in MaxFramesCombo.Items)
        {
            if (item.Tag is string tag && int.TryParse(tag, out var n) && n == value)
            {
                MaxFramesCombo.SelectedItem = item;
                return;
            }
        }

        MaxFramesCombo.SelectedIndex = 0;
    }

    private void CollectSettings()
    {
        _settings.LastImage = _imagePath;
        _settings.Crop = FitCombo.SelectedIndex == 0;
        _settings.MaxFrames = SelectedMaxFrames();
        _settings.Effect = Math.Max(0, EffectCombo.SelectedIndex);
        _settings.Color = ColorBox.Text.Trim();
        _settings.Rainbow = RainbowCheck.IsChecked == true;
        _settings.SyncLight = SyncLightCheck.IsChecked == true;
        _settings.SyncStyle = Math.Clamp(SyncStyleCombo.SelectedIndex, 0, 2);
        _settings.Brightness = (int)Math.Round(BrightnessSlider.Value);
        _settings.Speed = (int)Math.Round(SpeedSlider.Value);
        _settings.Direction = Math.Clamp(DirectionCombo.SelectedIndex, 0, 3);
    }

    // ── Подключение ──────────────────────────────────────────────────────────

    private bool _connecting;

    /// <summary>Когда последний раз пробовали провод и не смогли открыть его интерфейсы.</summary>
    private long _wiredFailedAt;
    private bool _wiredWasPresent;

    private async Task ConnectAsync()
    {
        // Переподключение могут одновременно запросить кнопка, сторож кабеля и восстановление
        // подсветки. Два параллельных поиска закрывали друг другу только что открытые хэндлы.
        if (_connecting) return;
        _connecting = true;
        try
        {
            await ConnectCoreAsync();
        }
        finally
        {
            _connecting = false;
        }
    }

    private async Task ConnectCoreAsync()
    {
        StopLightSync();
        SetBusy(true);
        SetStatus("Ищу клавиатуру…", "Кабель USB-C или донгл 2,4 ГГц.", connected: false);

        var (device, error, radio) = await Task.Run(() =>
        {
            var found = Ak820Device.TryOpen(out var message);

            // По кабелю не нашли — пробуем донгл 2,4 ГГц.
            var dongle = found is null ? Ak820Dongle.TryOpen() : null;
            return (found, message, dongle);
        });

        var oldDevice = _device;
        var oldDongle = _dongle;
        _device = device;
        _dongle = radio;

        // Закрываем старое в фоне: Dispose ждёт, пока допишется текущая команда.
        _ = Task.Run(() =>
        {
            try { oldDevice?.Dispose(); } catch { }
            try { oldDongle?.Dispose(); } catch { }
        });

        // Кабель есть, а проводные интерфейсы не открылись (например, клавиатура в режиме 2,4 ГГц
        // и кабель только заряжает) — запоминаем, чтобы сторож не переподключался каждые 2 секунды.
        _wiredFailedAt = _device is null && await Task.Run(Ak820Device.IsWiredPresent) == true
            ? Environment.TickCount64
            : 0;

        if (_device is not null)
        {
            SetStatus("Клавиатура подключена", "По кабелю", connected: true);
            SetLog("Клавиатура найдена.");
        }
        else if (_dongle is not null)
        {
            SetStatus("Клавиатура по радио", "Донгл 2,4 ГГц · картинка только по кабелю",
                connected: true);
            SetLog("Найден донгл 2,4 ГГц.");
        }
        else
        {
            // Подсказка — в шапке страницы, технические подробности (список HID-интерфейсов) — во всплывающей подсказке.
            var text = error ?? "Проверьте кабель и режим подключения.";
            var cut = text.IndexOf(" Вендорские интерфейсы:", StringComparison.Ordinal);
            SetStatus("Клавиатура не найдена", cut > 0 ? text[..cut] : text, connected: false);
            StatusHint.ToolTip = cut > 0 ? text[(cut + 1)..] : null;
            SetLog("");
        }

        SetBusy(false);
        UpdateButtons();

        // Гифка уже может быть на экране — тогда подсветка поедет сразу.
        // Если синхронизация выключена, просто возвращаем сохранённые настройки
        // света на клавиатуру: после запуска не нужно жать «Применить».
        await ApplyLightingAsync();
    }

    private async void OnReconnect(object sender, RoutedEventArgs e) => await ConnectAsync();

    /// <summary>
    /// Следит за кабелем: воткнули — само переключается с донгла на провод,
    /// выдернули — обратно на радио. Перезапуск программы не нужен.
    /// </summary>
    private void StartLinkWatch()
    {
        if (_linkTimer is not null) return;

        _linkTimer = new System.Windows.Threading.DispatcherTimer
        {
            Interval = TimeSpan.FromSeconds(2)
        };
        _linkTimer.Tick += async (_, _) => await CheckLinkAsync();
        _linkTimer.Start();
    }

    private void StopLinkWatch()
    {
        _linkTimer?.Stop();
        _linkTimer = null;
    }

    private async Task CheckLinkAsync()
    {
        if (_busy || _linkBusy || _connecting || _uploadCts is not null) return;

        _linkBusy = true;
        try
        {
            if (_device is null)
            {
                // Есть ли кабель? Только тогда переподключаемся, иначе не трогаем радио.
                var present = await Task.Run(Ak820Device.IsWiredPresent);
                var appeared = present == true && !_wiredWasPresent;
                if (present is not null) _wiredWasPresent = present.Value;
                if (present != true) return;

                // Раньше здесь переподключались каждые 2 секунды, если кабель воткнут, а проводной режим
                // не открывается: радио каждый раз закрывалось и открывалось заново, подсветка и часы
                // срывались — «работает через раз». Теперь — только когда кабель появился,
                // или повторная попытка не чаще раза в 30 секунд.
                if (!appeared && _wiredFailedAt != 0 && Environment.TickCount64 - _wiredFailedAt < 30_000) return;

                SetLog("Подключена по кабелю — переключаюсь на провод.");
                await ConnectAsync();
                return;
            }

            // null — не удалось проверить: подключение не трогаем.
            if (await Task.Run(Ak820Device.IsWiredPresent) == false)
            {
                SetLog("Кабель отключён — перехожу на донгл 2,4 ГГц.");
                await ConnectAsync();
            }
        }
        catch (Exception ex)
        {
            // Проверка не удалась — попробуем на следующем тике.
            QuietLog.Write(ex);
        }
        finally
        {
            _linkBusy = false;
        }
    }

    // ── Экран ────────────────────────────────────────────────────────────────

    private void OnPickImage(object sender, RoutedEventArgs e)
    {
        var dialog = new OpenFileDialog
        {
            Title = "Картинка или гифка для экрана",
            Filter = "Изображения|*.png;*.jpg;*.jpeg;*.gif;*.bmp;*.tif;*.tiff|Все файлы|*.*"
        };

        if (dialog.ShowDialog(this) == true)
            LoadImage(dialog.FileName, quiet: false);
    }

    private void LoadImage(string path, bool quiet)
    {
        StopLightSync();

        try
        {
            var crop = FitCombo.SelectedIndex == 0;
            var (frames, preview) = ImageFrames.Load(path, crop, SelectedMaxFrames());

            _frames = frames;
            _imagePath = path;
            UpdateMiniScreen();

            PreviewImage.Source = preview;

            var total = frames.Sum(f => f.DelayMs) / 1000.0;
            FramesText.Text = frames.Count == 1
                ? $"Кадров: 1 · {Path.GetFileName(path)}"
                : $"Кадров: {frames.Count} · {total:0.0} с · {Path.GetFileName(path)}";

            SetLog($"Загружено: {Path.GetFileName(path)}");
            if (!quiet) _settings.Save();
        }
        catch (Exception ex)
        {
            _frames = null;
            _imagePath = null;
            UpdateMiniScreen();
            PreviewImage.Source = null;
            FramesText.Text = "Не удалось прочитать файл";
            SetLog($"Ошибка чтения изображения: {ex.Message}");
        }

        UpdateButtons();
    }

    private async void OnUpload(object sender, RoutedEventArgs e)
    {
        if (_device is null || _frames is null) return;

        byte[] blob;
        try
        {
            blob = Ak820Protocol.ScreenBlob(_frames);
        }
        catch (Exception ex)
        {
            SetLog($"Не удалось собрать кадры: {ex.Message}");
            return;
        }

        CollectSettings();
        _settings.Save();

        StopLightSync();

        var device = _device;
        var progress = new Progress<(int Done, int Total)>(p =>
        {
            UploadProgress.Value = (double)p.Done / p.Total;
            SetLog($"Заливка: {p.Done} из {p.Total} порций…");
        });

        _uploadCts = new CancellationTokenSource();
        var token = _uploadCts.Token;

        SetBusy(true);
        UploadProgress.Visibility = Visibility.Visible;
        UploadProgress.Value = 0;
        CancelButton.Visibility = Visibility.Visible;

        try
        {
            var stats = await Task.Run(() => device.UploadScreen(blob, progress, token));
            SetLog($"Картинка отправлена за {stats.TotalSeconds:0.0} с.");

            StartLightSync();
        }
        catch (OperationCanceledException)
        {
            SetLog("Заливка отменена.");
        }
        catch (Exception ex)
        {
            SetLog($"Ошибка заливки: {ex.Message}");
        }
        finally
        {
            _uploadCts.Dispose();
            _uploadCts = null;
            CancelButton.Visibility = Visibility.Collapsed;
            UploadProgress.Visibility = Visibility.Collapsed;
            SetBusy(false);
            UpdateButtons();
        }
    }

    private void OnCancelUpload(object sender, RoutedEventArgs e) => _uploadCts?.Cancel();

    private async void OnSyncTime(object sender, RoutedEventArgs e)
    {
        var device = _device;
        var dongle = _dongle;
        if (device is null && dongle is null) return;

        SetBusy(true);

        try
        {
            await Task.Run(() =>
            {
                if (device is not null) device.SyncTime(DateTime.Now);
                else dongle!.SyncTime(DateTime.Now);
            });

            SetLog($"Часы на экране выставлены: {DateTime.Now:HH:mm:ss}" + (device is null ? " (по радио)." : "."));
        }
        catch (Exception ex)
        {
            SetLog($"Ошибка синхронизации часов: {ex.Message}");
        }
        finally
        {
            SetBusy(false);
            UpdateButtons();
        }
    }

    private void OnFitChanged(object sender, SelectionChangedEventArgs e)
    {
        if (_loading || _imagePath is null) return;

        LoadImage(_imagePath, quiet: false);
    }

    /// <summary>Код режима, выбранного в списке эффектов.</summary>
    private int SelectedMode =>
        Ak820Protocol.Effects[Math.Clamp(EffectCombo.SelectedIndex, 0,
            Ak820Protocol.Effects.Length - 1)].Mode;

    /// <summary>Выбран свой эффект (рисуется попиксельно, только по кабелю).</summary>
    private bool IsCustomEffect => LightEffects.IsCustom(SelectedMode);

    // ── Подсветка ────────────────────────────────────────────────────────────

    private async void OnApplyEffect(object sender, RoutedEventArgs e)
    {
        var device = _device;
        var dongle = _dongle;
        if (device is null && dongle is null) return;

        CollectSettings();
        _settings.Save();
        SetLightOffState(false);

        // Свои эффекты живут отдельным потоком кадров; при синхронизации
        // «Применить» не должно красить клавиатуру в цвет из поля «Цвет»
        // (по умолчанию белый) — перезапускаем анимацию.
        if (SelectedMode == LightEffects.Paint)
        {
            StopLightSync();
            if (_settings.PaintAnim != 0 && device is not null)
            {
                StartLightSync();
                SetLog($"Раскраска: {KeyPaint.Animations[Math.Clamp(_settings.PaintAnim, 0, KeyPaint.Animations.Length - 1)]}.");
            }
            else
            {
                await ApplyPaintAsync(showResult: true);
            }
            return;
        }

        if (_settings.SyncLight || IsCustomEffect)
        {
            StopLightSync();
            StartLightSync();
            LightLog($"apply: sync={_settings.SyncLight} custom={IsCustomEffect} " +
                     $"started={_lightSyncCts is not null}");

            if (_lightSyncCts is not null)
            {
                SetLog(IsCustomEffect
                    ? "Своя подсветка запущена."
                    : "Подсветка в такт гифке.");
            }
            else if (IsCustomEffect)
            {
                SetLog("Свои эффекты — только по кабелю.");
            }
            else
            {
                SetLog("Нужна гифка на экране.");
            }

            return;
        }

        StopLightSync();

        var mode = SelectedMode;
        LightLog($"apply: manual mode={mode} color={_settings.Color} rainbow={_settings.Rainbow}");

        SetBusy(true);
        try
        {
            await Task.Run(() =>
            {
                if (device is not null)
                {
                    device.SetEffect(mode, _settings.Color, _settings.Rainbow,
                        _settings.Brightness, _settings.Speed, _settings.Direction);
                }
                else
                {
                    var (r, g, b) = Ak820Protocol.ParseColor(_settings.Color, (255, 255, 255));
                    dongle!.SendLight(mode, r, g, b, _settings.Rainbow,
                        _settings.Brightness, _settings.Speed, _settings.Direction);
                }
            });

            SetLog($"Подсветка: {Ak820Protocol.Effects[EffectCombo.SelectedIndex].Name}"
                + (device is null ? " (по радио)." : "."));
        }
        catch (Exception ex)
        {
            SetLog($"Ошибка подсветки: {ex.Message}");
        }
        finally
        {
            SetBusy(false);
            UpdateButtons();
        }
    }

    private async void OnLightOff(object sender, RoutedEventArgs e)
    {
        var device = _device;
        var dongle = _dongle;
        if (device is null && dongle is null) return;

        // Режим «Выключено» в прошивке «залипает»: после него обычные пакеты
        // подсветки игнорируются. Поэтому гасим не режимом 0, а нулевой
        // яркостью текущего эффекта — такой свет потом включается штатно.
        var mode = Ak820Protocol.Effects[Math.Clamp(_settings.Effect, 0,
            Ak820Protocol.Effects.Length - 1)].Mode;
        if (mode == 0 || LightEffects.IsCustom(mode)) mode = 1;

        var color = _settings.Color;
        var rainbow = _settings.Rainbow;
        var speed = Math.Clamp(_settings.Speed, 0, 5);
        var direction = Math.Clamp(_settings.Direction, 0, 3);
        var savedBrightness = Math.Clamp(_settings.Brightness, 0, 5);

        // Кнопка работает как переключатель: выключили — она же включает обратно.
        var turnOn = _lightOff;
        var brightness = turnOn ? Math.Max(1, savedBrightness) : 0;

        StopLightSync();
        SetBusy(true);

        try
        {
            await Task.Run(() =>
            {
                if (device is not null)
                {
                    // Полный пакет с сохранением — он же снимает «залипание» режима 0.
                    device.SetEffect(mode, color, rainbow, brightness, speed, direction);
                }
                else
                {
                    var (r, g, b) = Ak820Protocol.ParseColor(color, (255, 255, 255));
                    dongle!.SendLight(mode, r, g, b, rainbow, brightness, speed, direction);
                }
            });

            SetLightOffState(!turnOn);
            LightLog(turnOn
                ? $"on: mode={mode} bright={brightness} color={color} rainbow={rainbow}"
                : $"off: mode={mode} bright=0");

            if (turnOn)
            {
                if (_settings.SyncLight || IsCustomEffect) StartLightSync();
                SetLog("Подсветка включена в сохранённых настройках.");
            }
            else
            {
                SetLog("Подсветка выключена.");
            }
        }
        catch (Exception ex)
        {
            SetLog($"Ошибка подсветки: {ex.Message}");
        }
        finally
        {
            SetBusy(false);
            UpdateButtons();
        }
    }

    /// <summary>Состояние «свет выключен» и подпись кнопки.</summary>
    private void SetLightOffState(bool off)
    {
        _lightOff = off;
        OffButton.Content = off ? "Включить" : "Выключить";
    }

    // ── Подсветка в такт гифке ───────────────────────────────────────────────

    private void OnSyncLightChanged(object sender, RoutedEventArgs e)
    {
        if (_loading) return;

        _settings.SyncLight = SyncLightCheck.IsChecked == true;
        _settings.Save();
        UpdateSyncUi();

        if (!_settings.SyncLight)
        {
            StopLightSync();

            // Свои эффекты живут и без синхронизации — просто берут цвет из поля «Цвет».
            if (IsCustomEffect) StartLightSync();

            SetLog("Синхронизация подсветки с гифкой выключена.");
            return;
        }

        SetLightOffState(false);

        // Экран уже может показывать загруженную гифку — запускаем сразу,
        // не заставляя заливать файл повторно.
        StartLightSync();
        SetLog(_lightSyncCts is null
            ? "Нужна гифка на экране."
            : "Подсветка в такт гифке.");
    }

    /// <summary>Журнал подсветки — чтобы видеть, что реально уходит на клавиатуру.</summary>
    private static void LightLog(string text)
    {
        LogFile.Append("light.log", $"{DateTime.Now:HH:mm:ss.fff}  {text}{Environment.NewLine}");
    }

    /// <summary>Пока включена синхронизация, «радуга» недоступна: она подменяет цвет гифки.</summary>
    private void UpdateSyncUi()
    {
        UpdateRainbowUi();

        var sync = SyncLightCheck.IsChecked == true;
        SyncStyleCombo.IsEnabled = sync;
        SyncStyleHint.Visibility = sync ? Visibility.Visible : Visibility.Collapsed;
        SyncStyleHint.Text = Math.Clamp(SyncStyleCombo.SelectedIndex, 0, 2) switch
        {
            GifLight.Picture => "Кадр гифки растянут на клавиши — каждая светит цветом своего участка. Только по кабелю, вместо эффекта.",
            GifLight.Ripple => "Главный цвет кадра расходится от экранчика волной, «Скорость» — как быстро. Только по кабелю.",
            _ => "Выбранный эффект перекрашивается в главный цвет кадра. Работает и по радио."
        };
    }

    private void OnSyncStyleChanged(object sender, SelectionChangedEventArgs e)
    {
        if (_loading || SyncStyleCombo.SelectedIndex < 0) return;

        _settings.SyncStyle = Math.Clamp(SyncStyleCombo.SelectedIndex, 0, 2);
        _settings.Save();
        UpdateSyncUi();

        if (!_settings.SyncLight) return;

        SetLightOffState(false);
        StopLightSync();
        StartLightSync();
        SetLog(_lightSyncCts is null ? "Нужна гифка на экране." : $"Подсветка в такт гифке: {((ComboBoxItem)SyncStyleCombo.SelectedItem).Content}.");
    }

    /// <summary>
    /// Галочка «Радуга» блокируется, когда её нельзя менять: при синхронизации
    /// с гифкой и у эффектов «Разноцветный»/«Спектр» — они в прошивке всегда радужные.
    /// </summary>
    private void UpdateRainbowUi()
    {
        var sync = SyncLightCheck.IsChecked == true;
        var forced = IsRainbowForced;
        var custom = IsCustomEffect;
        var locked = sync || forced || custom;

        RainbowCheck.IsEnabled = !locked;
        RainbowCheck.Opacity = locked ? 0.45 : 1;
        RainbowCheck.ToolTip = forced
            ? "«Разноцветный» и «Спектр» в прошивке всегда радужные — отключить нельзя."
            : sync
                ? "При синхронизации с гифкой цвет берётся из кадров — радуга недоступна."
                : custom && LightEffects.IsRainbowFlow(SelectedMode)
                    ? "Этот эффект всегда радужный."
                : custom
                    ? "Свои эффекты рисуются выбранным цветом — радуга к ним не применяется."
                    : null;
    }

    /// <summary>Эффект в прошивке всегда радужный, флаг «радуга» на него не влияет.</summary>
    private bool IsRainbowForced => SelectedMode is 6 or 8;

    private static (byte R, byte G, byte B) AverageOf((byte R, byte G, byte B)[] colors)
    {
        long r = 0, g = 0, b = 0;

        foreach (var color in colors)
        {
            r += color.R;
            g += color.G;
            b += color.B;
        }

        var n = Math.Max(1, colors.Length);
        return ((byte)(r / n), (byte)(g / n), (byte)(b / n));
    }

    private static void Send(Ak820Device? device, Ak820Dongle? dongle, int mode,
        (byte R, byte G, byte B) color, bool rainbow, int brightness, int speed, int direction,
        bool committed = false)
    {
        if (device is not null)
        {
            if (committed)
                device.SetEffect(mode, $"#{color.R:X2}{color.G:X2}{color.B:X2}",
                    rainbow, brightness, speed, direction);
            else
                device.SetEffectFast(mode, color.R, color.G, color.B, rainbow, brightness, speed, direction);
        }
        else
        {
            dongle!.SendLight(mode, color.R, color.G, color.B, rainbow, brightness, speed, direction);
        }
    }

    /// <summary>
    /// Красит подсветку в средний цвет кадра экрана и меняет его в такт
    /// задержкам гифки. Прошивка проигрывает кадры по кругу — цикл повторяем.
    /// </summary>
    private void StartLightSync()
    {
        StopLightSync();

        // «Картинка» и «Волна от экрана»: гифка рисуется по клавишам живыми кадрами — только по кабелю.
        if (WantsGifKeys)
        {
            if (_device is { } wired)
            {
                StartGifKeysLight(wired, _frames!);
                return;
            }

            if (_dongle is not null && !_gifKeysRadioNoted)
            {
                _gifKeysRadioNoted = true;
                LightLog("gif keys: radio — one colour instead");
                _ = Dispatcher.BeginInvoke(() => SetLog(
                    "Картинка и волна по клавишам работают только по кабелю — по радио подсветка меняет один цвет кадра."));
            }
        }

        // Свои эффекты не зависят от гифки: кадры рисует программа.
        if (IsCustomEffect)
        {
            StartCustomLight(SelectedMode);
            return;
        }

        var frames = _frames;
        var device = _device;
        var dongle = _dongle;

        if (frames is null || frames.Count < 2) return;
        if (device is null && dongle is null) return;
        if (SyncLightCheck.IsChecked != true) return;

        var average = frames.Select(f => GifLight.Dominant(f.Rgb565)).ToArray();
        var colors = Ak820Protocol.VividPalette(average);
        var delays = frames.Select(f => Math.Max(20, f.DelayMs)).ToArray();
        var brightness = Math.Max(0, _settings.Brightness);

        // Эффект берём тот, что выбран в списке: «Прокрутка», «Перекат», «Волна»
        // и т.п. дают бегущий свет по клавишам.
        var mode = SelectedMode;
        if (mode == 0) mode = 1;

        var speed = Math.Clamp(_settings.Speed, 0, 5);
        if (speed == 0) speed = 3; // на нуле бегущие эффекты стоят на месте
        var direction = Math.Clamp(_settings.Direction, 0, 3);

        // Цвет всегда из гифки: с включённой «радугой» прошивка игнорирует
        // байты цвета, и синхронизация превращается в обычную радугу.
        const bool rainbow = false;

        // У бегущих эффектов прошивка перезапускает волну с начала на каждом
        // пакете. Поэтому цвет ставим один раз на цикл гифки: волна успевает
        // пробежать целиком, а перезапуск совпадает с перезапуском анимации.
        var loopColor = AverageOf(colors);
        var loopMs = delays.Sum();

        if (mode is 6 or 8)
        {
            LightLog($"mode {mode} ignores color (firmware rainbow)");
            _ = Dispatcher.BeginInvoke(() => SetLog(
                "«Разноцветный» и «Спектр» в прошивке всегда радужные — цвет гифки к ним не применяется."));
        }

        var cts = new CancellationTokenSource();
        _lightSyncCts = cts;
        var token = cts.Token;

        // Эти массивы зависят только от выбранной гифки. Считаем их один раз
        // до фонового цикла, а не заново на каждом кадре.
        var paletteStarts = new long[colors.Length];
        long paletteTotal = 0;
        for (var i = 0; i < colors.Length; i++)
        {
            paletteStarts[i] = paletteTotal;
            paletteTotal += delays[i];
        }

        var paletteTones = colors
            .Select(c => c.R + c.G + c.B < 24 ? loopColor : c)
            .Select(c => (R: (double)c.R, G: (double)c.G, B: (double)c.B))
            .ToArray();

        // Пока не сказано обратное — это не свой радужный эффект (вентиляторы берут цвета клавиш).
        LightMirror.Custom(0, 0, 0, 1, 0);

        LightLog($"start: frames={frames.Count} mode={mode} speed={speed} dir={direction} " +
                 $"loop=#{loopColor.R:X2}{loopColor.G:X2}{loopColor.B:X2} loopMs={loopMs} " +
                 $"via={(device is not null ? "cable" : "radio")}");

        _ = Task.Run(async () =>
        {
            try
            {
                var next = Environment.TickCount64;

                // Первый пакет шлём «с сохранением»: он гарантированно поднимает
                // подсветку, даже если клавиатура была в режиме «Выключено».
                var first = true;

                // Разовые сбои HID (устройство занято, драйвер моргнул) не должны
                // останавливать подсветку — повторяем пакет несколько раз.
                var failures = 0;

                async Task<bool> SendOnce((byte R, byte G, byte B) color, bool committed)
                {
                    try
                    {
                        token.ThrowIfCancellationRequested();
                        Send(device, dongle, mode, color, rainbow, brightness, speed, direction, committed);
                        failures = 0;
                        return true;
                    }
                    catch (OperationCanceledException)
                    {
                        throw;
                    }
                    catch (ObjectDisposedException)
                    {
                        throw;
                    }
                    catch (Exception ex)
                    {
                        failures++;
                        LightLog($"send failed ({failures}): {ex.Message}");
                        if (failures >= 6)
                            throw new InvalidOperationException("HID не принимает пакеты подсветки", ex);
                        await Task.Delay(60, token);
                        return false;
                    }
                }

                while (!token.IsCancellationRequested)
                {
                    if (mode == 1)
                    {
                        // «Статичный цвет» не анимируется — цвет можно менять сколько угодно часто.
                        // Плавно: за время кадра гифки цвет перетекает в цвет следующего кадра,
                        // поверх — лёгкое сглаживание, чтобы у быстрых гифок не было мельтешения.
                        var tickMs = device is not null ? 40 : 60;
                        var clock = System.Diagnostics.Stopwatch.StartNew();
                        (double R, double G, double B)? shown = null;
                        (byte R, byte G, byte B)? sent = null;
                        var lastMs = 0.0;

                        while (!token.IsCancellationRequested)
                        {
                            var nowMs = clock.Elapsed.TotalMilliseconds;
                            var at = (long)nowMs % Math.Max(1, paletteTotal);
                            var i = Array.BinarySearch(paletteStarts, at);
                            if (i < 0) i = ~i - 1;
                            i = Math.Clamp(i, 0, colors.Length - 1);

                            var k = (at - paletteStarts[i]) / (double)Math.Max(1, delays[i]);
                            k = k * k * (3 - 2 * k);
                            var a0 = paletteTones[i];
                            var a1 = paletteTones[(i + 1) % paletteTones.Length];
                            var target = (R: a0.R + (a1.R - a0.R) * k, G: a0.G + (a1.G - a0.G) * k, B: a0.B + (a1.B - a0.B) * k);

                            // Сглаживание ~120 мс.
                            var alpha = shown is null ? 1 : 1 - Math.Exp(-(nowMs - lastMs) / 120.0);
                            lastMs = nowMs;
                            var s0 = shown ?? target;
                            shown = (s0.R + (target.R - s0.R) * alpha, s0.G + (target.G - s0.G) * alpha, s0.B + (target.B - s0.B) * alpha);

                            var color = ((byte)Math.Round(shown.Value.R), (byte)Math.Round(shown.Value.G), (byte)Math.Round(shown.Value.B));

                            // Тот же цвет повторно не шлём.
                            if (sent is not { } p || Math.Abs(p.R - color.Item1) + Math.Abs(p.G - color.Item2) + Math.Abs(p.B - color.Item3) >= 3 || first)
                            {
                                if (await SendOnce(color, first))
                                {
                                    first = false;
                                    sent = color;
                                }
                            }

                            var wait = tickMs - (int)(clock.Elapsed.TotalMilliseconds - nowMs);
                            if (wait > 0) await Task.Delay(wait, token);
                        }
                    }
                    else
                    {
                        // Бегущий эффект: один пакет на цикл гифки, дальше не мешаем.
                        if (await SendOnce(loopColor, first)) first = false;

                        next = Environment.TickCount64 + loopMs;
                        await Task.Delay(loopMs, token);
                    }
                }
            }
            catch (OperationCanceledException)
            {
                // остановлено вручную
            }
            catch (ObjectDisposedException)
            {
                // устройство переподключили — новая анимация запустится сама
            }
            catch (Exception ex) when (ex.InnerException is ObjectDisposedException)
            {
                // то же, но ошибка завёрнута после нескольких попыток
            }
            catch (Exception ex)
            {
                // Канал отвалился — подсветка останется на последнем цвете.
                // Пробуем восстановиться сами: переоткрываем устройство и снова
                // запускаем синхронизацию, чтобы свет не остался погасшим.
                LightLog($"stop: {ex.Message}");
                _ = Dispatcher.BeginInvoke(new Action(async () =>
                {
                    SetLog($"Синхронизация подсветки прервана: {ex.Message} — переподключаюсь.");
                    var now = Environment.TickCount64;
                    if (now - _lastLightRecover < 10_000) return;
                    _lastLightRecover = now;
                    await ConnectAsync();
                }));
            }
        }, token);
    }

    private void StopLightSync()
    {
        var cts = _lightSyncCts;
        _lightSyncCts = null;

        if (cts is null) return;

        try { cts.Cancel(); } catch { }
        cts.Dispose();
    }

    /// <summary>
    /// Свой эффект: один раз включаем «свой» режим прошивки (20) и дальше
    /// шлём готовые кадры цветов по клавишам. По радио попиксельных цветов
    /// нет — там откатываемся на встроенную «Прокрутку».
    /// </summary>
    private void StartCustomLight(int mode)
    {
        var device = _device;
        var dongle = _dongle;
        if (device is null && dongle is null) return;

        var color = Ak820Protocol.ParseColor(_settings.Color, (255, 255, 255));
        var rainbow = _settings.Rainbow;
        var brightness = Math.Clamp(_settings.Brightness, 0, 5);
        var speed = Math.Clamp(_settings.Speed, 0, 5);
        var direction = Math.Clamp(_settings.Direction, 0, 3);

        // Своя раскраска хранится в прошивке и работает и по кабелю, и по радио — без потока кадров.
        // С анимацией её рисует программа кадрами — это только по кабелю.
        var paintAnim = mode == LightEffects.Paint ? Math.Clamp(_settings.PaintAnim, 0, KeyPaint.Animations.Length - 1) : 0;
        if (mode == LightEffects.Paint && (paintAnim == 0 || device is null))
        {
            _ = ApplyPaintAsync();
            if (paintAnim != 0)
                _ = Dispatcher.BeginInvoke(() => SetLog("Анимация раскраски работает только по кабелю — по радио раскраска без анимации."));
            return;
        }

        if (mode == LightEffects.Paint) _paintSnapshot = new Dictionary<string, string>(PaintMap);

        if (device is null)
        {
            // По радио — ближайший встроенный: спектр → «Спектр», волна от центра, течение, вращение — такие же
            // прошивки (радужные), остальные — «Прокрутка».
            var radioMode = mode switch
            {
                LightEffects.Spectrum => 8,
                LightEffects.CenterWave => 9,
                LightEffects.Flow => 16,
                LightEffects.Rotate => 12,
                _ => 10
            };
            var radioRainbow = rainbow || LightEffects.IsRainbowFlow(mode);
            var radioName = Ak820Protocol.Effects.FirstOrDefault(e => e.Mode == radioMode).Name ?? "Прокрутка";
            _ = Task.Run(() =>
            {
                try { dongle!.SendLight(radioMode, color.R, color.G, color.B, radioRainbow, brightness, speed, direction); }
                catch (Exception ex) { LightLog($"custom radio failed: {ex.Message}"); }
            });
            LightLog($"custom {mode}: radio fallback mode={radioMode}");
            _ = Dispatcher.BeginInvoke(() => SetLog(
                "Свои эффекты рисуются по клавишам и работают только по кабелю. " +
                $"По радио включён похожий эффект прошивки «{radioName}» — подключите провод для своих эффектов."));
            return;
        }

        // «Гифка → справа налево»: по клавишам течёт правый край кадров гифки.
        (byte R, byte G, byte B)[][]? strips = null;
        int[]? flowCumulative = null;
        var flowLoopMs = 0;

        if (mode == LightEffects.GifFlow)
        {
            var gif = _frames;
            if (gif is null || gif.Count == 0)
            {
                _ = Dispatcher.BeginInvoke(() => SetLog(
                    "Для «Гифка → справа налево» выберите гифку на вкладке «Экран» и отправьте её — подсветка продолжит её."));
                return;
            }

            strips = gif.Select(f => Ak820Protocol.StripColors(f.Rgb565, 6)).ToArray();
            flowCumulative = new int[gif.Count];

            var acc = 0;
            for (var i = 0; i < gif.Count; i++)
            {
                acc += Math.Max(20, gif[i].DelayMs);
                flowCumulative[i] = acc;
            }

            flowLoopMs = acc;
        }

        // Если включена синхронизация — берём цвет текущего кадра гифки,
        // волна при этом продолжает бежать по клавишам.
        (byte R, byte G, byte B)[]? palette = null;
        int[]? cumulative = null;
        var loopMs = 0;

        if (_settings.SyncLight && _frames is { Count: >= 2 } frames)
        {
            var average = frames.Select(f => GifLight.Dominant(f.Rgb565)).ToArray();
            palette = Ak820Protocol.VividPalette(average);

            cumulative = new int[palette.Length];
            var sum = 0;
            for (var i = 0; i < palette.Length; i++)
            {
                sum += Math.Max(20, frames[i].DelayMs);
                cumulative[i] = sum;
            }

            loopMs = sum;
        }

        LightLog($"custom start: mode={mode} bright={brightness} speed={speed} dir={direction} " +
                 $"palette={(palette is null ? "none" : palette.Length.ToString())}");

        (byte R, byte G, byte B) PaletteColor(double seconds)
        {
            if (palette is null || loopMs <= 0 || cumulative is null) return color;

            var at = (int)((long)(seconds * 1000) % loopMs);
            var index = Array.FindIndex(cumulative, v => at < v);
            return index >= 0 && index < palette.Length ? palette[index] : color;
        }

        // Что клавиатура покажет сама, когда кадры перестанут идти (программу закрыли, ПК уснул):
        // для раскраски — она же без анимации, для своих эффектов — статичный цвет эффекта.
        Action<Ak820Device> fallback;
        if (mode == LightEffects.Paint)
        {
            var snapshot = _paintSnapshot;
            fallback = d => d.EnsureUserLight(KeyPaint.BuildFrame(snapshot, brightness));
        }
        else
        {
            var still = palette is { Length: > 0 } ? AverageOf(palette) : color;
            fallback = d => d.EnsureEffect(1, $"#{still.R:X2}{still.G:X2}{still.B:X2}", false, brightness, speed, direction);
        }

        var scale = brightness / 5.0;
        StartLiveStream(device, $"custom {mode}", fallback, seconds =>
        {
            if (mode == LightEffects.Paint)
                return KeyPaint.Animate(_paintSnapshot, brightness, paintAnim, seconds, Math.Clamp(_settings.PaintSpeed, 0, 5));

            var bank = strips is not null && flowCumulative is not null
                ? LightEffects.BuildFlow(strips, flowCumulative, flowLoopMs, seconds, speed)
                : LightEffects.Build(mode, seconds, speed, direction, PaletteColor(seconds));

            // Вентиляторы «как клавиатура» посчитают этот же эффект у себя.
            LightMirror.Custom(mode, speed, direction, scale, seconds);

            return LightEffects.Dim(bank, scale);
        });
    }

    /// <summary>
    /// «Картинка гифки на клавишах» и «Волна от экрана»: гифка с экрана рисуется по клавишам живыми кадрами.
    /// </summary>
    private void StartGifKeysLight(Ak820Device device, IReadOnlyList<ScreenFrame> frames)
    {
        var style = Math.Clamp(_settings.SyncStyle, 0, 2);
        GifLight light;

        try
        {
            light = new GifLight(frames, style);
        }
        catch (Exception ex)
        {
            LightLog($"gif keys failed: {ex.Message}");
            return;
        }

        var brightness = Math.Clamp(_settings.Brightness, 0, 5);
        var speed = Math.Clamp(_settings.Speed, 0, 5);
        var still = light.FallbackHex;

        LightLog($"gif keys start: style={style} frames={light.FrameCount} loopMs={light.LoopMs} still={still}");

        StartLiveStream(device, "gif keys",
            d => d.EnsureEffect(1, still, false, brightness, speed, 0),
            seconds => light.Build(seconds, brightness, speed));
    }

    /// <summary>Нужна подсветка «гифкой по клавишам»: синхронизация включена, стиль «Картинка» или «Волна», гифка есть,
    /// а эффект — не раскраска и не «Гифка → справа налево» (у них своя логика).</summary>
    private bool WantsGifKeys =>
        _settings.SyncLight
        && _settings.SyncStyle is GifLight.Picture or GifLight.Ripple
        && _frames is { Count: > 0 }
        && SelectedMode is not (LightEffects.Paint or LightEffects.GifFlow);

    private bool _gifKeysRadioNoted;

    /// <summary>
    /// Поток живых кадров подсветки по кабелю: <paramref name="build"/> рисует таблицу цветов на момент t (секунды),
    /// кадр уходит командой 0x20 — как «Музыка» в официальной программе — примерно 20 раз в секунду.
    /// Перед стартом <paramref name="fallback"/> один раз сохраняет в клавиатуру похожий обычный эффект.
    /// </summary>
    private void StartLiveStream(Ak820Device device, string name, Action<Ak820Device>? fallback, Func<double, byte[]> build)
    {
        var cts = new CancellationTokenSource();
        _lightSyncCts = cts;
        var token = cts.Token;

        _ = Task.Run(async () =>
        {
            TimerResolution.Begin();
            try
            {
                if (fallback is not null)
                {
                    try { fallback(device); }
                    catch (Exception ex) when (ex is not ObjectDisposedException)
                    {
                        LightLog($"{name}: fallback failed: {ex.Message}");
                    }
                }

                var clock = System.Diagnostics.Stopwatch.StartNew();
                var failures = 0;
                var lastStatus = int.MinValue;
                long sent = 0;

                while (!token.IsCancellationRequested)
                {
                    var started = clock.ElapsedMilliseconds;
                    var live = Ak820Protocol.LiveFrame(build(clock.Elapsed.TotalSeconds));

                    try
                    {
                        device.SendLiveFrame(live, token);
                        failures = 0;
                        sent++;

                        // В журнал — только смена ответа клавиатуры (1 — кадр принят).
                        if (device.LastLiveStatus != lastStatus)
                        {
                            lastStatus = device.LastLiveStatus;
                            LightLog($"{name}: live status={lastStatus} (frame {sent})");
                        }
                    }
                    catch (OperationCanceledException)
                    {
                        throw;
                    }
                    catch (ObjectDisposedException)
                    {
                        throw;
                    }
                    catch (Exception ex)
                    {
                        failures++;
                        LightLog($"{name}: send failed ({failures}): {ex.Message}");
                        if (failures >= 6)
                            throw new InvalidOperationException("HID не принимает кадры подсветки", ex);

                        await Task.Delay(80, token);
                        continue;
                    }

                    var wait = LightEffects.FrameIntervalMs - (int)(clock.ElapsedMilliseconds - started);
                    if (wait > 0) await Task.Delay(wait, token);
                }
            }
            catch (OperationCanceledException)
            {
                // остановлено вручную
            }
            catch (ObjectDisposedException)
            {
                // устройство переподключили — новая анимация запустится сама
            }
            catch (Exception ex) when (ex.InnerException is ObjectDisposedException)
            {
                // то же, но ошибка завёрнута после нескольких попыток
            }
            catch (Exception ex)
            {
                LightLog($"{name} stop: {ex.Message}");
                _ = Dispatcher.BeginInvoke(new Action(async () =>
                {
                    SetLog($"Подсветка прервана: {ex.Message} — переподключаюсь.");
                    var now = Environment.TickCount64;
                    if (now - _lastLightRecover < 10_000) return;
                    _lastLightRecover = now;
                    await ConnectAsync();
                }));
            }
            finally
            {
                TimerResolution.End();
                if (ReferenceEquals(_lightSyncCts, cts)) _lightSyncCts = null;
            }
        }, token);
    }

    /// <summary>
    /// Сторож подсветки: если синхронизация включена, но почему-то не идёт
    /// (сбой HID, смена канала), сама запускает её заново.
    /// </summary>
    private void StartLightWatch()
    {
        if (_lightTimer is not null) return;

        _lightTimer = new System.Windows.Threading.DispatcherTimer
        {
            Interval = TimeSpan.FromSeconds(3)
        };
        _lightTimer.Tick += (_, _) =>
        {
            if (_loading || _busy || _lightBusy || _lightOff) return;

            // Раскраска без анимации хранится в прошивке — перезапускать нечего.
            if (SelectedMode == LightEffects.Paint && (_settings.PaintAnim == 0 || _device is null)) return;

            // Гифка по клавишам: хватает и одной картинки.
            if (WantsGifKeys && _device is not null)
            {
                if (_lightSyncCts is { IsCancellationRequested: false }) return;
                LightLog("watch: gif keys not running — restarting");
                StartLightSync();
                return;
            }

            var custom = IsCustomEffect;
            if (!_settings.SyncLight && !custom) return;
            if (!custom && (_frames is null || _frames.Count < 2)) return;
            if (custom && _device is null) return;
            // «Гифка → справа налево» без гифки не запустится — не дёргаем её каждые 3 секунды.
            if (custom && SelectedMode == LightEffects.GifFlow && (_frames is null || _frames.Count == 0)) return;
            if (_device is null && _dongle is null) return;

            // Уже работает — не мешаем.
            if (_lightSyncCts is { IsCancellationRequested: false }) return;

            LightLog("watch: sync not running — restarting");
            StartLightSync();
        };
        _lightTimer.Start();
    }

    private void StopLightWatch()
    {
        _lightTimer?.Stop();
        _lightTimer = null;
    }

    private void OnRainbowChanged(object sender, RoutedEventArgs e)
    {
        if (_loading) return;
        _rainbowAuto = false;
        CollectSettings();
        _settings.Save();
        ScheduleLightApply();
    }

    /// <summary>
    /// Живое применение подсветки: любое изменение настроек уходит на клавиатуру
    /// само, без кнопки «Применить». Пауза 300 мс склеивает быстрые правки.
    /// </summary>
    private void ScheduleLightApply()
    {
        if (_loading) return;

        // Любая правка настроек сама включает свет, если он был выключен кнопкой.
        if (_lightOff) SetLightOffState(false);

        _autoApplyTimer ??= new System.Windows.Threading.DispatcherTimer
        {
            Interval = TimeSpan.FromMilliseconds(300)
        };

        _autoApplyTimer.Stop();
        _autoApplyTimer.Start();
    }

    private async Task ApplyLightingAsync()
    {
        if (_lightBusy) return;
        if (_lightOff) return;

        var device = _device;
        var dongle = _dongle;
        if (device is null && dongle is null) return;

        if (_settings.SyncLight || IsCustomEffect)
        {
            StartLightSync();
            if (_lightSyncCts is not null) return;

            // Свой эффект по радио уже заменён встроенным — второй раз не шлём.
            if (IsCustomEffect) return;
        }

        var mode = Ak820Protocol.Effects[Math.Clamp(_settings.Effect, 0,
            Ak820Protocol.Effects.Length - 1)].Mode;
        var rainbow = _settings.Rainbow;
        var brightness = Math.Clamp(_settings.Brightness, 0, 5);
        var speed = Math.Clamp(_settings.Speed, 0, 5);
        var direction = Math.Clamp(_settings.Direction, 0, 3);
        var color = _settings.Color;

        LightLog($"apply: sync off, mode={mode} color={color} rainbow={rainbow} " +
                 $"bright={brightness} speed={speed} dir={direction} " +
                 $"via={(device is not null ? "cable" : "radio")}");

        _lightBusy = true;
        try
        {
            await Task.Run(() =>
            {
                if (device is not null)
                {
                    device.SetEffect(mode, color, rainbow, brightness, speed, direction);
                }
                else
                {
                    var (r, g, b) = Ak820Protocol.ParseColor(color, (255, 255, 255));
                    dongle!.SendLight(mode, r, g, b, rainbow, brightness, speed, direction);
                }
            });
        }
        catch (Exception ex)
        {
            SetLog($"Ошибка подсветки: {ex.Message}");
        }
        finally
        {
            _lightBusy = false;
        }
    }

    private void OnColorTextChanged(object sender, TextChangedEventArgs e)
    {
        UpdateColorSwatch();
        if (_loading) return;

        CollectSettings();
        ScheduleLightApply();
    }

    private ColorWheel? _lightWheel;
    private bool _lightColorFromWheel;

    /// <summary>Цвет подсветки — как кисть в «Своей раскраске»: круг, палитра и поле #RRGGBB.</summary>
    private void SetupLightColor()
    {
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
            swatch.MouseLeftButtonUp += (_, _) => ColorBox.Text = hex;
            LightSwatches.Children.Add(swatch);
        }

        _lightWheel = new ColorWheel(140);
        _lightWheel.ColorChanged += color =>
        {
            _lightColorFromWheel = true;
            ColorBox.Text = ColorWheel.Hex(color);
            _lightColorFromWheel = false;
        };
        LightWheelHost.Content = _lightWheel;

        UpdateColorSwatch();
    }

    private void UpdateColorSwatch()
    {
        var (r, g, b) = Ak820Protocol.ParseColor(ColorBox.Text, (255, 255, 255));
        var color = Color.FromRgb(r, g, b);
        ColorSwatch.Background = new SolidColorBrush(color);
        if (!_lightColorFromWheel) _lightWheel?.SetColor(color);

        var hex = $"#{r:X2}{g:X2}{b:X2}";
        foreach (Border swatch in LightSwatches.Children)
        {
            swatch.SetResourceReference(Border.BorderBrushProperty,
                string.Equals(swatch.Tag as string, hex, StringComparison.OrdinalIgnoreCase) ? "AccentDeepBrush" : "LineBrush");
        }
    }

    private void OnBrightnessChanged(object sender, RoutedPropertyChangedEventArgs<double> e)
    {
        UpdateBrightnessLabel();
        if (_loading) return;

        CollectSettings();
        ScheduleLightApply();
    }

    private void UpdateBrightnessLabel() =>
        BrightnessLabel.Text = $"Яркость: {(int)Math.Round(BrightnessSlider.Value)}";

    private void OnSpeedChanged(object sender, RoutedPropertyChangedEventArgs<double> e)
    {
        UpdateSpeedLabel();
        if (_loading) return;

        CollectSettings();
        ScheduleLightApply();
    }

    private void UpdateSpeedLabel() =>
        SpeedLabel.Text = $"Скорость: {(int)Math.Round(SpeedSlider.Value)}";

    private bool _rainbowAuto;
    private CancellationTokenSource? _demoCts;

    /// <summary>
    /// Перебирает эффекты с плавной волной по 4 секунды, чтобы выбрать нужный.
    /// Повторное нажатие останавливает перебор.
    /// </summary>
    private async void OnEffectDemo(object sender, RoutedEventArgs e)
    {
        if (_demoCts is not null)
        {
            _demoCts.Cancel();
            return;
        }

        if (_device is null && _dongle is null) return;

        var cts = new CancellationTokenSource();
        _demoCts = cts;

        var candidates = new[] { 9, 16, 10, 18, 12, 15, 17, 19 };

        try
        {
            foreach (var index in candidates)
            {
                if (cts.IsCancellationRequested) break;

                EffectCombo.SelectedIndex = index;
                SetLog($"Проверка: «{Ak820Protocol.Effects[index].Name}» — 4 секунды. " +
                       "Нажмите «Подобрать» ещё раз, чтобы остановить.");
                LightLog($"demo: {index} {Ak820Protocol.Effects[index].Name}");

                await Task.Delay(4000, cts.Token);
            }

            if (!cts.IsCancellationRequested)
                SetLog("Проверка закончена. Выберите эффект, который понравился.");
        }
        catch (OperationCanceledException)
        {
            SetLog("Проверка остановлена.");
        }
        finally
        {
            if (ReferenceEquals(_demoCts, cts)) _demoCts = null;
            cts.Dispose();
        }
    }

    private void OnEffectChanged(object sender, SelectionChangedEventArgs e)
    {
        if (_loading) return;

        var mode = Ak820Protocol.Effects[Math.Clamp(EffectCombo.SelectedIndex, 0,
            Ak820Protocol.Effects.Length - 1)].Mode;

        // «Разноцветный» и «Спектр» всегда радужные — показываем это галочкой
        // и блокируем её, чтобы нельзя было снять.
        if (mode is 6 or 8)
        {
            RainbowCheck.IsChecked = true;
            _rainbowAuto = true;
        }
        else if (_rainbowAuto)
        {
            RainbowCheck.IsChecked = false;
            _rainbowAuto = false;
        }

        UpdateRainbowUi();

        CollectSettings();
        _settings.Save();
        ScheduleLightApply();
    }

    private void OnDirectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (_loading) return;

        CollectSettings();
        _settings.Save();
        ScheduleLightApply();
    }

    // ── Перетаскивание файла ─────────────────────────────────────────────────

    private void OnDragOver(object sender, DragEventArgs e)
    {
        e.Effects = e.Data.GetDataPresent(DataFormats.FileDrop)
            ? DragDropEffects.Copy
            : DragDropEffects.None;
        e.Handled = true;
    }

    private void OnDropFile(object sender, DragEventArgs e)
    {
        if (e.Data.GetData(DataFormats.FileDrop) is not string[] { Length: > 0 } files) return;

        if (PagePump.IsVisible)
            HandleCoolerDrop(files[0]);
        else
            LoadImage(files[0], quiet: false);
    }

    // ── Мелочи ───────────────────────────────────────────────────────────────

    private void SetStatus(string text, string hint, bool connected)
    {
        const string prefix = "Клавиатура ";
        var shortText = text.StartsWith(prefix, StringComparison.Ordinal) ? text[prefix.Length..] : text;
        StatusText.Text = shortText.Length > 0 ? char.ToUpper(shortText[0]) + shortText[1..] : shortText;
        StatusHint.Text = hint;
        StatusHint.ToolTip = null;
        StatusDot.SetResourceReference(System.Windows.Shapes.Shape.FillProperty, connected ? "GoodBrush" : "BadBrush");
    }


    private void SetLog(string text) => LogText.Text = text;

    private void SetBusy(bool busy)
    {
        _busy = busy;

        PickButton.IsEnabled = !busy;
        ReconnectButton.IsEnabled = !busy;
        FitCombo.IsEnabled = !busy;
        MaxFramesCombo.IsEnabled = !busy;
    }

    private void UpdateButtons()
    {
        var ready = _device is not null && !_busy;
        var lightReady = (_device is not null || _dongle is not null) && !_busy;

        UploadButton.IsEnabled = ready && _frames is not null;
        TimeButton.IsEnabled = lightReady;

        ApplyButton.IsEnabled = lightReady;
        PaintApplyButton.IsEnabled = lightReady;
        OffButton.IsEnabled = lightReady;
        DemoButton.IsEnabled = lightReady;
    }
}

