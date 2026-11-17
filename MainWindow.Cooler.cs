using System.IO;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using Rectangle = System.Windows.Shapes.Rectangle;
using Shape = System.Windows.Shapes.Shape;
using System.Windows.Threading;
using Microsoft.Win32;

namespace RigControl;

/// <summary>Страница «Помпа Thermalright»: редактор экрана, фон, вывод на помпу, автовывод.</summary>
public partial class MainWindow
{
    private static readonly string[] FontNames =
    {
        AppFonts.Kawaii, "Segoe UI", "Segoe UI Light", "Bahnschrift", "Consolas", "Arial", "Verdana", "Trebuchet MS", "Georgia", "Impact"
    };

    private static readonly string[] SwatchColors =
    {
        "#FFFFFF", "#8A90A0", "#4DA3FF", "#5FD38D", "#FFC14D", "#FF8A4D", "#FF5C5C", "#C084FF", "#4DE0E0", "#FF6FB5",
        "#FFC2DA", "#B9A7FF", "#9FF5D8", "#FFE58A"
    };

    private CoolerLcd? _cooler;
    private Telemetry? _telemetry;
    private Snapshot _snapshot;
    private CancellationTokenSource? _coolerCts;
    private Task? _coolerTask;
    private List<LcdFrame>? _coolerFrames;
    private string? _coolerImagePath;
    private bool _coolerLoading = true;
    private bool _coolerActive;
    private bool _coolerBusy;
    private bool _userStopped;

    // Редактор экрана
    private List<LcdItem> _layout = new();
    private LcdItem? _selected;                                   // главный выделенный объект (его свойства на панели)
    private readonly List<LcdItem> _multi = new();                // все выделенные (рамкой, с Ctrl)
    private readonly List<Rectangle> _multiRects = new();         // рамки остальных выделенных
    private bool _marquee;
    private Point _marqueeStart;
    private List<LcdItem> _marqueeBase = new();
    private Point _dragStart;
    private Dictionary<LcdItem, Point> _dragOrigins = new();
    private Dictionary<LcdItem, FrameworkElement> _elements = new();
    private BitmapSource? _background;
    private List<(byte[] Jpeg, int DelayMs)>? _bgFrames;   // фон-гифка: кадры в JPEG (в памяти в десятки раз меньше)
    private long _bgTotalMs;
    private int _bgCachedIndex = -1;                        // какой кадр фона-гифки сейчас распакован
    private WriteableBitmap? _bgFrameBitmap;                // в эту картинку распаковывается текущий кадр
    private byte[]? _bgPixels;
    private int _bgLoadedLong, _bgLoadedShort;              // под какой размер сцены уменьшен фон
    private long _telemetryIdleSince;
    private Image? _bgPreviewImage;
    private DispatcherTimer? _bgAnimTimer;
    private string? _backgroundPath;
    private string? _bgTop;
    private string? _bgBottom;
    private bool _dragging;
    private bool _propLoading;

    private DispatcherTimer? _tick;
    private DispatcherTimer? _watchdog;
    private DispatcherTimer? _saveTimer;
    private bool _tickBusy;
    private bool _scenePortrait;
    private string? _previewSignature;
    private string? _frameSignature;
    private BitmapSource? _lastFrame;
    private int _sentInWindow;
    private int _lastBgIndex = -1;       // кадр фона-гифки на прошлой отрисовке

    private bool _coolerWanted;          // экран запущен вручную (сработает и при выключенном автовыводе)
    private long _watchFrames;           // счётчик кадров на прошлой проверке сторожа
    private int _stallChecks;            // сколько проверок подряд кадры не идут
    private long _lastUsbReset;          // когда последний раз сбрасывали USB экрана
    private long _pumpFailureSince;      // начало непрерывной серии сбоев передачи/подключения
    private long _lastContentUpdate;     // когда последний раз перерисовывали содержимое
    private long _lastReArm;             // когда последний раз повторяли рукопожатие
    private long _reArmBlockedUntil;     // помпа не отвечает — не трогаем её до этого момента
    private int _pumpLogTicks;           // раз в минуту пишем в журнал, сколько кадров ушло
    private long _pumpLogFrames;         // счётчик кадров на прошлую запись в журнал

    private bool CoolerRunning => _coolerActive;
    private bool CoolerImageMode => CoolerModeImage.IsChecked == true;

    /// <summary>Экран должен работать: автовывод или ручной запуск, и пользователь его не останавливал.</summary>
    private bool ScreenWanted => !_userStopped && (_settings.AutoOutput || _coolerWanted);

    // ── Подготовка интерфейса ────────────────────────────────────────────────

