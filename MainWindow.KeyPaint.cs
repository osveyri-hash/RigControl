using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Shapes;
using System.Windows.Media.Effects;
using System.Windows.Media.Imaging;
using System.Windows.Threading;

namespace RigControl;

/// <summary>Вкладка «Раскраска»: свой цвет на каждую клавишу, рисуем мышью по схеме.</summary>
public partial class MainWindow
{
    private static readonly string[] PaintPalette =
    {
        "#FF0000", "#FF0050", "#FF2D9A", "#FF00FF", "#C000FF", "#8000FF", "#0000FF",
        "#00B4FF", "#00FFD0", "#00FF40", "#B4FF00", "#FFC800", "#FF6400", "#FFFFFF"
    };

    private static readonly Brush KeyIdleBorder = Frozen(Color.FromRgb(0x47, 0x4C, 0x55));
    private static readonly Brush KeyLabelDark = Frozen(Color.FromRgb(0x22, 0x22, 0x22));
    private static readonly Brush KeyLabelLight = Frozen(Colors.White);

    private readonly Dictionary<byte, Border> _keyCaps = new();
    private string _paintColor = "#FF7EB6";
    private bool _painting;
    private bool _paintErasing;
    private bool _paintColorLoading;
    private DispatcherTimer? _paintSaveTimer;

    private DispatcherTimer? _paintSendTimer;
    private bool _paintSending;
    private bool _paintPending;

    private static SolidColorBrush Frozen(Color color)
    {
        var brush = new SolidColorBrush(color);
        brush.Freeze();
        return brush;
    }

    /// <summary>Копия раскраски для потока кадров анимации (читается из фонового потока).</summary>
    private volatile Dictionary<string, string>? _paintSnapshot;

    private Dictionary<string, string> PaintMap => _settings.KeyPaint ??= new Dictionary<string, string>();

    // ── Экранчик на схеме: показывает картинку/гифку с вкладки «Экран» ──
    private Image? _miniScreen;
    // Кадры не копируем в картинки: один WriteableBitmap, в него пишется текущий кадр из _frames.
    private List<ScreenFrame>? _miniFrames;
    private WriteableBitmap? _miniBitmap;
    private DispatcherTimer? _miniTimer;
    private int _miniIndex;

    /// <summary>Колёсико громкости: рифлёный ободок и гладкая шайба, как на клавиатуре.</summary>
    private void SetupKnob()
    {
        var r = KeyPaint.KnobRadius;
        var size = r * 2;
        var knob = new Grid { Width = size, Height = size, IsHitTestVisible = false };

        knob.Children.Add(new Ellipse
        {
            Fill = new RadialGradientBrush(Color.FromRgb(0x3C, 0x3C, 0x40), Color.FromRgb(0x0C, 0x0C, 0x0E)),
            Stroke = Frozen(Color.FromRgb(0x05, 0x05, 0x05)),
            StrokeThickness = 1
        });

        // Насечки по краю
        var ticks = new StreamGeometry();
        using (var ctx = ticks.Open())
        {
            for (var i = 0; i < 56; i++)
            {
                var a = i * Math.PI * 2 / 56;
                var (cos, sin) = (Math.Cos(a), Math.Sin(a));
                ctx.BeginFigure(new Point(r + cos * (r - 1.2), r + sin * (r - 1.2)), false, false);
                ctx.LineTo(new Point(r + cos * (r - 4.2), r + sin * (r - 4.2)), true, false);
            }
        }
        ticks.Freeze();
        knob.Children.Add(new Path { Data = ticks, Stroke = Frozen(Color.FromRgb(0x6A, 0x6A, 0x70)), StrokeThickness = 0.9 });

        var cap = r * 2 - 11;
        knob.Children.Add(new Ellipse
        {
            Width = cap,
            Height = cap,
            Fill = new LinearGradientBrush(Color.FromRgb(0x34, 0x34, 0x38), Color.FromRgb(0x12, 0x12, 0x14), 60),
            Stroke = Frozen(Color.FromRgb(0x4A, 0x4A, 0x50)),
            StrokeThickness = 0.8
        });
        knob.Children.Add(new Ellipse
        {
            Width = cap * 0.55,
            Height = cap * 0.22,
            Margin = new Thickness(0, 0, cap * 0.18, cap * 0.42),
            Fill = Frozen(Color.FromArgb(0x26, 0xFF, 0xFF, 0xFF))
        });

        Canvas.SetLeft(knob, KeyPaint.KnobX - r - KeyPaint.OriginX);
        Canvas.SetTop(knob, KeyPaint.KnobY - r - KeyPaint.OriginY);
        PaintCanvas.Children.Add(knob);
    }