    /// <summary>Однократная настройка элементов, которые удобнее собрать кодом.</summary>
    private void SetupPumpUi()
    {
        PropFont.ItemsSource = FontNames;

        foreach (var hex in SwatchColors)
        {
            var color = (Color)ColorConverter.ConvertFromString(hex);
            var swatch = new Border
            {
                Width = 26,
                Height = 26,
                CornerRadius = new CornerRadius(6),
                Margin = new Thickness(0, 0, 8, 8),
                Background = new SolidColorBrush(color),
                BorderThickness = new Thickness(1),
                Cursor = Cursors.Hand,
                Tag = hex
            };
            swatch.SetResourceReference(Border.BorderBrushProperty, "LineBrush");
            swatch.MouseLeftButtonUp += OnSwatchClick;
            PropSwatches.Children.Add(swatch);
        }

        foreach (var theme in LcdThemes.All)
        {
            var top = (Color)ColorConverter.ConvertFromString(theme.Top);
            var bottom = (Color)ColorConverter.ConvertFromString(theme.Bottom);
            var light = 0.299 * top.R + 0.587 * top.G + 0.114 * top.B > 150;

            Brush background = new LinearGradientBrush(top, bottom, 90);
            if (theme.Image is not null)
            {
                try
                {
                    var art = new BitmapImage();
                    art.BeginInit();
                    art.UriSource = new Uri(theme.Image);
                    art.DecodePixelWidth = 160;
                    art.CacheOption = BitmapCacheOption.OnLoad;
                    art.EndInit();
                    art.Freeze();
                    background = new ImageBrush(art) { Stretch = Stretch.UniformToFill };
                }
                catch (Exception ex)
                {
                    // нет картинки — останется градиент
                    QuietLog.Write(ex);
                }
            }

            var label = new TextBlock
            {
                Text = theme.Name,
                Foreground = light ? new SolidColorBrush(Color.FromRgb(0x2B, 0x2B, 0x2B)) : Brushes.White,
                FontWeight = FontWeights.SemiBold
            };

            var tile = new Border
            {
                Height = 44,
                Margin = new Thickness(4),
                CornerRadius = new CornerRadius(6),
                BorderThickness = new Thickness(2),
                Background = background,
                Cursor = Cursors.Hand,
                Tag = theme,
                // На картинке подпись — на подложке, чтобы читалась
                Child = theme.Image is null
                    ? label
                    : new Border
                    {
                        Child = label,
                        Padding = new Thickness(8, 1, 8, 2),
                        CornerRadius = new CornerRadius(7),
                        Background = new SolidColorBrush(light ? Color.FromArgb(0xC8, 0xFF, 0xFF, 0xFF) : Color.FromArgb(0xB0, 0, 0, 0)),
                        VerticalAlignment = VerticalAlignment.Center,
                        HorizontalAlignment = HorizontalAlignment.Center
                    }
            };
            label.VerticalAlignment = VerticalAlignment.Center;
            label.HorizontalAlignment = HorizontalAlignment.Center;

            tile.SetResourceReference(Border.BorderBrushProperty, "LineBrush");
            tile.MouseEnter += (_, _) => tile.SetResourceReference(Border.BorderBrushProperty, "AccentBrush");
            tile.MouseLeave += (_, _) => tile.SetResourceReference(Border.BorderBrushProperty, "LineBrush");
            tile.MouseLeftButtonUp += OnThemeClick;
            ThemePanel.Children.Add(tile);
        }

        _tick = new DispatcherTimer { Interval = TimeSpan.FromSeconds(1) };
        _tick.Tick += OnTick;

        _watchdog = new DispatcherTimer { Interval = TimeSpan.FromSeconds(5) };
        _watchdog.Tick += OnWatchdog;

        // После сна и после возврата окна из трея помпа могла остаться на своём логотипе:
        // повторяем рукопожатие, чтобы она снова принимала кадры.
        StateChanged += (_, _) =>
        {
            if (WindowState == WindowState.Normal) _ = ReArmOnWakeAsync();
        };
        SystemEvents.PowerModeChanged += OnPumpPowerMode;

        _saveTimer = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(700) };
        _saveTimer.Tick += (_, _) =>
        {
            _saveTimer?.Stop();
            CollectCoolerSettings();
            _settings.Save();
        };
    }

    private async Task InitCoolerAsync()
    {
        _coolerLoading = true;

        CoolerModeImage.IsChecked = _settings.CoolerMode == 1;
        CoolerModeMonitor.IsChecked = _settings.CoolerMode != 1;
        SelectByTag(CoolerRotationCombo, _settings.CoolerRotation.ToString());
        CoolerSizeCombo.SelectedIndex = Math.Clamp(_settings.CoolerSize, 0, CoolerSizeCombo.Items.Count - 1);
        BgBrightness.Value = Math.Clamp(_settings.CoolerBrightness, 0, 100);
        FpsSlider.Value = Math.Clamp(_settings.CoolerFps, 1, 10);
        UpdateBrightnessText();
        UpdateFpsLabel();

        _bgTop = _settings.CoolerBgTop;
        _bgBottom = _settings.CoolerBgBottom;
        UpdateBgSwatches();
        {
            var (sw, sh) = SceneSize();
            _scenePortrait = sh > sw;
        }

        _layout = _settings.CoolerLayout is { Count: > 0 } saved ? saved : LcdItem.DefaultLayout();
        _settings.CoolerLayout = _layout;

        if (!string.IsNullOrWhiteSpace(_settings.CoolerBackground)
            && (LcdRender.IsResource(_settings.CoolerBackground) || File.Exists(_settings.CoolerBackground)))
            LoadBackground(_settings.CoolerBackground!);

        ApplyModeVisibility();

        _coolerLoading = false;

        RebuildScene();
        UpdateProps();

        UpdateTelemetryTimerState();
        _tick?.Start();
        _watchdog?.Start();

        await CoolerConnectAsync();

        if (CoolerImageMode && !string.IsNullOrWhiteSpace(_settings.CoolerImage) && File.Exists(_settings.CoolerImage))
            LoadCoolerImage(_settings.CoolerImage!);

        // Автовывод: сразу после запуска программы выводим картинку на помпу.
        if (_settings.AutoOutput && _cooler is not null && (!CoolerImageMode || _coolerFrames is not null))
            StartCooler();
    }

    private static void SelectByTag(ComboBox combo, string tag)
    {
        foreach (ComboBoxItem item in combo.Items)
        {
            if (item.Tag is string t && t == tag)
            {
                combo.SelectedItem = item;
                return;
            }
        }
    }

    private void CollectCoolerSettings()
    {
        _settings.CoolerMode = CoolerImageMode ? 1 : 0;
        _settings.CoolerImage = _coolerImagePath;
        _settings.CoolerRotation = CoolerRotation();
        _settings.CoolerSize = Math.Max(0, CoolerSizeCombo.SelectedIndex);
        _settings.CoolerLayout = _layout;
        _settings.CoolerBackground = _backgroundPath;
        _settings.CoolerBgTop = _bgTop;
        _settings.CoolerBgBottom = _bgBottom;
        _settings.CoolerBrightness = (int)Math.Round(BgBrightness.Value);
        _settings.CoolerFps = (int)Math.Round(FpsSlider.Value);
    }

    private void SaveSoon()
    {
        _saveTimer?.Stop();
        _saveTimer?.Start();
    }

    private int CoolerRotation() =>
        CoolerRotationCombo.SelectedItem is ComboBoxItem { Tag: string tag } && int.TryParse(tag, out var degrees)
            ? degrees
            : 0;

    private double Brightness() => Math.Clamp(BgBrightness.Value / 100.0, 0, 1);

    /// <summary>Размер панели: выбранный вручную или полученный от помпы.</summary>
    private (int Width, int Height) CoolerPanelSize()
    {
        if (CoolerSizeCombo.SelectedItem is ComboBoxItem { Tag: string tag })
        {
            var parts = tag.Split('x');
            if (parts.Length == 2 && int.TryParse(parts[0], out var w) && int.TryParse(parts[1], out var h) && w > 0 && h > 0)
                return (w, h);
        }

        return _cooler?.Resolution ?? (320, 240);
    }

    /// <summary>Размер сцены до поворота: при 90°/270° рисуем «боком», потом поворачиваем под панель.</summary>
    private (int Width, int Height) SceneSize()
    {
        var (w, h) = CoolerPanelSize();
        return CoolerRotation() is 90 or 270 ? (h, w) : (w, h);
    }

    private void ApplyModeVisibility()
    {
        var image = CoolerImageMode;

        SceneViewbox.Visibility = image ? Visibility.Collapsed : Visibility.Visible;
        CoolerPreview.Visibility = image ? Visibility.Visible : Visibility.Collapsed;
        EditorToolbar.Visibility = image ? Visibility.Collapsed : Visibility.Visible;
        ImageToolbar.Visibility = image ? Visibility.Visible : Visibility.Collapsed;
        PropCard.Visibility = image ? Visibility.Collapsed : Visibility.Visible;
        ThemeCard.Visibility = image ? Visibility.Collapsed : Visibility.Visible;
        BgCard.Visibility = image ? Visibility.Collapsed : Visibility.Visible;
    }

    // ── Редактор: сцена ──────────────────────────────────────────────────────

    private void RebuildScene()
    {
        if (_coolerLoading) return;

        EnsureBackgroundSize();

        var (w, h) = SceneSize();
        _elements = LcdScene.Build(SceneCanvas, w, h, _layout, _snapshot, DateTime.Now, CurrentBackground(), Brightness(),
            _bgTop, _bgBottom);
        _bgPreviewImage = SceneCanvas.Children.Count > 0 && SceneCanvas.Children[0] is Grid layer
            ? layer.Children.OfType<Image>().FirstOrDefault()
            : null;
        SceneCanvas.UpdateLayout();
        UpdateSelectionBox();
        _previewSignature = SceneSignature();
    }

    /// <summary>Всё, от чего зависит картинка сцены. Не изменилось — не перерисовываем.</summary>
    private string SceneSignature()
    {
        var (w, h) = SceneSize();
        return $"{w}x{h}|{CoolerRotation()}|{_backgroundPath}|{_background?.GetHashCode()}|{_bgTop}|{_bgBottom}|{Brightness():0.00}|" +
               LcdScene.Signature(_layout, _snapshot, DateTime.Now);
    }

    private void ClearSelectionState()
    {
        _selected = null;
        _multi.Clear();
    }

    private void UpdateSelectionBox()
    {
        // Объекты, которых больше нет (удалили, сменили тему), из выделения выкидываем.
        _multi.RemoveAll(i => !_layout.Contains(i));
        if (_selected is not null && !_layout.Contains(_selected)) _selected = _multi.LastOrDefault();

        static void Frame(Rectangle rect, FrameworkElement block)
        {
            Canvas.SetLeft(rect, Canvas.GetLeft(block) - 2);
            Canvas.SetTop(rect, Canvas.GetTop(block) - 2);
            rect.Width = Math.Max(4, block.ActualWidth + 4);
            rect.Height = Math.Max(4, block.ActualHeight + 4);
            rect.Visibility = Visibility.Visible;
        }

        if (_selected is not null && _elements.TryGetValue(_selected, out var main)) Frame(SelectionRect, main);
        else SelectionRect.Visibility = Visibility.Collapsed;

        // Рамки остальных выделенных: тоньше и бледнее главной. Прямоугольники переиспользуем.
        var others = _multi.Where(i => !ReferenceEquals(i, _selected) && _elements.ContainsKey(i)).ToList();
        while (_multiRects.Count < others.Count)
        {
            var rect = new Rectangle { StrokeThickness = 1, StrokeDashArray = new DoubleCollection { 2, 2 }, Opacity = 0.75 };
            rect.SetResourceReference(Shape.StrokeProperty, "AccentBrush");
            OverlayCanvas.Children.Add(rect);
            _multiRects.Add(rect);
        }

        for (var i = 0; i < _multiRects.Count; i++)
        {
            if (i < others.Count) Frame(_multiRects[i], _elements[others[i]]);
            else _multiRects[i].Visibility = Visibility.Collapsed;
        }
    }

    private static Rect BoundsOf(FrameworkElement block) =>
        new(Canvas.GetLeft(block), Canvas.GetTop(block), block.ActualWidth, block.ActualHeight);

    private LcdItem? HitItem(Point point)
    {
        for (var i = _layout.Count - 1; i >= 0; i--)
        {
            var item = _layout[i];
            if (!_elements.TryGetValue(item, out var block)) continue;

            var bounds = new Rect(Canvas.GetLeft(block), Canvas.GetTop(block), block.ActualWidth, block.ActualHeight);
            bounds.Inflate(3, 3);
            if (bounds.Contains(point)) return item;
        }

        return null;
    }

    private void SelectItem(LcdItem? item) => SetSelection(item is null ? Array.Empty<LcdItem>() : new[] { item }, item);

    /// <summary>Выделить объекты; <paramref name="primary"/> — чьи свойства показывать (по умолчанию — последний).</summary>
    private void SetSelection(IEnumerable<LcdItem> items, LcdItem? primary)
    {
        _multi.Clear();
        foreach (var item in items)
            if (!_multi.Contains(item)) _multi.Add(item);

        _selected = primary is not null && _multi.Contains(primary) ? primary : _multi.LastOrDefault();
        UpdateProps();
        UpdateSelectionBox();
    }

    /// <summary>Выделенные объекты (главный — тоже); пусто — ничего не выделено.</summary>
    private IReadOnlyList<LcdItem> Selection =>
        _multi.Count > 0 ? _multi : _selected is not null ? new[] { _selected } : Array.Empty<LcdItem>();

    private void OnSceneDown(object sender, MouseButtonEventArgs e)
    {
        SceneHost.Focus();

        var point = e.GetPosition(SceneCanvas);
        var hit = HitItem(point);
        var additive = (Keyboard.Modifiers & (ModifierKeys.Control | ModifierKeys.Shift)) != 0;

        if (hit is not null)
        {
            if (additive)
            {
                // Ctrl/Shift + щелчок — добавить объект к выделению или убрать из него.
                var list = Selection.ToList();
                if (list.Remove(hit))
                {
                    SetSelection(list, ReferenceEquals(_selected, hit) ? null : _selected);
                    e.Handled = true;
                    return;
                }

                list.Add(hit);
                SetSelection(list, hit);
            }
            else if (!_multi.Contains(hit))
            {
                SelectItem(hit);
            }
            else if (!ReferenceEquals(_selected, hit))
            {
                // Щелчок по уже выделенному — группа остаётся, на панели — его свойства.
                _selected = hit;
                UpdateProps();
                UpdateSelectionBox();
            }

            // Тянем всё выделенное разом.
            _dragging = true;
            _dragStart = point;
            _dragOrigins = Selection.ToDictionary(i => i, i => new Point(i.X, i.Y));
            SceneHost.CaptureMouse();
        }
        else
        {
            // Пустое место: рамка выделения. С Ctrl/Shift — добавляем к уже выделенному.
            _marquee = true;
            _marqueeStart = point;
            _marqueeBase = additive ? Selection.ToList() : new List<LcdItem>();
            if (!additive) SelectItem(null);

            Canvas.SetLeft(MarqueeRect, point.X);
            Canvas.SetTop(MarqueeRect, point.Y);
            MarqueeRect.Width = 0;
            MarqueeRect.Height = 0;
            MarqueeRect.Visibility = Visibility.Visible;
            SceneHost.CaptureMouse();
        }

        e.Handled = true;
    }

    private void OnSceneMove(object sender, MouseEventArgs e)
    {
        var point = e.GetPosition(SceneCanvas);

        if (_marquee)
        {
            var rect = new Rect(_marqueeStart, point);
            Canvas.SetLeft(MarqueeRect, rect.X);
            Canvas.SetTop(MarqueeRect, rect.Y);
            MarqueeRect.Width = rect.Width;
            MarqueeRect.Height = rect.Height;

            // Выделяется всё, чего рамка касается.
            var inside = _layout.Where(i => _elements.TryGetValue(i, out var b) && BoundsOf(b).IntersectsWith(rect));
            var next = _marqueeBase.Concat(inside.Where(i => !_marqueeBase.Contains(i))).ToList();
            if (!next.SequenceEqual(_multi)) SetSelection(next, _marqueeBase.Count > 0 ? _selected : null);
            return;
        }

        if (!_dragging || _dragOrigins.Count == 0) return;

        var delta = point - _dragStart;
        foreach (var (item, origin) in _dragOrigins)
        {
            if (!_elements.TryGetValue(item, out var block)) continue;

            item.X = Math.Round(Math.Clamp(origin.X + delta.X, 12 - block.ActualWidth, SceneCanvas.Width - 12));
            item.Y = Math.Round(Math.Clamp(origin.Y + delta.Y, 12 - block.ActualHeight, SceneCanvas.Height - 12));

            Canvas.SetLeft(block, item.X);
            Canvas.SetTop(block, item.Y);
        }

        UpdateSelectionBox();
    }

    private void OnSceneUp(object sender, MouseButtonEventArgs e)
    {
        if (_marquee)
        {
            _marquee = false;
            MarqueeRect.Visibility = Visibility.Collapsed;
            SceneHost.ReleaseMouseCapture();
            return;
        }

        if (!_dragging) return;

        _dragging = false;
        _dragOrigins = new Dictionary<LcdItem, Point>();
        SceneHost.ReleaseMouseCapture();
        SaveSoon();
    }

    private void OnSceneWheel(object sender, MouseWheelEventArgs e)
    {
        var items = Selection;
        if (items.Count == 0) return;

        var factor = e.Delta > 0 ? 1.06 : 1 / 1.06;
        foreach (var item in items) item.Scale = Math.Clamp(Math.Round(item.Scale * factor, 2), 0.3, 4);

        RebuildScene();
        UpdateProps();
        SaveSoon();
        e.Handled = true;
    }

    private void OnSceneKeyDown(object sender, KeyEventArgs e)
    {
        var ctrl = Keyboard.Modifiers.HasFlag(ModifierKeys.Control);

        if (ctrl && e.Key == Key.A)
        {
            SetSelection(_layout, _selected);
            e.Handled = true;
            return;
        }

        if (e.Key == Key.Escape)
        {
            SelectItem(null);
            e.Handled = true;
            return;
        }

        var items = Selection;
        if (items.Count == 0) return;

        var step = Keyboard.Modifiers.HasFlag(ModifierKeys.Shift) ? 10 : 1;
        var (dx, dy) = e.Key switch
        {
            Key.Left => (-step, 0),
            Key.Right => (step, 0),
            Key.Up => (0, -step),
            Key.Down => (0, step),
            _ => (0, 0)
        };
        if (dx == 0 && dy == 0) return;

        foreach (var item in items)
        {
            item.X += dx;
            item.Y += dy;
            if (_elements.TryGetValue(item, out var block))
            {
                Canvas.SetLeft(block, item.X);
                Canvas.SetTop(block, item.Y);
            }
        }

        // Сдвиг — без пересборки сцены: элементы просто переставляем.
        UpdateSelectionBox();
        SaveSoon();
        e.Handled = true;
    }

    /// <summary>Del удаляет выделенный объект, пока фокус не в поле ввода.</summary>
    private void OnWindowPreviewKeyDown(object sender, KeyEventArgs e)
    {
        if (e.Key != Key.Delete || Selection.Count == 0 || !PagePump.IsVisible || CoolerImageMode) return;
        if (Keyboard.FocusedElement is TextBox) return;

        DeleteSelected();
        e.Handled = true;
    }

    // ── Темы ─────────────────────────────────────────────────────────────────

    private void OnThemeClick(object sender, MouseButtonEventArgs e)
    {
        if (sender is Border { Tag: LcdTheme theme }) ApplyTheme(theme);
    }

    /// <summary>Тема меняет расстановку и фон целиком; потом всё это можно править руками.</summary>
    private void ApplyTheme(LcdTheme theme)
    {
        var items = ThemeItems(theme);

        _layout.Clear();
        _layout.AddRange(items);
        ClearSelectionState();

        _bgTop = theme.Top;
        _bgBottom = theme.Bottom;
        SetBackgroundFrames(null);
        _backgroundPath = null;
        if (theme.Image is not null) LoadBackground(theme.Image);
        BgName.Text = $"Тема «{theme.Name}»";
        UpdateBgSwatches();
        _settings.CoolerTheme = theme.Name;

        if (theme.Brightness is { } brightness)
        {
            _coolerLoading = true;
            BgBrightness.Value = brightness;
            UpdateBrightnessText();
            _coolerLoading = false;
        }

        RebuildScene();
        UpdateProps();
        SaveSoon();
    }

    /// <summary>Расстановка темы под текущий экран: для вертикального — своя, затем подгонка под размер.</summary>
    private List<LcdItem> ThemeItems(LcdTheme theme)
    {
        var (w, h) = SceneSize();

        // Вертикальный экран (поворот 90°/270°) — своя раскладка под 240×320.
        var portrait = h > w && theme.Portrait is not null;
        var baseW = portrait ? 240.0 : 320.0;
        var baseH = portrait ? 320.0 : 240.0;
        var kx = w / baseW;
        var ky = h / baseH;
        var ks = Math.Min(kx, ky);

        var items = portrait ? theme.Portrait!() : theme.Items();
        foreach (var item in items)
        {
            item.X = Math.Round(item.X * kx);
            item.Y = Math.Round(item.Y * ky);
            item.Scale = Math.Round(item.Scale * ks, 2);
        }

        return items;
    }

    // ── Редактор: объекты ────────────────────────────────────────────────────

    private void OnAddClick(object sender, RoutedEventArgs e) => AddPopup.IsOpen = !AddPopup.IsOpen;

    private void OnAddKind(object sender, RoutedEventArgs e)
    {
        AddPopup.IsOpen = false;

        if (sender is not Button button || button.Tag is not string tag) return;
        if (!Enum.TryParse<LcdKind>(tag, out var kind)) return;

        var (w, h) = SceneSize();
        var same = _layout.Count(i => i.Kind == kind);

        var item = LcdItem.Create(kind);
        item.X = Math.Min(Math.Max(0, w - 100), 24 + same * 18);
        item.Y = Math.Min(Math.Max(0, h - 50), 24 + same * 18);

        _layout.Add(item);
        RebuildScene();
        SelectItem(item);
        SaveSoon();
    }

    private void OnDeleteClick(object sender, RoutedEventArgs e) => DeleteSelected();

    private void DeleteSelected()
    {
        var items = Selection.ToList();
        if (items.Count == 0) return;

        foreach (var item in items) _layout.Remove(item);
        ClearSelectionState();

        RebuildScene();
        UpdateProps();
        SaveSoon();
    }

    private void OnResetLayout(object sender, RoutedEventArgs e)
    {
        var answer = MessageBox.Show(this, "Вернуть расстановку объектов по умолчанию? Свои объекты пропадут.",
            "Rig Control", MessageBoxButton.YesNo, MessageBoxImage.Question);
        if (answer != MessageBoxResult.Yes) return;

        _layout.Clear();
        _layout.AddRange(ThemeItems(LcdThemes.Default));
        ClearSelectionState();

        RebuildScene();
        UpdateProps();
        SaveSoon();
    }

    // ── Редактор: свойства выбранного объекта ────────────────────────────────

    private void UpdateProps()
    {
        _propLoading = true;

        try
        {
            var item = _selected;

            PropBody.IsEnabled = item is not null;
            PropBody.Opacity = item is null ? 0.45 : 1;
            PropHint.Visibility = item is null ? Visibility.Visible : Visibility.Collapsed;
            DeleteButton.IsEnabled = item is not null;

            if (item is null)
            {
                PropTitle.Text = "";
                return;
            }

            var count = Selection.Count;
            PropTitle.Text = count > 1
                ? $"{LcdItem.KindName(item.Kind)} и ещё {count - 1} · изменения — для всех выделенных"
                : LcdItem.KindName(item.Kind);

            PropTextRow.Visibility = item.Kind == LcdKind.Text ? Visibility.Visible : Visibility.Collapsed;
            PropText.Text = item.Text ?? "";

            PropLabel.Visibility = item.HasLabel() ? Visibility.Visible : Visibility.Collapsed;
            PropLabel.IsChecked = item.Label;
            PropSeconds.Visibility = item.Kind == LcdKind.Time ? Visibility.Visible : Visibility.Collapsed;
            PropSeconds.IsChecked = item.Seconds;
            PropBold.IsChecked = item.Bold;

            if (!string.IsNullOrWhiteSpace(item.Font) && !FontNames.Contains(item.Font))
                PropFont.ItemsSource = FontNames.Append(item.Font).ToList();
            else if (PropFont.ItemsSource is not string[])
                PropFont.ItemsSource = FontNames;

            PropFont.SelectedItem = string.IsNullOrWhiteSpace(item.Font) ? FontNames[0] : item.Font;

            PropColorBox.Text = string.IsNullOrWhiteSpace(item.Color) ? "авто" : item.Color;
            PropColorAuto.Visibility = item.Kind is LcdKind.CpuTemp or LcdKind.GpuTemp
                ? Visibility.Visible
                : Visibility.Collapsed;

            PropScale.Value = Math.Clamp(item.Scale, PropScale.Minimum, PropScale.Maximum);
            PropScaleLabel.Text = $"Размер: {item.Scale * 100:0}%";
        }
        finally
        {
            _propLoading = false;
        }
    }

    /// <summary>Изменение свойства — всем выделенным объектам (или только главному).</summary>
    private void ApplyProp(Action<LcdItem> change, bool primaryOnly = false)
    {
        if (_propLoading || _coolerLoading || _selected is not { } item) return;

        if (primaryOnly) change(item);
        else foreach (var each in Selection) change(each);

        RebuildScene();
        SaveSoon();
    }

    // Текст у каждого свой — меняем только у главного.
    private void OnPropTextChanged(object sender, TextChangedEventArgs e) =>
        ApplyProp(item => item.Text = PropText.Text, primaryOnly: true);

    /// <summary>Флажок — только тот, что нажали, и только тем, у кого он есть.</summary>
    private void OnPropFlagChanged(object sender, RoutedEventArgs e)
    {
        if (sender == PropLabel)
            ApplyProp(item => { if (item.HasLabel()) item.Label = PropLabel.IsChecked == true; });
        else if (sender == PropSeconds)
            ApplyProp(item => { if (item.Kind == LcdKind.Time) item.Seconds = PropSeconds.IsChecked == true; });
        else if (sender == PropBold)
            ApplyProp(item => item.Bold = PropBold.IsChecked == true);
    }

    private void OnPropFontChanged(object sender, SelectionChangedEventArgs e)
    {
        if (PropFont.SelectedItem is string font) ApplyProp(item => item.Font = font);
    }

    private void OnPropScaleChanged(object sender, RoutedPropertyChangedEventArgs<double> e)
    {
        if (_propLoading || _coolerLoading) return;

        var value = Math.Round(e.NewValue, 2);
        PropScaleLabel.Text = $"Размер: {value * 100:0}%";

        // У группы размер меняется во столько же раз, во сколько у главного: пропорции сохраняются.
        var factor = _selected is { Scale: > 0 } main ? value / main.Scale : 1;
        ApplyProp(item => item.Scale = ReferenceEquals(item, _selected)
            ? value
            : Math.Clamp(Math.Round(item.Scale * factor, 2), 0.3, 4));
    }

    private void OnSwatchClick(object sender, MouseButtonEventArgs e)
    {
        if (sender is not Border { Tag: string hex }) return;

        ApplyProp(item => item.Color = hex);
        UpdateProps();
    }

    private void OnPropColorTextChanged(object sender, TextChangedEventArgs e)
    {
        if (_propLoading || _coolerLoading) return;

        var text = PropColorBox.Text.Trim();
        if (!text.StartsWith('#') || !LcdScene.TryParseColor(text, out _)) return;

        ApplyProp(item => item.Color = text);
    }

    private void OnPropWheelClick(object sender, MouseButtonEventArgs e)
    {
        if (_selected is null) return;
        var initial = LcdScene.TryParseColor(PropColorBox.Text.Trim(), out var c) ? c : Colors.White;
        ColorWheel.ShowPopup(PropWheelButton, initial, color => PropColorBox.Text = ColorWheel.Hex(color));
    }

    private void OnPropColorAuto(object sender, RoutedEventArgs e)
    {
        ApplyProp(item => item.Color = "");
        UpdateProps();
    }

    // ── Фон, яркость, частота ────────────────────────────────────────────────

    private void OnBgPick(object sender, RoutedEventArgs e)
    {
        var dialog = new OpenFileDialog
        {
            Title = "Фон для экрана помпы",
            Filter = "Изображения|*.png;*.jpg;*.jpeg;*.gif;*.bmp;*.tif;*.tiff|Все файлы|*.*"
        };

        if (dialog.ShowDialog(this) == true) SetBackground(dialog.FileName);
    }

    private void OnBgReset(object sender, RoutedEventArgs e)
    {
        SetBackgroundFrames(null);
        _backgroundPath = null;
        _bgTop = null;
        _bgBottom = null;
        BgName.Text = "Стандартный тёмный фон";
        UpdateBgSwatches();

        RebuildScene();
        SaveSoon();
    }

    private static readonly Color DefaultBgTop = Color.FromRgb(0x10, 0x12, 0x1A);
    private static readonly Color DefaultBgBottom = Color.FromRgb(0x05, 0x06, 0x0A);

    private Color BgColor(bool top)
    {
        var text = top ? _bgTop : _bgBottom;
        return LcdScene.TryParseColor(text ?? "", out var c) ? c : top ? DefaultBgTop : DefaultBgBottom;
    }

    private void UpdateBgSwatches()
    {
        BgTopSwatch.Background = new SolidColorBrush(BgColor(true));
        BgBottomSwatch.Background = new SolidColorBrush(BgColor(false));
    }

    /// <summary>Свой фон: два цвета градиента (верх/низ) через цветовой круг.</summary>
    private void OnBgColorClick(object sender, MouseButtonEventArgs e)
    {
        if (sender is not Border swatch) return;
        var top = (swatch.Tag as string) == "top";

        ColorWheel.ShowPopup(swatch, BgColor(top), color =>
        {
            var hex = ColorWheel.Hex(color);
            if (top) _bgTop = hex; else _bgBottom = hex;
            // отдельные цвета не должны «сбросить» другой: фиксируем оба
            if (top) _bgBottom ??= ColorWheel.Hex(DefaultBgBottom); else _bgTop ??= ColorWheel.Hex(DefaultBgTop);

            SetBackgroundFrames(null);
            _backgroundPath = null;
            BgName.Text = "Свой градиент";
            UpdateBgSwatches();
            RebuildScene();
            SaveSoon();
        });
    }

    private void SetBackground(string path)
    {
        if (!LoadBackground(path)) return;

        RebuildScene();
        SaveSoon();
    }

    private bool LoadBackground(string path)
    {
        try
        {
            var (w, h) = SceneSize();
            _bgLoadedLong = Math.Max(w, h);
            _bgLoadedShort = Math.Min(w, h);

            var (first, animation) = LcdRender.LoadBackground(path, w, h, 100);
            SetBackgroundFrames(first, animation);
            _backgroundPath = path;
            BgName.Text = (LcdRender.IsResource(path) ? "Встроенный фон" : Path.GetFileName(path)) +
                          (animation is { Count: > 1 } ? $" · {animation.Count} кадр." : "");
            return true;
        }
        catch (Exception ex)
        {
            BgName.Text = $"Не удалось открыть файл: {ex.Message}";
            return false;
        }
    }

    /// <summary>Сцена стала крупнее той, под которую уменьшали фон (например, подключилась помпа побольше), — перечитываем фон.</summary>
    private void EnsureBackgroundSize()
    {
        if (_backgroundPath is null) return;

        var (w, h) = SceneSize();
        if (Math.Max(w, h) <= _bgLoadedLong && Math.Min(w, h) <= _bgLoadedShort) return;

        // Запоминаем сразу: если файл пропал, не пытаемся читать его заново на каждом кадре.
        _bgLoadedLong = Math.Max(w, h);
        _bgLoadedShort = Math.Min(w, h);
        LoadBackground(_backgroundPath);
    }

    /// <summary>Фон-картинка или гифка (кадры в JPEG); null — без картинки.</summary>
    private void SetBackgroundFrames(BitmapSource? first, List<(byte[] Jpeg, int DelayMs)>? animation = null)
    {
        _background = first;
        _bgFrames = first is not null && animation is { Count: > 1 } ? animation : null;
        _bgTotalMs = _bgFrames?.Sum(f => (long)Math.Max(20, f.DelayMs)) ?? 0;
        _bgCachedIndex = -1;
        _bgFrameBitmap = null;
        _bgPixels = null;

        if (_bgFrames is not null && _bgAnimTimer is null)
        {
            _bgAnimTimer = new DispatcherTimer(DispatcherPriority.Render) { Interval = TimeSpan.FromMilliseconds(40) };
            _bgAnimTimer.Tick += OnBgAnimTick;
        }

        if (_bgFrames is not null) _bgAnimTimer!.Start();
        else _bgAnimTimer?.Stop();
    }

    /// <summary>Номер кадра фона-гифки по текущему времени (−1 — фон не анимирован).</summary>
    private int BgFrameIndex()
    {
        var frames = _bgFrames;
        if (frames is null || _bgTotalMs <= 0) return -1;

        var t = Environment.TickCount64 % _bgTotalMs;
        for (var i = 0; i < frames.Count; i++)
        {
            t -= Math.Max(20, frames[i].DelayMs);
            if (t < 0) return i;
        }

        return frames.Count - 1;
    }

    private BitmapSource? CurrentBackground()
    {
        var index = BgFrameIndex();
        if (index < 0 || _bgFrames is not { } frames) return _background;
        if (index == 0) return _background;

        if (index != _bgCachedIndex || _bgFrameBitmap is null)
        {
            try
            {
                LcdRender.DecodeJpegInto(frames[index].Jpeg, ref _bgFrameBitmap, ref _bgPixels);
                _bgCachedIndex = index;
            }
            catch
            {
                _bgCachedIndex = -1;
                return _background;
            }
        }

        return _bgFrameBitmap;
    }

    /// <summary>Анимация фона-гифки в предпросмотре: меняем только картинку фона, сцену не пересобираем.</summary>
    private void OnBgAnimTick(object? sender, EventArgs e)
    {
        if (_bgPreviewImage is null || CoolerImageMode || !PagePump.IsVisible) return;

        var current = CurrentBackground();
        if (!ReferenceEquals(_bgPreviewImage.Source, current)) _bgPreviewImage.Source = current;
    }

    private void HandleCoolerDrop(string path)
    {
        if (CoolerImageMode) LoadCoolerImage(path);
        else SetBackground(path);
    }

    private void OnBgBrightnessChanged(object sender, RoutedPropertyChangedEventArgs<double> e)
    {
        if (_coolerLoading) return;

        UpdateBrightnessText();
        RebuildScene();
        SaveSoon();
    }

    private void UpdateBrightnessText() =>
        BgBrightnessLabel.Text = $"Яркость фона: {(int)Math.Round(BgBrightness.Value)}%";

    private void OnFpsChanged(object sender, RoutedPropertyChangedEventArgs<double> e)
    {
        if (_coolerLoading) return;

        _settings.CoolerFps = (int)Math.Round(FpsSlider.Value);
        UpdateFpsLabel();
        SaveSoon();
    }

    private void UpdateFpsLabel() => FpsLabel.Text = $"Частота: до {(int)Math.Round(FpsSlider.Value)} кадр/с";

    private void OnToggleDiag(object sender, RoutedEventArgs e)
    {
        DiagPanel.Visibility = DiagCheck.IsChecked == true ? Visibility.Visible : Visibility.Collapsed;
        RefreshCoolerLog();
    }

    // ── Датчики и сторожевой таймер ──────────────────────────────────────────

    /// <summary>
    /// Когда ни одна страница не использует датчики, не нужно будить
    /// LibreHardwareMonitor каждую секунду. Редкий тик оставляем только для
    /// быстрой реакции на смену вкладки. Вывод помпы и watchdog работают отдельно.
    /// </summary>
    private void UpdateTelemetryTimerState()
    {
        if (_tick is null)
            return;

        var needed = (!CoolerImageMode && PagePump.IsVisible)
                     || (CoolerRunning && !CoolerImageMode)
                     || ArgbWantsCpuTemp
                     || FansWanted
                     || SystemWanted;
        _tick.Interval = needed
            ? TimeSpan.FromSeconds(1)
            : TimeSpan.FromSeconds(8);
    }

    private async void OnTick(object? sender, EventArgs e)
    {
        if (_tickBusy) return;
        _tickBusy = true;

        try
        {
            // Датчики нужны, только если экран помпы работает или открыт редактор.
            var editorVisible = !CoolerImageMode && PagePump.IsVisible;
            var argbTemp = ArgbWantsCpuTemp;
            var fans = FansWanted;
            var system = SystemWanted;
            if (!editorVisible && !(CoolerRunning && !CoolerImageMode) && !argbTemp && !fans && !system)
            {
                CoolerFpsText.Text = CoolerRunning ? $"Отправлено: {_sentInWindow} кадр/с" : "";
                LogPumpRate();
                _sentInWindow = 0;
                await ReleaseIdleTelemetryAsync();
                return;
            }

            _telemetryIdleSince = 0;
            _telemetry ??= new Telemetry();

            // LibreHardwareMonitor (температуры, видеокарта, частота, мощность) — только если такие объекты есть на экране.
            // Странице «О системе» нужна и плата: на её чипе мониторинга — Vcore.
            var telemetry = _telemetry;
            var sensors = argbTemp || fans || system || _layout.Any(i => Telemetry.NeedsSensors(i.Kind));
            _snapshot = await Task.Run(() => telemetry.Read(sensors, fans || system, system));
            _argbCpuTemp = _snapshot.CpuTemp ?? double.NaN;
            if (fans) await UpdateFansAsync(telemetry);
            if (system) UpdateSystemPage(_snapshot, telemetry.SensorsAvailable);

            if (editorVisible && !_dragging && SceneSignature() != _previewSignature) RebuildScene();

            CoolerFpsText.Text = CoolerRunning ? $"Отправлено: {_sentInWindow} кадр/с" : "";
            LogPumpRate();
            _sentInWindow = 0;
        }
        catch (Exception ex)
        {
            // Датчики недоступны — экран работает с прочерками.
            QuietLog.Write(ex);
        }
        finally
        {
            _tickBusy = false;
        }
    }

    private void LogPumpRate()
    {
        if (!CoolerRunning || _cooler is not { } lcd) return;

        _pumpLogTicks++;
        if (_pumpLogTicks < 60) return;

        var sent = lcd.FramesSent;
        var mode = CoolerImageMode ? "картинка" : "редактор";
        PumpLog.Write($"работа: режим {mode}, окно {(WindowShown ? "на экране" : "в трее")}, " +
                      $"за минуту {sent - _pumpLogFrames} кадров, всего {sent}, ошибок {lcd.Errors}, " +
                      $"рукопожатий {lcd.Handshakes}");
        _pumpLogFrames = sent;
        _pumpLogTicks = 0;
    }

    /// <summary>Датчики никому не нужны уже минуту — закрываем LibreHardwareMonitor: он занимает заметно памяти.</summary>
    private async Task ReleaseIdleTelemetryAsync()
    {
        if (_telemetry is null) return;

        var now = Environment.TickCount64;
        if (_telemetryIdleSince == 0) _telemetryIdleSince = now;
        if (now - _telemetryIdleSince < 60_000) return;

        var telemetry = _telemetry;
        _telemetry = null;
        _telemetryIdleSince = 0;
        await Task.Run(telemetry.Dispose);
    }

    /// <summary>
    /// Сторожевой таймер. Помпа после паузы в кадрах или сбоя USB возвращается к своему логотипу,
    /// поэтому смотрим не только «работает ли цикл», но и «идут ли кадры»: сначала повторяем
    /// рукопожатие, потом переподключаем устройство, в крайнем случае сбрасываем USB — как кнопкой,
    /// но уже без участия человека.
    /// </summary>
    private async void OnWatchdog(object? sender, EventArgs e)
    {
        if (_coolerBusy || _coolerLoading || !ScreenWanted) return;

        _coolerBusy = true;
        try
        {
            if (CoolerRunning && _cooler is { } device)
            {
                if (device.FramesSent != _watchFrames)
                {
                    _watchFrames = device.FramesSent;
                    _stallChecks = 0;
                    return;
                }

                // Кадры не идут: 10 с — ещё повод подождать, дальше по ступеням.
                _stallChecks++;
                if (_stallChecks <= 2) return;

                PumpLog.Write($"кадры не идут {_stallChecks * 5} с " +
                              $"(всего {device.FramesSent}, ошибок {device.Errors}, последняя: {device.LastError})");

                switch (_stallChecks)
                {
                    case 3:
                        // Дешёвый шаг: то же рукопожатие, что при запуске показа.
                        var ok = await Task.Run(() => device.Reinit(400));
                        PumpLog.Write($"сторож: рукопожатие {(ok ? "принято" : "не принято")}");
                        break;

                    case 4:
                        PumpLog.Write("сторож: переподключаю помпу");
                        await CoolerConnectAsync(quiet: true);
                        _watchFrames = 0;
                        if (_cooler is not null && (!CoolerImageMode || _coolerFrames is not null)) StartCooler();
                        break;

                    default:
                        // Ни рукопожатие, ни переподключение не помогли. После короткой
                        // серии сбоев сбрасываем только USB-экран, а не весь ПК. Раньше
                        // ожидание было 5 минут — за это время экран фактически оставался
                        // выключенным во время игры.
                        if (Environment.TickCount64 - _lastUsbReset > 60_000)
                        {
                            PumpLog.Write("сторож: сбрасываю USB экрана");
                            await ResetScreenUsbAsync(quiet: true);
                        }

                        _stallChecks = 0;
                        break;
                }

                return;
            }

            if (CoolerRunning) return;

            // Цикл показа остановился сам (ошибка отправки, отключили USB, сон) — поднимаем заново.
            if (_cooler is null)
            {
                await CoolerConnectAsync(quiet: true);

                if (_cooler is null)
                {
                    // Помпа есть на шине, но не открывается — через короткую серию
                    // неудачных попыток автоматически переинициализируем USB.
                    var failedLongEnough = _pumpFailureSince != 0 &&
                                           Environment.TickCount64 - _pumpFailureSince > 20_000;
                    if (failedLongEnough && CoolerLcd.IsPresent() &&
                        Environment.TickCount64 - _lastUsbReset > 60_000)
                    {
                        PumpLog.Write("сторож: помпа не открывается — сбрасываю USB экрана");
                        await ResetScreenUsbAsync(quiet: true);
                    }

                    return;
                }
            }

            if (_cooler is not null && (!CoolerImageMode || _coolerFrames is not null))
                StartCooler();
        }
        catch (Exception ex)
        {
            // попробуем в следующий раз
            QuietLog.Write(ex);
        }
        finally
        {
            _coolerBusy = false;
        }
    }

    // ── Возврат экрана к показу ──────────────────────────────────────────────

    private void OnPumpPowerMode(object? sender, PowerModeChangedEventArgs e)
    {
        // Событие питания приходит не в потоке интерфейса — возвращаемся в него,
        // иначе обновление кнопок ниже упадёт с ошибкой доступа к чужому потоку.
        if (e.Mode == PowerModes.Resume) Dispatcher.BeginInvoke(() => _ = ReArmOnWakeAsync());
    }

    /// <summary>
    /// После сна или возврата окна: повторяем рукопожатие и сразу отправляем кадр.
    /// Помпа, вернувшаяся к логотипу, без этого новые кадры не принимает.
    /// </summary>
    private async Task ReArmOnWakeAsync()
    {
        var lcd = _cooler;
        if (lcd is null || !ScreenWanted || _coolerBusy) return;

        try
        {
            var ok = await Task.Run(() => lcd.Reinit(400));
            _watchFrames = lcd.FramesSent;
            _stallChecks = 0;
            PumpLog.Write($"после сна/возврата окна: рукопожатие {(ok ? "принято" : "не принято")}");

            if (!CoolerRunning && _cooler is not null && (!CoolerImageMode || _coolerFrames is not null))
                StartCooler();
        }
        catch (Exception ex)
        {
            QuietLog.Write(ex);
        }
    }

    /// <summary>
    /// Перед кадром: если отправка надолго прервалась (сборка мусора, сон, свёрнутое окно) или просто
    /// прошло много времени, прошивка помпы уже могла показать свой логотип и кадры игнорировать.
    /// Повторяем рукопожатие — оно возвращает экран к показу без сброса USB.
    /// </summary>
    private async Task ReArmBeforeFrameAsync(CoolerLcd lcd, int interval, CancellationToken token)
    {
        var now = Environment.TickCount64;
        var gap = lcd.LastFrameTick == 0 ? 0 : now - lcd.LastFrameTick;
        var pause = Math.Max(1500, interval * 2 + 500);

        var afterPause = gap > pause;
        var afterTime = now > _reArmBlockedUntil && now - _lastReArm > KeepAliveMs;
        if (!afterPause && !afterTime) return;

        _lastReArm = now;

        bool ok;
        try
        {
            ok = await Task.Run(() => lcd.Reinit(250), token);
        }
        catch (OperationCanceledException)
        {
            throw;
        }
        catch (Exception ex)
        {
            QuietLog.Write(ex);
            return;
        }

        if (ok)
        {
            _reArmBlockedUntil = 0;
            if (afterPause) PumpLog.Write($"пауза в кадрах {gap} мс — рукопожатие принято");
        }
        else
        {
            // Не ответила: либо занята, либо это другая прошивка. Кадры шлём дальше,
            // а профилактические рукопожатия на время прекращаем.
            _reArmBlockedUntil = now + 300_000;
            PumpLog.Write("профилактическое рукопожатие: ответа нет");
        }
    }

    private const long KeepAliveMs = 30_000;   // как часто повторять рукопожатие «на всякий случай»

    // ── Подключение ──────────────────────────────────────────────────────────

    private async Task CoolerConnectAsync(bool quiet = false)
    {
        await StopCoolerAsync();

        CoolerFindButton.IsEnabled = false;
        if (!quiet) CoolerInfo.Text = "Ищу помпу…";

        var (device, error) = await Task.Run(() =>
        {
            var found = CoolerLcd.TryOpen(out var message);
            return (found, message);
        });

        _cooler?.Dispose();
        _cooler = device;
        if (device is not null)
            _pumpFailureSince = 0;
        else if (_pumpFailureSince == 0)
            _pumpFailureSince = Environment.TickCount64;

        CoolerInfo.Text = device is not null
            ? $"Подключена · {device.Resolution.Width}×{device.Resolution.Height}" +
              (device.HandshakeOk ? "" : " · нет ответа")
            : error ?? "Помпа не найдена.";

        CoolerFindButton.IsEnabled = true;
        RefreshCoolerLog();
        RebuildScene();
        UpdateCoolerButtons();
    }

    private async void OnCoolerFind(object sender, RoutedEventArgs e)
    {
        _userStopped = false;
        await CoolerConnectAsync();
    }

    /// <summary>
    /// Журнал помпы. Обновляется до 10 раз в секунду (на каждый кадр), поэтому текст собираем и
    /// подставляем, только когда журнал виден на экране и что-то поменялось.
    /// </summary>
    private void RefreshCoolerLog()
    {
        if (DiagPanel.Visibility != Visibility.Visible || !WindowShown || !PagePump.IsVisible) return;

        var text = _cooler is null ? "Помпа не подключена." : _cooler.Description + Environment.NewLine + _cooler.Diagnostics;
        if (CoolerLog.Text != text) CoolerLog.Text = text;
    }

    /// <summary>Как в TrccSharp: удалить USB-устройство экрана и пересканировать, чтобы Windows подняла его заново.</summary>
    private async void OnCoolerReset(object sender, RoutedEventArgs e) => await ResetScreenUsbAsync(quiet: false);

    /// <summary>
    /// Полный сброс экрана через USB: удалить устройство, дождаться, пересканировать шину, подключиться заново
    /// и (если экран должен работать) возобновить показ. quiet — без сообщений в интерфейсе, для сторожа.
    /// </summary>
    private async Task ResetScreenUsbAsync(bool quiet)
    {
        await StopCoolerAsync();
        _cooler?.Dispose();
        _cooler = null;

        _lastUsbReset = Environment.TickCount64;
        if (!quiet) CoolerInfo.Text = "Сбрасываю USB экрана…";
        PumpLog.Write("сброс USB экрана: удаляю устройство и пересканирую шину");

        const string script =
            "$d = Get-PnpDevice -PresentOnly -ErrorAction SilentlyContinue | " +
            "Where-Object { $_.InstanceId -like 'USB\\VID_0416&PID_5302\\*' }; " +
            "foreach ($x in $d) { pnputil /remove-device \"$($x.InstanceId)\" | Out-Null }; " +
            "Start-Sleep -Seconds 2; pnputil /scan-devices | Out-Null; Start-Sleep -Seconds 3";

        try
        {
            await Task.Run(() =>
            {
                var escaped = script.Replace("\"", "\\\"");
                var info = new System.Diagnostics.ProcessStartInfo("powershell.exe",
                    "-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -Command \"" + escaped + "\"")
                {
                    CreateNoWindow = true,
                    UseShellExecute = false
                };

                using var process = System.Diagnostics.Process.Start(info);
                process?.WaitForExit(30000);
            });
        }
        catch (Exception ex)
        {
            if (!quiet) CoolerInfo.Text = $"Не удалось сбросить: {ex.Message}";
            PumpLog.Write($"сброс USB не удался: {ex.Message}");
        }

        await CoolerConnectAsync(quiet);

        if (ScreenWanted && _cooler is not null && (!CoolerImageMode || _coolerFrames is not null))
        {
            StartCooler();
            PumpLog.Write("после сброса USB показ возобновлён");
        }
    }

    private void UpdateCoolerButtons()
    {
        CoolerStartButton.IsEnabled = _cooler is not null && (!CoolerImageMode || _coolerFrames is not null);
        CoolerStartButton.Content = CoolerRunning ? "Остановить" : "Запустить";

        PumpStatusText.Text = CoolerRunning ? "Экран работает" : _cooler is not null ? "Подключена" : "Не найдена";
        PumpStatusDot.SetResourceReference(System.Windows.Shapes.Shape.FillProperty,
            CoolerRunning || _cooler is not null ? "GoodBrush" : "BadBrush");

        if (_trayToggle is not null)
            _trayToggle.Header = CoolerRunning ? "Остановить экран помпы" : "Запустить экран помпы";
    }

    // ── Обработчики интерфейса ───────────────────────────────────────────────

    private async void OnCoolerModeChanged(object sender, RoutedEventArgs e)
    {
        if (_loading || _coolerLoading) return;

        ApplyModeVisibility();
        RebuildScene();
        await RestartCoolerIfRunningAsync();
    }

    private async void OnCoolerOptionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (_loading || _coolerLoading) return;

        // Размер и поворот влияют на подготовленные кадры — готовим заново.
        if (_coolerImagePath is not null) LoadCoolerImage(_coolerImagePath);

        // Экран сменил ориентацию — переставляем объекты темы под новую (фон остаётся).
        var (w, h) = SceneSize();
        var portrait = h > w;
        if (portrait != _scenePortrait)
        {
            _scenePortrait = portrait;
            var theme = LcdThemes.All.FirstOrDefault(t => t.Name == _settings.CoolerTheme) ?? LcdThemes.Default;
            _layout.Clear();
            _layout.AddRange(ThemeItems(theme));
            ClearSelectionState();
            UpdateProps();
        }

        RebuildScene();
        SaveSoon();
        await RestartCoolerIfRunningAsync();
    }

    private void OnCoolerPick(object sender, RoutedEventArgs e)
    {
        var dialog = new OpenFileDialog
        {
            Title = "Картинка или гифка для экрана помпы",
            Filter = "Изображения|*.png;*.jpg;*.jpeg;*.gif;*.bmp;*.tif;*.tiff|Все файлы|*.*"
        };

        if (dialog.ShowDialog(this) == true) LoadCoolerImage(dialog.FileName);
    }

    private void LoadCoolerImage(string path)
    {
        try
        {
            var (width, height) = CoolerPanelSize();
            var (frames, preview) = LcdRender.LoadImage(path, width, height, CoolerRotation(), crop: true, maxFrames: 60);

            _coolerFrames = frames;
            _coolerImagePath = path;
            CoolerPreview.Source = preview;

            var seconds = frames.Sum(f => f.DelayMs) / 1000.0;
            CoolerInfo.Text = (_cooler is null ? "Помпа не найдена · " : "Подключена · ") +
                              $"кадров: {frames.Count} · {seconds:0.0} с · {Path.GetFileName(path)}";

            SaveSoon();
        }
        catch (Exception ex)
        {
            _coolerFrames = null;
            _coolerImagePath = null;
            CoolerInfo.Text = $"Не удалось прочитать файл: {ex.Message}";
        }

        UpdateCoolerButtons();
    }

    private async void OnCoolerStartStop(object sender, RoutedEventArgs e) => await ToggleCoolerAsync();

    private async Task ToggleCoolerAsync()
    {
        if (CoolerRunning)
        {
            _userStopped = true;
            _coolerWanted = false;
            await StopCoolerAsync();
            return;
        }

        _userStopped = false;
        _coolerWanted = true;
        if (_cooler is null) await CoolerConnectAsync();

        StartCooler();
    }

    private async Task RestartCoolerIfRunningAsync()
    {
        var wasRunning = CoolerRunning;
        await StopCoolerAsync();

        if (wasRunning && _cooler is not null && (!CoolerImageMode || _coolerFrames is not null))
            StartCooler();
        else
            UpdateCoolerButtons();
    }

    // ── Цикл отправки ────────────────────────────────────────────────────────

    private void StartCooler()
    {
        _coolerWanted = true;

        if (_cooler is null || CoolerRunning) return;
        if (CoolerImageMode && _coolerFrames is null) return;

        _coolerCts = new CancellationTokenSource();
        var token = _coolerCts.Token;

        _lastContentUpdate = 0;
        _watchFrames = _cooler.FramesSent;
        _stallChecks = 0;
        _pumpLogTicks = 0;
        _pumpLogFrames = _cooler.FramesSent;
        _lastReArm = Environment.TickCount64;

        _coolerActive = true;
        _coolerTask = CoolerImageMode ? RunImageLoopAsync(token) : RunEditorLoopAsync(token);
        UpdateCoolerButtons();
    }

    private async Task StopCoolerAsync()
    {
        var cts = _coolerCts;
        var task = _coolerTask;

        cts?.Cancel();

        if (task is not null)
        {
            try { await task; }
            catch { /* остановка — не ошибка */ }
        }

        cts?.Dispose();
        _coolerCts = null;
        _coolerTask = null;

        if (_coolerActive) PumpLog.Write("показ на экране помпы остановлен");

        _coolerActive = false;
        UpdateCoolerButtons();
    }

    /// <summary>Сбой отправки: бросаем устройство, сторожевой таймер переподключит его сам.</summary>
    private void DropCooler()
    {
        try { _cooler?.Dispose(); } catch { /* уже недоступно */ }
        _cooler = null;
        _pumpFailureSince = _pumpFailureSince == 0 ? Environment.TickCount64 : _pumpFailureSince;
        PumpLog.Write("устройство помпы отброшено после ошибки передачи");
    }

    private async Task RunEditorLoopAsync(CancellationToken token)
    {
        try
        {
            var interval = 1000 / Math.Clamp(_settings.CoolerFps, 1, 10);

            // Кадр уходит не реже раза в 0,5 с, даже если слайдер стоит на 1 кадр/с: долгое молчание
            // прошивка помпы считает сбоем и возвращается к своему логотипу. Содержимое при этом
            // обновляется с выбранной частотой — лишние кадры просто повторяют прошлый.
            // Если на фоне гифка — она идёт с темпом самой гифки (обычно 10 кадров в секунду):
            // кадр 320×240 уходит на помпу за ~43 мс (пакеты шлём по два в полёте), плюс
            // пауза после кадра, — так что цикл показа успевает за сменой кадров.
            var animated = _bgFrames is { Count: > 1 } ? 100 : 500;
            var tick = Math.Min(interval, animated);

            if (_cooler is { } device) await Task.Run(() => device.Reinit(), token);
            PumpLog.Write($"показ запущен: {_settings.CoolerFps} кадр/с, окно {(WindowShown ? "на экране" : "в трее")}");
            RefreshCoolerLog();

            while (!token.IsCancellationRequested)
            {
                var started = Environment.TickCount64;

                var lcd = _cooler;
                if (lcd is null) break;

                await ReArmBeforeFrameAsync(lcd, interval, token);

                var now = Environment.TickCount64;
                var bgIndex = BgFrameIndex();
                var refresh = _lastFrame is null || now - _lastContentUpdate >= interval || bgIndex != _lastBgIndex;

                var (panelW, panelH) = CoolerPanelSize();
                BitmapSource frame;
                if (refresh)
                {
                    EnsureBackgroundSize();
                    var (sceneW, sceneH) = SceneSize();

                    // Содержимое не менялось (те же значения и расстановка) — шлём прошлый кадр без перерисовки.
                    var signature = $"{panelW}x{panelH}|{bgIndex}|" + SceneSignature();
                    if (_lastFrame is not null && signature == _frameSignature)
                    {
                        frame = _lastFrame;
                    }
                    else
                    {
                        frame = LcdRender.Rotate(
                            LcdScene.Render(sceneW, sceneH, _layout, _snapshot, DateTime.Now, CurrentBackground(), Brightness(), _bgTop, _bgBottom),
                            CoolerRotation());
                        _lastFrame = frame;
                        _frameSignature = signature;
                    }

                    _lastContentUpdate = now;
                    _lastBgIndex = bgIndex;
                    RefreshCoolerLog();
                }
                else
                {
                    frame = _lastFrame!;
                }

                await Task.Run(() => lcd.SendBitmap(frame, panelW, panelH), token);
                if (refresh) _sentInWindow++;

                var wait = tick - (int)(Environment.TickCount64 - started);
                if (wait > 0) await Task.Delay(wait, token);
            }
        }
        catch (OperationCanceledException)
        {
            // остановлено пользователем
        }
        catch (Exception ex)
        {
            CoolerInfo.Text = $"Ошибка отправки на экран помпы: {ex.Message}";
            PumpLog.Write($"ошибка отправки: {ex.Message}");
            DropCooler();
        }
        finally
        {
            _coolerActive = false;
            UpdateCoolerButtons();
        }
    }

    private async Task RunImageLoopAsync(CancellationToken token)
    {
        try
        {
            var frames = _coolerFrames;
            if (frames is null || frames.Count == 0) return;

            var (width, height) = CoolerPanelSize();

            if (_cooler is { } device) await Task.Run(() => device.Reinit(), token);
            PumpLog.Write($"показ картинки запущен: {frames.Count} кадр., окно {(WindowShown ? "на экране" : "в трее")}");
            RefreshCoolerLog();

            while (!token.IsCancellationRequested)
            {
                foreach (var frame in frames)
                {
                    var started = Environment.TickCount64;

                    var lcd = _cooler;
                    if (lcd is null) return;

                    // Одна статичная картинка: подтверждаем раз в 0,5 с, чтобы экран не погас.
                    var delay = frames.Count == 1 ? 500 : frame.DelayMs;

                    await ReArmBeforeFrameAsync(lcd, delay, token);

                    var jpeg = frame.Jpeg;
                    await Task.Run(() => lcd.SendJpeg(jpeg, width, height), token);
                    _sentInWindow++;
                    RefreshCoolerLog();

                    var wait = delay - (int)(Environment.TickCount64 - started);
                    if (wait > 0) await Task.Delay(wait, token);

                    token.ThrowIfCancellationRequested();
                }
            }
        }
        catch (OperationCanceledException)
        {
            // остановлено пользователем
        }
        catch (Exception ex)
        {
            CoolerInfo.Text = $"Ошибка отправки на экран помпы: {ex.Message}";
            PumpLog.Write($"ошибка отправки: {ex.Message}");
            DropCooler();
        }
        finally
        {
            _coolerActive = false;
            UpdateCoolerButtons();
        }
    }

    private void CoolerShutdown()
    {
        _tick?.Stop();
        _watchdog?.Stop();
        _saveTimer?.Stop();
        SystemEvents.PowerModeChanged -= OnPumpPowerMode;

        try { _coolerCts?.Cancel(); } catch { /* закрываемся */ }

        _cooler?.Dispose();
        _cooler = null;

        _telemetry?.Dispose();
        _telemetry = null;
    }
}