    private void SetupMiniScreen()
    {
        _miniScreen = new Image
        {
            Width = KeyPaint.ScreenSize - 4,
            Height = KeyPaint.ScreenSize - 4,
            Stretch = Stretch.UniformToFill,
            IsHitTestVisible = false
        };
        RenderOptions.SetBitmapScalingMode(_miniScreen, BitmapScalingMode.HighQuality);

        var bezel = new Border
        {
            Width = KeyPaint.ScreenSize,
            Height = KeyPaint.ScreenSize,
            CornerRadius = new CornerRadius(3),
            Background = Frozen(Color.FromRgb(0x05, 0x05, 0x08)),
            BorderBrush = Frozen(Color.FromRgb(0x3A, 0x3A, 0x3A)),
            BorderThickness = new Thickness(1),
            Padding = new Thickness(1),
            Child = _miniScreen,
            IsHitTestVisible = false
        };
        Canvas.SetLeft(bezel, KeyPaint.ScreenX - KeyPaint.OriginX);
        Canvas.SetTop(bezel, KeyPaint.ScreenY - KeyPaint.OriginY);
        PaintCanvas.Children.Add(bezel);

        // IsVisible учитывает и вкладку, и свёрнутое в трей окно.
        KbdPaintPanel.IsVisibleChanged += (_, _) => UpdateMiniScreenTimer();
        UpdateMiniScreen();
    }

    /// <summary>Экранчик показывает картинку/гифку, загруженную на вкладке «Экран».</summary>
    private void UpdateMiniScreen()
    {
        if (_miniScreen is null) return;
        _miniTimer?.Stop();
        _miniIndex = 0;
        _miniFrames = _frames is { Count: > 0 } frames ? frames : null;

        if (_miniFrames is null)
        {
            _miniScreen.Source = null;
            _miniBitmap = null;
            return;
        }

        var size = Ak820Protocol.ScreenSize;
        _miniBitmap ??= new WriteableBitmap(size, size, 96, 96, PixelFormats.Bgr565, null);
        DrawMiniFrame();
        _miniScreen.Source = _miniBitmap;
        UpdateMiniScreenTimer();
    }

    private void DrawMiniFrame()
    {
        if (_miniFrames is not { } frames || _miniBitmap is null || _miniIndex >= frames.Count) return;

        var size = Ak820Protocol.ScreenSize;
        var pixels = frames[_miniIndex].Rgb565;
        if (pixels.Length < size * size * 2) return;
        _miniBitmap.WritePixels(new Int32Rect(0, 0, size, size), pixels, size * 2, 0);
    }

    private int MiniDelay() =>
        _miniFrames is { } frames && _miniIndex < frames.Count ? Math.Max(20, frames[_miniIndex].DelayMs) : 100;

    /// <summary>Гифка крутится только пока открыта «Раскраска» и окно видно.</summary>
    private void UpdateMiniScreenTimer()
    {
        var run = _miniFrames is { Count: > 1 } && KbdPaintPanel is { IsVisible: true };
        if (!run)
        {
            _miniTimer?.Stop();
            return;
        }

        if (_miniTimer is null)
        {
            _miniTimer = new DispatcherTimer(DispatcherPriority.Render);
            _miniTimer.Tick += (_, _) =>
            {
                if (_miniFrames is not { Count: > 1 } frames || _miniScreen is null) { _miniTimer!.Stop(); return; }
                _miniIndex = (_miniIndex + 1) % frames.Count;
                DrawMiniFrame();
                _miniTimer!.Interval = TimeSpan.FromMilliseconds(MiniDelay());
            };
        }

        if (_miniTimer.IsEnabled) return;
        _miniTimer.Interval = TimeSpan.FromMilliseconds(MiniDelay());
        _miniTimer.Start();
    }

    private void SetupKeyPaintUi()
    {
        foreach (var key in KeyPaint.Keys)
        {
            var label = new TextBlock
            {
                Text = key.Label,
                FontSize = key.Label.Length > 3 ? 8.5 : 10,
                FontWeight = FontWeights.Bold,
                HorizontalAlignment = HorizontalAlignment.Center,
                VerticalAlignment = VerticalAlignment.Center,
                IsHitTestVisible = false
            };

            var cap = new Border
            {
                Width = key.W + 6,
                Height = key.H + 6,
                CornerRadius = new CornerRadius(5),
                BorderThickness = new Thickness(1),
                Child = label,
                Tag = key,
                ToolTip = key.Label
            };

            Canvas.SetLeft(cap, key.X - KeyPaint.OriginX - 3);
            Canvas.SetTop(cap, key.Y - KeyPaint.OriginY - 3);
            RenderOptions.SetBitmapScalingMode(cap, BitmapScalingMode.HighQuality);
            PaintCanvas.Children.Add(cap);
            _keyCaps[key.Code] = cap;
        }

        SetupMiniScreen();
        SetupKnob();

        foreach (var hex in PaintPalette)
        {
            var swatch = new Border
            {
                Width = 26,
                Height = 26,
                CornerRadius = new CornerRadius(6),
                Margin = new Thickness(0, 0, 8, 8),
                Background = new SolidColorBrush((Color)ColorConverter.ConvertFromString(hex)),
                BorderBrush = (Brush)FindResource("LineBrush"),
                BorderThickness = new Thickness(2),
                Cursor = Cursors.Hand,
                Tag = hex,
                ToolTip = hex
            };
            swatch.MouseLeftButtonUp += (_, _) => SetPaintColor(hex);
            PaintSwatches.Children.Add(swatch);
        }

        foreach (var (name, color) in KeyPaint.Presets)
        {
            var button = new Button { Content = name, Margin = new Thickness(0, 0, 8, 8), Padding = new Thickness(12, 6, 12, 6) };
            button.Click += (_, _) =>
            {
                PaintMap.Clear();
                foreach (var key in KeyPaint.Keys) PaintMap[key.Code.ToString()] = color(key);
                OnPaintChanged();
            };
            PaintPresets.Children.Add(button);
        }

        _paintSaveTimer = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(600) };
        _paintSaveTimer.Tick += (_, _) =>
        {
            _paintSaveTimer.Stop();
            _settings.Save();
        };

        _paintWheel = new ColorWheel(150);
        _paintWheel.ColorChanged += color => SetPaintColor(ColorWheel.Hex(color), fromWheel: true);
        PaintWheelHost.Content = _paintWheel;

        _paintAnimLoading = true;
        PaintAnimCombo.ItemsSource = KeyPaint.Animations;
        PaintAnimCombo.SelectedIndex = Math.Clamp(_settings.PaintAnim, 0, KeyPaint.Animations.Length - 1);
        PaintSpeedSlider.Value = Math.Clamp(_settings.PaintSpeed, 0, 5);
        PaintSpeedLabel.Text = $"Скорость: {_settings.PaintSpeed}";
        _paintAnimLoading = false;

        SetPaintColor(string.IsNullOrWhiteSpace(_settings.PaintColor) ? _paintColor : _settings.PaintColor);
        RefreshKeyCaps();

        PaintMirrorCheck.IsChecked = _settings.PaintMirror;
        KbdPaintPanel.IsVisibleChanged += (_, _) => UpdatePaintMirrorTimer();
        StateChanged += (_, _) => UpdatePaintMirrorTimer();
        UpdatePaintMirrorHint();
    }

    // ── Цвет кисти ───────────────────────────────────────────────────────────

    private ColorWheel? _paintWheel;
    private bool _paintAnimLoading;

    private void SetPaintColor(string hex, bool fromWheel = false)
    {
        var (r, g, b) = Ak820Protocol.ParseColor(hex, (0xFF, 0x7E, 0xB6));
        _paintColor = $"#{r:X2}{g:X2}{b:X2}";
        _settings.PaintColor = _paintColor;
        if (!fromWheel) _paintWheel?.SetColor(Color.FromRgb(r, g, b));

        PaintColorSwatch.Background = new SolidColorBrush(Color.FromRgb(r, g, b));

        if (!string.Equals(PaintColorBox.Text.Trim(), _paintColor, StringComparison.OrdinalIgnoreCase))
        {
            _paintColorLoading = true;
            PaintColorBox.Text = _paintColor;
            _paintColorLoading = false;
        }

        var accent = (Brush)FindResource("AccentDeepBrush");
        var line = (Brush)FindResource("LineBrush");
        foreach (Border swatch in PaintSwatches.Children)
        {
            swatch.BorderBrush = string.Equals(swatch.Tag as string, _paintColor, StringComparison.OrdinalIgnoreCase)
                ? accent
                : line;
        }
    }

    private void OnPaintAnimChanged(object sender, SelectionChangedEventArgs e)
    {
        if (_paintAnimLoading || PaintAnimCombo.SelectedIndex < 0) return;

        _settings.PaintAnim = PaintAnimCombo.SelectedIndex;
        _paintSaveTimer?.Stop();
        _paintSaveTimer?.Start();

        // Раскраска уже включена — сразу переключаем анимацию.
        if (SelectedMode != LightEffects.Paint || (_device is null && _dongle is null)) return;

        StopLightSync();
        if (_settings.PaintAnim != 0 && _device is not null) StartLightSync();
        else _ = ApplyPaintAsync();
    }

    private void OnPaintSpeedChanged(object sender, RoutedPropertyChangedEventArgs<double> e)
    {
        if (PaintSpeedLabel is null) return;
        var speed = (int)Math.Round(PaintSpeedSlider.Value);
        PaintSpeedLabel.Text = $"Скорость: {speed}";
        if (_paintAnimLoading) return;

        _settings.PaintSpeed = speed;
        _paintSaveTimer?.Stop();
        _paintSaveTimer?.Start();
    }

    private void OnPaintColorTextChanged(object sender, TextChangedEventArgs e)
    {
        if (_paintColorLoading || PaintColorSwatch is null) return;

        var text = PaintColorBox.Text.Trim().TrimStart('#');
        if (text.Length is 3 or 6) SetPaintColor(PaintColorBox.Text);
    }

    // ── Рисование мышью ──────────────────────────────────────────────────────

    private static KeyPaint.KeyCap? KeyAt(Point point)
    {
        var x = point.X + KeyPaint.OriginX;
        var y = point.Y + KeyPaint.OriginY;

        foreach (var key in KeyPaint.Keys)
        {
            if (x >= key.X - 3 && x <= key.X + key.W + 3 && y >= key.Y - 3 && y <= key.Y + key.H + 3)
                return key;
        }

        return null;
    }

    private void OnPaintDown(object sender, MouseButtonEventArgs e)
    {
        _painting = true;
        _paintErasing = Keyboard.Modifiers.HasFlag(ModifierKeys.Shift);
        _paintEditAt = Environment.TickCount64;
        if (_mirrorShown) ShowPaintView();
        PaintCanvas.CaptureMouse();
        PaintAt(e.GetPosition(PaintCanvas));
        e.Handled = true;
    }

    private void OnPaintMove(object sender, MouseEventArgs e)
    {
        if (!_painting || e.LeftButton != MouseButtonState.Pressed) return;
        PaintAt(e.GetPosition(PaintCanvas));
    }

    private void OnPaintUp(object sender, MouseButtonEventArgs e)
    {
        _painting = false;
        _paintEditAt = Environment.TickCount64;
        PaintCanvas.ReleaseMouseCapture();
        RefreshPaintCount();
    }

    private void OnPaintLostCapture(object sender, MouseEventArgs e) => _painting = false;

    /// <summary>ПКМ — пипетка: берём цвет клавиши в кисть (если на схеме подсветка — тот, что горит).</summary>
    private void OnPaintPick(object sender, MouseButtonEventArgs e)
    {
        if (KeyAt(e.GetPosition(PaintCanvas)) is { } key)
        {
            if (_mirrorShown && _keyGlows.TryGetValue(key.Code, out var glow) && glow.Lit == true)
                SetPaintColor(ColorWheel.Hex(glow.Shadow.Color));
            else if (PaintMap.TryGetValue(key.Code.ToString(), out var hex))
                SetPaintColor(hex);
        }

        e.Handled = true;
    }

    private void PaintAt(Point point)
    {
        if (KeyAt(point) is not { } key) return;

        var id = key.Code.ToString();
        if (_paintErasing)
        {
            if (!PaintMap.Remove(id)) return;
        }
        else
        {
            if (PaintMap.TryGetValue(id, out var old) && string.Equals(old, _paintColor, StringComparison.OrdinalIgnoreCase))
                return;

            PaintMap[id] = _paintColor;
        }

        OnPaintChanged(key.Code);
        if (!_painting) RefreshPaintCount();
    }

    private void OnPaintFillAll(object sender, RoutedEventArgs e)
    {
        foreach (var key in KeyPaint.Keys) PaintMap[key.Code.ToString()] = _paintColor;
        OnPaintChanged();
    }

    private void OnPaintClear(object sender, RoutedEventArgs e)
    {
        PaintMap.Clear();
        OnPaintChanged();
    }

    /// <summary>После любой правки: перерисовать схему, обновить живой кадр и сохранить (с задержкой).</summary>
    private void OnPaintChanged(byte? onlyKey = null)
    {
        // Пока рисуете (и ещё 3 секунды после) схема показывает раскраску, а не текущую подсветку.
        _paintEditAt = Environment.TickCount64;
        if (_mirrorShown) ShowPaintView();

        if (onlyKey is { } code) RefreshKeyCap(code);
        else RefreshKeyCaps();

        _paintSaveTimer?.Stop();
        _paintSaveTimer?.Start();

        // Если раскраска сейчас включена — отправляем правки на клавиатуру (с небольшой задержкой,
        // чтобы при рисовании мышью не слать таблицу на каждую клавишу).
        if (SelectedMode == LightEffects.Paint && !_lightOff && (_device is not null || _dongle is not null))
        {
            _paintSendTimer ??= CreatePaintSendTimer();
            _paintSendTimer.Stop();
            _paintSendTimer.Start();
        }
    }

    private void RefreshKeyCaps()
    {
        foreach (var code in _keyCaps.Keys) RefreshKeyCap(code);
        RefreshPaintCount();
    }

    private static BitmapImage? _keyArt;
    private static bool _keyArtFailed;
    private readonly Dictionary<byte, Brush> _keyArtBrushes = new();

    /// <summary>Кусок фото кейкапов под конкретную клавишу (null — если фото нет или клавиши нет на нём).</summary>
    private Brush? KeyArtBrush(byte code)
    {
        if (!KeyPaint.HasArt(code)) return null;
        if (_keyArtBrushes.TryGetValue(code, out var cached)) return cached;
        if (_keyArt is null && !_keyArtFailed)
        {
            try
            {
                var bmp = new BitmapImage();
                bmp.BeginInit();
                bmp.UriSource = new Uri("pack://application:,,,/Assets/KeyArt.png", UriKind.Absolute);
                bmp.CacheOption = BitmapCacheOption.OnLoad;
                // Оригинал ×3: рисунок кейкапов остаётся чётким на HiDPI и при масштабировании.
                bmp.DecodePixelWidth = (int)(KeyPaint.Width * 3);
                bmp.EndInit();
                bmp.Freeze();
                _keyArt = bmp;
            }
            catch
            {
                _keyArtFailed = true;
            }
        }
        if (_keyArt is null) return null;

        foreach (var key in KeyPaint.Keys)
        {
            if (key.Code != code) continue;
            var brush = new ImageBrush(_keyArt)
            {
                ViewboxUnits = BrushMappingMode.RelativeToBoundingBox,
                Viewbox = new Rect(
                    // Чуть внутрь и выше: на фото верх колпачка смещён вверх, а по краям — тени.
                    (key.X - KeyPaint.OriginX - 2) / KeyPaint.Width,
                    (key.Y - KeyPaint.OriginY - 5.5) / KeyPaint.Height,
                    (key.W + 4) / KeyPaint.Width,
                    (key.H + 4) / KeyPaint.Height),
                Stretch = Stretch.Fill
            };
            brush.Freeze();
            _keyArtBrushes[code] = brush;
            return brush;
        }
        return null;
    }

    private void RefreshKeyCap(byte code)
    {
        if (!_keyCaps.TryGetValue(code, out var cap)) return;
        var label = (TextBlock)cap.Child;

        // Колпачок всегда своего цвета (как на настоящей клавиатуре), а подсветка
        // рисуется светящимся ободком вокруг — как свет из-под кейкапа.
        if (KeyArtBrush(code) is { } art)
        {
            // Рисунок с фото уже содержит подписи — свою не выводим.
            cap.Background = art;
            label.Visibility = Visibility.Collapsed;
        }
        else
        {
            var (cr, cg, cb) = Ak820Protocol.ParseColor(KeyPaint.CapColor(code), (0xF7, 0xF6, 0xF2));
            cap.Background = Frozen(Color.FromRgb(cr, cg, cb));
            label.Foreground = 0.299 * cr + 0.587 * cg + 0.114 * cb > 150 ? KeyLabelDark : KeyLabelLight;
            // «→» у пользователя — просто белый колпачок без подписи.
            label.Visibility = code == 102 ? Visibility.Collapsed : Visibility.Visible;
        }

        // Пока схема показывает текущую подсветку, свечением управляет она (MirrorTick).
        if (!_mirrorShown) SetKeyGlow(code, cap, PaintGlow(code));
    }

    /// <summary>Цвет раскраски клавиши (null — не закрашена).</summary>
    private Color? PaintGlow(byte code)
    {
        if (!PaintMap.TryGetValue(code.ToString(), out var hex)) return null;
        var (r, g, b) = Ak820Protocol.ParseColor(hex, (0xFF, 0xFF, 0xFF));
        return Color.FromRgb(r, g, b);
    }

    // ── Схема показывает подсветку, которая горит сейчас ─────────────────────

    /// <summary>Свечение клавиши: кисть ободка и тень меняются на месте — без новых объектов на каждый кадр.</summary>
    private sealed class KeyGlow
    {
        public readonly SolidColorBrush Border = new(Colors.Black);
        public readonly DropShadowEffect Shadow = new() { BlurRadius = 6, ShadowDepth = 0, Opacity = 0.42 };
        public bool? Lit;
    }

    private static readonly Thickness LitBorder = new(1.5), IdleBorder = new(1);

    private readonly Dictionary<byte, KeyGlow> _keyGlows = new();
    private DispatcherTimer? _mirrorTimer;
    private bool _mirrorShown;
    private long _paintEditAt;

    /// <summary>
    /// Ободок и свечение клавиши. <paramref name="border"/> — цвет ободка (как горит),
    /// <paramref name="glow"/> — цвет свечения (по умолчанию тот же), <paramref name="strength"/> — его сила 0…1.
    /// </summary>
    private void SetKeyGlow(byte code, Border cap, Color? border, Color? glow = null, double strength = 1)
    {
        if (!_keyGlows.TryGetValue(code, out var state)) _keyGlows[code] = state = new KeyGlow();

        if (border is not { } color || strength < 0.03)
        {
            if (state.Lit == false) return;
            state.Lit = false;
            cap.BorderBrush = KeyIdleBorder;
            cap.BorderThickness = IdleBorder;
            cap.Effect = null;
            return;
        }

        var shadow = glow ?? color;
        if (state.Border.Color != color) state.Border.Color = color;
        if (state.Shadow.Color != shadow) state.Shadow.Color = shadow;

        var opacity = Math.Round(0.42 * Math.Clamp(strength, 0, 1), 2);
        if (Math.Abs(state.Shadow.Opacity - opacity) > 0.001) state.Shadow.Opacity = opacity;

        if (state.Lit == true) return;
        state.Lit = true;
        cap.BorderBrush = state.Border;
        cap.BorderThickness = LitBorder;
        cap.Effect = state.Shadow;
    }

    private void OnPaintMirrorChanged(object sender, RoutedEventArgs e)
    {
        _settings.PaintMirror = PaintMirrorCheck.IsChecked == true;
        _paintSaveTimer?.Stop();
        _paintSaveTimer?.Start();
        UpdatePaintMirrorTimer();
        UpdatePaintMirrorHint();
    }

    /// <summary>Таймер схемы работает, только пока «Раскраска» открыта и окно видно.</summary>
    private void UpdatePaintMirrorTimer()
    {
        var run = _settings.PaintMirror && KbdPaintPanel is { IsVisible: true } && WindowState != WindowState.Minimized;
        if (!run)
        {
            _mirrorTimer?.Stop();
            ShowPaintView();
            return;
        }

        if (_mirrorTimer is null)
        {
            _mirrorTimer = new DispatcherTimer(DispatcherPriority.Render) { Interval = TimeSpan.FromMilliseconds(50) };
            _mirrorTimer.Tick += (_, _) => MirrorTick();
        }

        if (_mirrorTimer.IsEnabled) return;
        _mirrorTimer.Start();
        MirrorTick();
    }

    /// <summary>Кадр схемы: цвета, которые сейчас горят на клавиатуре (их записывают Ak820Device/Ak820Dongle).</summary>
    private void MirrorTick()
    {
        var connected = _device is not null || _dongle is not null;
        var editing = _painting || Environment.TickCount64 - _paintEditAt < 3000;
        var approximate = false;
        var bank = editing || !connected ? null : LightMirror.Display(out approximate);

        if (bank is null)
        {
            ShowPaintView();
            SetMirrorHint(!connected ? MirrorHint.NoKeyboard : editing ? MirrorHint.Editing : MirrorHint.Unknown);
            return;
        }

        _mirrorShown = true;
        SetMirrorHint(approximate ? MirrorHint.Approximate : MirrorHint.Exact);

        foreach (var (code, cap) in _keyCaps)
        {
            var at = code * 4;
            if (at + 3 >= bank.Length || bank[at] != code)
            {
                SetKeyGlow(code, cap, null);
                continue;
            }

            var (r, g, b) = (bank[at + 1], bank[at + 2], bank[at + 3]);
            var max = Math.Max(r, Math.Max(g, b));
            if (max < 6)
            {
                SetKeyGlow(code, cap, null);
                continue;
            }

            // Свечение — тем же оттенком в полную силу, но прозрачнее у тусклых клавиш.
            var k = 255.0 / max;
            var glow = Color.FromRgb((byte)Math.Min(255, r * k), (byte)Math.Min(255, g * k), (byte)Math.Min(255, b * k));
            SetKeyGlow(code, cap, Color.FromRgb(r, g, b), glow, max / 255.0);
        }
    }

    /// <summary>Вернуть на схему раскраску (рисуем, клавиатура не подключена или показ подсветки выключен).</summary>
    private void ShowPaintView()
    {
        if (!_mirrorShown) return;
        _mirrorShown = false;

        foreach (var (code, cap) in _keyCaps) SetKeyGlow(code, cap, PaintGlow(code));
    }

    private enum MirrorHint { None, Editing, NoKeyboard, Unknown, Exact, Approximate }

    private MirrorHint _mirrorHint = (MirrorHint)(-1);

    /// <summary>Подпись под схемой: что сейчас на ней показано.</summary>
    private void SetMirrorHint(MirrorHint hint)
    {
        if (!_settings.PaintMirror) hint = MirrorHint.None;
        if (hint == _mirrorHint || PaintMirrorHint is null) return;
        _mirrorHint = hint;

        PaintMirrorHint.Visibility = hint == MirrorHint.None ? Visibility.Collapsed : Visibility.Visible;
        PaintMirrorHint.Text = hint switch
        {
            MirrorHint.Editing => "Сейчас на схеме — ваша раскраска. Через 3 секунды после правки снова покажется подсветка.",
            MirrorHint.NoKeyboard => "Клавиатура не подключена — на схеме ваша раскраска.",
            MirrorHint.Unknown => "Подсветку программа ещё не отправляла — на схеме ваша раскраска.",
            MirrorHint.Exact => "Сейчас на схеме — подсветка клавиатуры, в такт с ней.",
            MirrorHint.Approximate => "Сейчас на схеме — подсветка клавиатуры. Встроенные эффекты прошивки показаны примерно.",
            _ => ""
        };
    }

    private void UpdatePaintMirrorHint()
    {
        if (!_settings.PaintMirror) SetMirrorHint(MirrorHint.None);
        else if (_mirrorTimer is { IsEnabled: true }) MirrorTick();
    }

    private void RefreshPaintCount()
    {
        var count = KeyPaint.Keys.Count(k => PaintMap.ContainsKey(k.Code.ToString()));
        PaintCountText.Text = count == 0
            ? "Ничего не раскрашено"
            : $"Раскрашено: {count} из {KeyPaint.Keys.Length}";
    }

    // ── Вывод на клавиатуру ─────────────────────────────────────────────────

    /// <summary>Выбирает эффект «Своя раскраска» и сразу применяет его.</summary>
    private void OnPaintApply(object sender, RoutedEventArgs e)
    {
        var index = Array.FindIndex(Ak820Protocol.Effects, x => x.Mode == LightEffects.Paint);
        if (index < 0) return;

        if (EffectCombo.SelectedIndex != index)
        {
            // Без _loading смена эффекта запустила бы ещё и отложенное применение — свет мигнул бы дважды.
            _loading = true;
            EffectCombo.SelectedIndex = index;
            if (_rainbowAuto)
            {
                RainbowCheck.IsChecked = false;
                _rainbowAuto = false;
            }
            _loading = false;
            UpdateRainbowUi();
        }

        OnApplyEffect(sender, e);
    }

    private DispatcherTimer CreatePaintSendTimer()
    {
        var timer = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(400) };
        timer.Tick += async (_, _) =>
        {
            timer.Stop();
            await ApplyPaintAsync();
        };
        return timer;
    }

    /// <summary>
    /// Отправляет раскраску на клавиатуру (кабель или радио). Если во время отправки её снова
    /// поменяли — после окончания отправит ещё раз уже свежую.
    /// </summary>
    private async Task ApplyPaintAsync(bool showResult = false)
    {
        if (_paintSending)
        {
            _paintPending = true;
            return;
        }

        _paintSending = true;
        try
        {
            do
            {
                _paintPending = false;

                var device = _device;
                var dongle = _dongle;
                if (device is null && dongle is null) return;

                // С анимацией по кабелю раскраску рисует поток кадров — ему достаточно свежей копии цветов.
                if (_settings.PaintAnim != 0 && device is not null && SelectedMode == LightEffects.Paint)
                {
                    _paintSnapshot = new Dictionary<string, string>(PaintMap);
                    if (_lightSyncCts is null) StartLightSync();
                    if (showResult) SetLog($"Раскраска: {KeyPaint.Animations[Math.Clamp(_settings.PaintAnim, 0, KeyPaint.Animations.Length - 1)]}.");
                    return;
                }

                var frame = KeyPaint.BuildFrame(PaintMap, _settings.Brightness);

                try
                {
                    await Task.Run(() =>
                    {
                        if (device is not null) device.SetUserLight(frame);
                        else dongle!.SetUserLight(frame);
                    });

                    if (showResult)
                        SetLog(device is not null ? "Раскраска применена." : "Раскраска применена (по радио).");
                }
                catch (ObjectDisposedException)
                {
                    // переподключились во время отправки — ConnectAsync сам применит свет заново
                }
                catch (Exception ex)
                {
                    SetLog($"Ошибка раскраски: {ex.Message}");
                }
            }
            while (_paintPending);
        }
        finally
        {
            _paintSending = false;
        }
    }
}
