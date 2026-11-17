using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Animation;
using System.Windows.Shapes;
using System.Windows.Threading;
using RigControl.Devices;
using RigControl.Mouse;

namespace RigControl;

/// <summary>Мышь MCHOSE (A7 Ultra): опрос, шапка страницы, вкладки «Производительность» и «Прочее».</summary>
public partial class MainWindow
{
    private DispatcherTimer? _mouseTimer;
    private McMouse? _mouse;
    private MouseStatus? _mouseStatus;
    private MouseConfig? _mouseConfig;
    private string? _mouseFw, _mouseRxFw;
    private string _mouseLog = "";
    private bool _mouseBusy;
    private int _mouseTicks;
    private int _mouseMisses;
    private int _mouseOffline;
    private DateTime _mouseStatusAt;
    private bool _mouseUiUpdating;
    private bool _mouseWarned;
    private DispatcherTimer? _mouseNoteTimer;

    private bool MousePageVisible => PageMouse is { IsVisible: true };
    private bool MouseWireless => _mouse?.ViaDongle == true;
    private bool MouseOnline => _mouse is not null && _mouseStatus is { HasMouse: true };

    private void StartMouseWatch()
    {
        MouseArtHost.Content = DeviceArt.Picture(DeviceArt.Mouse, "\uE962");
        MouseEmptyArt.Content = DeviceArt.Picture(DeviceArt.Mouse, "\uE962");
        InitMouseKeys();
        InitMouseDpi();

        PageMouse.IsVisibleChanged += async (_, _) =>
        {
            if (MousePageVisible) await UpdateMouseAsync(force: true);
            else StopRateTest();
        };

        _mouseTimer = new DispatcherTimer { Interval = TimeSpan.FromSeconds(3) };
        _mouseTimer.Tick += async (_, _) => await UpdateMouseAsync(force: false);
        _mouseTimer.Start();
        _ = UpdateMouseAsync(force: true);
    }

    private void StopMouseWatch()
    {
        _mouseTimer?.Stop();
        StopRateTest();
        try { _mouse?.Dispose(); } catch { }
        _mouse = null;
    }

    private async Task UpdateMouseAsync(bool force)
    {
        if (_mouseBusy) return;
        _mouseBusy = true;
        try
        {
            _mouseTicks++;
            var watching = MousePageVisible || DevicesPageVisible;

            if (_mouse is null)
            {
                // Ищем раз в ~12 секунд, а когда страница открыта — каждые 3.
                if (!force && !watching && _mouseTicks % 4 != 0) return;

                var (found, log) = await Task.Run(() =>
                {
                    var m = McMouse.Find(out var l);
                    return (m, l);
                });
                _mouseLog = log;
                _mouse = found;
                _mouseMisses = 0;
                _mouseStatus = null;
                ForgetMouseState();
                if (_mouse is null)
                {
                    ShowMouse();
                    return;
                }
            }

            var mouse = _mouse;
            var needStatus = force || watching || DateTime.UtcNow - _mouseStatusAt > TimeSpan.FromSeconds(30);
            var needConfig = MousePageVisible || (force && _mouseConfig is null);
            if (!needStatus && !needConfig) return;
            var needVersions = MousePageVisible && _mouseFw is null;

            var (status, config, fw, rx, error) = await Task.Run(() =>
            {
                try
                {
                    var st = mouse.ReadStatus();
                    var online = st is { HasMouse: true };
                    var cfg = needConfig && online ? mouse.ReadConfig() : null;
                    string? f = null, r = null;
                    if (needVersions && online)
                    {
                        f = mouse.ReadVersion(receiver: false);
                        if (mouse.ViaDongle) r = mouse.ReadVersion(receiver: true);
                    }
                    return (st, cfg, f, r, (string?)null);
                }
                catch (Exception ex)
                {
                    return ((MouseStatus?)null, (MouseConfig?)null, (string?)null, (string?)null, ex.Message);
                }
            });

            if (!ReferenceEquals(mouse, _mouse)) return;

            if (status is null)
            {
                // Нет ответа: часто это просто M HUB или донгл заняты — держим прошлое показание.
                // Ошибка 3 раза подряд (донгл вынули) или молчит ~30 секунд — ищем заново.
                _mouseMisses++;
                if ((error is not null && _mouseMisses >= 3) || _mouseMisses >= 10)
                {
                    _mouseLog += $"\nПотеряна связь: {error ?? "нет ответа"}";
                    try { mouse.Dispose(); } catch { }
                    _mouse = null;
                    _mouseStatus = null;
                    ForgetMouseState();
                    _mouseOffline = 0;
                }
                ShowMouse();
                return;
            }

            _mouseMisses = 0;
            _mouseStatusAt = DateTime.UtcNow;

            if (!status.HasMouse && _mouseStatus is { HasMouse: true } && ++_mouseOffline < 3)
            {
                // Донгл разок ответил «мыши нет» — не мигаем, ждём ещё двух таких ответов подряд.
                ShowMouse();
                return;
            }

            if (status.HasMouse) _mouseOffline = 0;
            _mouseStatus = status;
            if (fw is not null || needVersions) _mouseFw = fw ?? FormatFirmware(status.Firmware);
            if (rx is not null) _mouseRxFw = rx;
            if (!status.HasMouse) ForgetMouseState();
            else if (config is { Valid: true }) _mouseConfig = config;
            else if (config is not null) _mouseLog = $"Непонятные настройки: {config.Raw}";
            ShowMouse();
        }
        finally
        {
            _mouseBusy = false;
        }
    }

    private void ForgetMouseState()
    {
        _mouseConfig = null;
        _mouseFw = null;
        _mouseRxFw = null;
        _dpiSeparate = false;
    }

    private int MouseCurrentLevel(MouseConfig c) => Math.Clamp(MouseWireless ? c.GDpi : c.UsbDpi, 0, c.Count - 1);

    private string MouseModelName
    {
        get
        {
            if (_mouse is null) return "Мышь";
            var model = _mouse.Model(_mouseStatus);
            return model.StartsWith("Мышь", StringComparison.Ordinal) ? model : "MCHOSE " + model;
        }
    }

    private void ShowMouse()
    {
        var mouse = _mouse;
        var st = _mouseStatus;
        var cfg = _mouseConfig;
        var online = MouseOnline;

        // Боковая панель
        MouseSideDot.SetResourceReference(Shape.FillProperty, online ? "GoodBrush" : "BadBrush");
        MouseSideText.Text = mouse is null ? "Не найдена" : !online ? "Не на связи" : mouse.Model(st);
        MouseSideBattery.Visibility = online ? Visibility.Visible : Visibility.Collapsed;
        if (online)
        {
            MouseSidePercent.Text = st!.Charging ? $"{st.Battery}%⚡" : $"{st.Battery}%";
            MouseSideFill.Width = Math.Max(1.5, 18 * st.Battery / 100.0);
            MouseSideFill.SetResourceReference(Border.BackgroundProperty, st.Battery <= BatteryLowPercent ? "BadBrush" : "GoodBrush");

            if (st.Battery <= BatteryLowPercent && !st.Charging && !_mouseWarned)
            {
                _mouseWarned = true;
                _tray?.ShowBalloon("Мышь садится", $"Заряд {st.Battery} % — пора подключить кабель.");
            }
            else if (st.Battery > BatteryLowPercent + 10 || st.Charging) _mouseWarned = false;
        }

        // Мыши нет — одна понятная карточка вместо пустых настроек.
        var page = online ? Visibility.Visible : Visibility.Collapsed;
        MouseHeader.Visibility = MouseTabs.Visibility = MouseBody.Visibility = page;
        MouseEmpty.Visibility = online ? Visibility.Collapsed : Visibility.Visible;
        MouseEmptyTitle.Text = mouse is null ? "Мышь не найдена" : "Мышь не на связи";
        MouseEmptyText.Text = mouse is null
            ? "Подключите донгл 2.4G или кабель. Если открыт M HUB — закройте его: он занимает мышь."
            : "Донгл на месте, а мышь молчит: включите её (переключатель снизу — 2.4G) или пошевелите.";

        if (online)
        {
            MouseName.Text = MouseModelName;
            MouseBatteryText.Text = $"{st!.Battery} %";
            MouseBatteryFill.Width = Math.Max(2, 24 * st.Battery / 100.0);
            MouseBatteryFill.SetResourceReference(Border.BackgroundProperty, st.Battery <= BatteryLowPercent ? "BadBrush" : "GoodBrush");
            MouseChargeText.Visibility = st.Charging ? Visibility.Visible : Visibility.Collapsed;
            var link = MouseWireless ? "2.4G" : "Кабель";
            MouseLinkText.Text = cfg is null ? link : $"{link}   ·   {MouseDpiText(cfg)} DPI   ·   {MouseRateHz(cfg)} Гц";
            if (cfg is null)
            {
                if (_mouseLog.StartsWith("Непонятные", StringComparison.Ordinal))
                    MouseNote("Мышь ответила непонятно — закройте M HUB и нажмите «Найти мышь заново» во вкладке «Прочее».", error: true);
                else if (_mouseNoteText is null) MouseNote(ReadingNote);
            }
            else if (_mouseNoteText == ReadingNote) MouseNote(null);
        }

        var ready = online && cfg is not null;
        MouseKeysPanel.IsEnabled = MouseDpiPanel.IsEnabled = MousePerfPanel.IsEnabled = ready;
        MouseFactoryButton.IsEnabled = ready;
        _mouseUiUpdating = true;
        try
        {
            if (cfg is not null && FindName($"MouseProfile{Math.Clamp(cfg.Profile, 0, 2)}") is RadioButton profile)
                profile.IsChecked = true;
        }
        finally
        {
            _mouseUiUpdating = false;
        }

        DrawMouseKeys(cfg);
        DrawMouseDpi(cfg);
        DrawMouseRates(cfg);
        DrawMousePerformance(cfg);
        DrawMouseAbout();

        if (MouseDiag.Visibility == Visibility.Visible) MouseDiag.Text = MouseDiagText();
        UpdateDeviceTiles();
    }

    private string MouseDpiText(MouseConfig cfg)
    {
        var level = MouseCurrentLevel(cfg);
        return cfg.SeparateY ? $"{cfg.Dpi[level]}×{cfg.DpiY[level]}" : $"{cfg.Dpi[level]}";
    }

    private static string FormatFirmware(uint fw) => fw == 0 ? "—" : $"{fw:X}";

    private int? MouseRateHz(MouseConfig cfg)
    {
        var rates = _mouse?.Rates ?? McMouse.Rates8K;
        var index = MouseWireless ? cfg.GRate : cfg.UsbRate;
        return index >= 0 && index < rates.Length ? rates[index] : null;
    }

    private const string ReadingNote = "Читаю настройки…";
    private string? _mouseNoteText;

    /// <summary>
    /// Всплывающее сообщение внизу страницы мыши: «Сохранено», ошибки записи и т. п. Обычные гаснут через 2,5 с,
    /// ошибки — через 6 с, «Читаю настройки…» — когда настройки прочитаны.
    /// </summary>
    private void MouseNote(string? text, bool error = false)
    {
        _mouseNoteTimer?.Stop();
        _mouseNoteText = string.IsNullOrEmpty(text) ? null : text;
        if (_mouseNoteText is null)
        {
            if (MouseToast.Visibility != Visibility.Visible) return;
            var fade = new DoubleAnimation(0, TimeSpan.FromMilliseconds(180));
            fade.Completed += (_, _) =>
            {
                if (_mouseNoteText is null) MouseToast.Visibility = Visibility.Collapsed;
            };
            MouseToast.BeginAnimation(OpacityProperty, fade);
            return;
        }

        MouseToastText.Text = _mouseNoteText;
        MouseToast.SetResourceReference(Border.BackgroundProperty, error ? "BadBrush" : "TextBrush");
        if (MouseToast.Visibility != Visibility.Visible || MouseToast.Opacity < 1)
        {
            MouseToast.Visibility = Visibility.Visible;
            MouseToast.BeginAnimation(OpacityProperty, new DoubleAnimation(1, TimeSpan.FromMilliseconds(140)));
        }
        if (text == ReadingNote) return;

        _mouseNoteTimer ??= new DispatcherTimer();
        _mouseNoteTimer.Interval = TimeSpan.FromSeconds(error ? 6 : 2.5);
        _mouseNoteTimer.Tick -= OnMouseNoteTimer;
        _mouseNoteTimer.Tick += OnMouseNoteTimer;
        _mouseNoteTimer.Start();
    }

    private void OnMouseNoteTimer(object? sender, EventArgs e) => MouseNote(null);

    private string MouseDiagText()
    {
        var lines = new List<string>();
        if (_mouse is not null) lines.Add($"Интерфейс: {_mouse.Interface} «{_mouse.Name}»");
        if (_mouseStatus is { } st)
            lines.Add($"Статус: VID {st.Vid:X4} PID {st.Pid:X4} fw {st.Firmware:X8} mode {st.ConnectMode} online {st.Online} " +
                      $"заряд {st.Battery} заряжается {st.Charge} · {st.Raw}");
        if (_mouseConfig is { } c)
            lines.Add($"Профиль {c.Profile + 1} · 2.4G уровень {c.GDpi} частота {c.GRate} · USB уровень {c.UsbDpi} частота {c.UsbRate} · " +
                      $"DPI {string.Join("/", c.Dpi.Take(c.Count))} · кнопки " +
                      string.Join(" ", c.Keys.Take(MouseKeys.Count).Select(k => $"{k.Type}:{k.Value:X6}")) + $" · {c.Raw}");
        if (_mouseLog.Length > 0) lines.Add("Поиск:\n" + _mouseLog);
        return string.Join("\n", lines);
    }

    // ── Отправка в мышь ──

    /// <summary>Отправка в мышь в фоне, затем — перечитать настройки (показываем то, что мышь реально приняла).</summary>
    private async Task MouseSendAsync(Action<McMouse> write, string? done = "Сохранено в мышь", int settleMs = 150)
    {
        if (_mouse is not { } mouse) return;
        var error = await Task.Run(() =>
        {
            try
            {
                write(mouse);
                return null;
            }
            catch (Exception ex) { return ex.Message; }
        });
        if (error is not null) MouseNote($"Не удалось записать в мышь: {error}", error: true);
        else if (done is not null) MouseNote("✓ " + done);
        await RereadMouseConfigAsync(settleMs);
    }

    private DispatcherTimer? _mouseWriteTimer;
    private Action<McMouse>? _mousePendingWrite;
    private DateTime _mouseHoldUi;   // до этого времени ползунки не перерисовываем (пользователь крутит)

    private bool MouseHoldUi => DateTime.UtcNow < _mouseHoldUi || _mousePendingWrite is not null;

    /// <summary>Для ползунков: пишем, когда перестали двигать (через 0,4 с).</summary>
    private void MouseSendLater(Action<McMouse> write)
    {
        _mouseHoldUi = DateTime.UtcNow.AddSeconds(2);
        _mousePendingWrite = write;
        if (_mouseWriteTimer is null)
        {
            _mouseWriteTimer = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(400) };
            _mouseWriteTimer.Tick += async (_, _) =>
            {
                _mouseWriteTimer!.Stop();
                var pending = _mousePendingWrite;
                _mousePendingWrite = null;
                if (pending is null) return;
                await MouseSendAsync(pending);
                _mouseHoldUi = DateTime.UtcNow.AddSeconds(1);
            };
        }
        _mouseWriteTimer.Stop();
        _mouseWriteTimer.Start();
    }

    private async Task RereadMouseConfigAsync(int settleMs = 150)
    {
        for (var i = 0; i < 10 && _mouseBusy; i++) await Task.Delay(100);
        await Task.Delay(settleMs);
        await UpdateMouseAsync(force: true);
    }

    /// <summary>Опасные кнопки срабатывают со второго нажатия: первое меняет надпись на «Точно?» на 4 секунды.</summary>
    private static bool ConfirmTwice(Button button, string question)
    {
        if (button.Tag is DispatcherTimer armed)
        {
            armed.Stop();
            button.Content = armed.Tag;
            button.Tag = null;
            return true;
        }

        var timer = new DispatcherTimer { Interval = TimeSpan.FromSeconds(4), Tag = button.Content };
        timer.Tick += (_, _) =>
        {
            timer.Stop();
            button.Content = timer.Tag;
            button.Tag = null;
        };
        button.Tag = timer;
        button.Content = question;
        timer.Start();
        return false;
    }

    // ── Шапка и вкладки ──

    private async void OnMouseProfile(object sender, RoutedEventArgs e)
    {
        if (_mouseUiUpdating || sender is not RadioButton { Tag: string tag } || _mouseConfig is not { } cfg) return;
        var index = int.Parse(tag);
        if (index == cfg.Profile) return;
        _dpiSeparate = false;
        await MouseSendAsync(m => m.SwitchProfile(index), $"Профиль {index + 1}", settleMs: 300);
    }

    private void OnMouseTab(object sender, RoutedEventArgs e)
    {
        if (MouseKeysPanel is null || MouseDpiPanel is null || MousePerfPanel is null || MouseMorePanel is null) return;
        MouseKeysPanel.Visibility = MouseTabKeys.IsChecked == true ? Visibility.Visible : Visibility.Collapsed;
        MouseDpiPanel.Visibility = MouseTabDpi.IsChecked == true ? Visibility.Visible : Visibility.Collapsed;
        MousePerfPanel.Visibility = MouseTabPerf.IsChecked == true ? Visibility.Visible : Visibility.Collapsed;
        MouseMorePanel.Visibility = MouseTabMore.IsChecked == true ? Visibility.Visible : Visibility.Collapsed;
        if (MouseTabPerf.IsChecked != true) StopRateTest();
        DrawMouseAbout();
    }

    // ── Производительность, сенсор, питание (как вкладка «Производительность» в M HUB) ──

    private void DrawMouseRates(MouseConfig? cfg)
    {
        var rates = _mouse?.Rates ?? McMouse.Rates8K;
        _mouseUiUpdating = true;
        try
        {
            if (!ReferenceEquals(MouseRates.Tag, rates))
            {
                MouseRates.Children.Clear();
                MouseRates.Tag = rates;
                for (var i = 0; i < rates.Length; i++)
                {
                    var button = new RadioButton
                    {
                        Style = (Style)FindResource("SegButton"), GroupName = "MouseRate", Tag = i, Content = rates[i].ToString(),
                    };
                    button.Checked += OnMouseRate;
                    MouseRates.Children.Add(button);
                }
            }

            var index = cfg is null ? -1 : MouseWireless ? cfg.GRate : cfg.UsbRate;
            for (var i = 0; i < MouseRates.Children.Count; i++)
                ((RadioButton)MouseRates.Children[i]).IsChecked = i == index;

            MouseRateNote.Text = MouseWireless && rates.Length == McMouse.Rates1K.Length
                ? "Через этот донгл — до 1000 Гц."
                : "Выше 1000 Гц — плавнее, но больше нагрузка на ПК и быстрее садится батарея.";
        }
        finally
        {
            _mouseUiUpdating = false;
        }
    }

    private async void OnMouseRate(object sender, RoutedEventArgs e)
    {
        if (_mouseUiUpdating || sender is not RadioButton { Tag: int index } || _mouseConfig is null) return;
        var wireless = MouseWireless;
        var hz = (_mouse?.Rates ?? McMouse.Rates8K)[index];
        await MouseSendAsync(m => m.WriteRate(index, wireless), $"Частота опроса {hz} Гц");
    }

    private void DrawMousePerformance(MouseConfig? cfg)
    {
        if (cfg is null) return;
        _mouseUiUpdating = true;
        try
        {
            ((RadioButton)FindName($"MouseMode{cfg.Mode}")).IsChecked = true;
            ((RadioButton)FindName($"MouseLod{cfg.Lod}")).IsChecked = true;
            MouseMotionSync.IsChecked = cfg.MotionSync;
            MouseRipple.IsChecked = cfg.Ripple;
            MouseAngleSnap.IsChecked = cfg.AngleSnap;
            MouseNeverSleep.IsChecked = cfg.Sleep == 0;
            MouseSleep.IsEnabled = cfg.Sleep != 0;

            if (!MouseHoldUi)
            {
                if (!MouseDebounce.IsMouseCaptureWithin) MouseDebounce.Value = Math.Clamp(cfg.Debounce, 0, 20);
                if (!MouseRotate.IsMouseCaptureWithin) MouseRotate.Value = Math.Clamp(cfg.Rotate, -30, 30);
                if (!MouseSleep.IsMouseCaptureWithin && cfg.Sleep != 0) MouseSleep.Value = Math.Clamp(cfg.Sleep, 1, 60);
            }
            UpdateMousePerfLabels();
        }
        finally
        {
            _mouseUiUpdating = false;
        }
    }

    private void UpdateMousePerfLabels()
    {
        MouseDebounceText.Text = $"{MouseDebounce.Value:0}";
        MouseRotateText.Text = $"{MouseRotate.Value:0}°";
        MouseRotateReset.Visibility = Math.Round(MouseRotate.Value) == 0 ? Visibility.Hidden : Visibility.Visible;
        MouseSleepText.Text = MouseNeverSleep.IsChecked == true ? "—" : $"{MouseSleep.Value:0}";
    }

    private int MouseLodNow => _mouseConfig?.Lod ?? 0;

    private async void OnMouseMode(object sender, RoutedEventArgs e)
    {
        if (_mouseUiUpdating || sender is not FrameworkElement { Tag: string tag } || _mouseConfig is null) return;
        var mode = int.Parse(tag) + 1;
        var lod = MouseLodNow;
        await MouseSendAsync(m => m.WriteSensor(lod, mode: mode));
    }

    private async void OnMouseLod(object sender, RoutedEventArgs e)
    {
        if (_mouseUiUpdating || sender is not FrameworkElement { Tag: string tag } || _mouseConfig is null) return;
        var lod = int.Parse(tag);
        await MouseSendAsync(m => m.WriteSensor(lod));
    }

    private async void OnMouseToggle(object sender, RoutedEventArgs e)
    {
        if (_mouseUiUpdating || sender is not CheckBox { Tag: string what } box || _mouseConfig is null) return;
        var value = box.IsChecked == true ? 1 : 2;
        var lod = MouseLodNow;
        await MouseSendAsync(m => m.WriteSensor(lod,
            ripple: what == "ripple" ? value : 0,
            line: what == "line" ? value : 0,
            motionSync: what == "motionSync" ? value : 0));
    }

    private void OnMouseDebounce(object sender, RoutedPropertyChangedEventArgs<double> e)
    {
        if (MouseDebounceText is null) return;
        UpdateMousePerfLabels();
        if (_mouseUiUpdating || _mouseConfig is null) return;
        var ms = (int)Math.Round(MouseDebounce.Value);
        MouseSendLater(m => m.WriteDebounce(ms));
    }

    private void OnMouseRotate(object sender, RoutedPropertyChangedEventArgs<double> e)
    {
        if (MouseRotateText is null) return;
        UpdateMousePerfLabels();
        if (_mouseUiUpdating || _mouseConfig is null) return;
        var angle = (int)Math.Round(MouseRotate.Value);
        var lod = MouseLodNow;
        MouseSendLater(m => m.WriteSensor(lod, rotate: angle));
    }

    private void OnMouseRotateReset(object sender, RoutedEventArgs e) => MouseRotate.Value = 0;

    private void OnMouseSleep(object sender, RoutedPropertyChangedEventArgs<double> e)
    {
        if (MouseSleepText is null) return;
        UpdateMousePerfLabels();
        if (_mouseUiUpdating || _mouseConfig is null || MouseNeverSleep.IsChecked == true) return;
        var minutes = (int)Math.Round(MouseSleep.Value);
        MouseSendLater(m => m.WriteSleep(minutes));
    }

    private async void OnMouseNeverSleep(object sender, RoutedEventArgs e)
    {
        if (_mouseConfig is null) return;
        var never = MouseNeverSleep.IsChecked == true;
        MouseSleep.IsEnabled = !never;
        UpdateMousePerfLabels();
        var minutes = never ? 0 : Math.Max(1, (int)Math.Round(MouseSleep.Value));
        await MouseSendAsync(m => m.WriteSleep(minutes), never ? "Мышь не будет засыпать" : $"Сон через {minutes} мин");
    }

    // ── Прочее ──

    private void DrawMouseAbout()
    {
        if (!MousePageVisible || MouseTabMore.IsChecked != true) return;
        var st = _mouseStatus;
        var rows = new List<(string, string)>
        {
            ("Модель", MouseModelName),
            ("Подключение", MouseWireless ? "Донгл 2.4G" : "Кабель USB"),
            ("Версия ПО мыши", _mouseFw ?? "—"),
        };
        if (MouseWireless) rows.Add(("Версия ПО приёмника", _mouseRxFw ?? "—"));
        if (_mouseConfig is { } cfg) rows.Add(("Профиль в мыши", $"{cfg.Profile + 1} из 3"));
        if (st is not null) rows.Add(("Заряд", st.Charging ? $"{st.Battery} %, заряжается" : $"{st.Battery} %"));
        if (_mouse is { } m) rows.Add(("USB-ID", $"{m.VendorId:X4}:{m.ProductId:X4}"));

        MouseAboutGrid.Children.Clear();
        MouseAboutGrid.RowDefinitions.Clear();
        for (var i = 0; i < rows.Count; i++)
        {
            MouseAboutGrid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
            var label = new TextBlock { Text = rows[i].Item1, Margin = new Thickness(0, 0, 12, 8) };
            label.SetResourceReference(TextBlock.ForegroundProperty, "MutedBrush");
            var value = new TextBlock { Text = rows[i].Item2, FontWeight = FontWeights.SemiBold, Margin = new Thickness(0, 0, 0, 8),
                TextTrimming = TextTrimming.CharacterEllipsis };
            Grid.SetRow(label, i);
            Grid.SetRow(value, i);
            Grid.SetColumn(value, 1);
            MouseAboutGrid.Children.Add(label);
            MouseAboutGrid.Children.Add(value);
        }
    }

    private async void OnMouseFactoryReset(object sender, RoutedEventArgs e)
    {
        if (_mouseConfig is null || !ConfirmTwice(MouseFactoryButton, "Точно сбросить?")) return;
        _dpiSeparate = false;
        await MouseSendAsync(m => m.FactoryReset(), "Профиль сброшен к заводским", settleMs: 700);
    }

    private async void OnMouseRescan(object sender, RoutedEventArgs e)
    {
        for (var i = 0; i < 20 && _mouseBusy; i++) await Task.Delay(100);
        try { _mouse?.Dispose(); } catch { }
        _mouse = null;
        _mouseStatus = null;
        ForgetMouseState();
        MouseEmptyTitle.Text = "Ищу мышь…";
        await UpdateMouseAsync(force: true);
    }

    private void OnMouseDiagToggle(object sender, RoutedEventArgs e)
    {
        MouseDiag.Visibility = MouseDiagToggle.IsChecked == true ? Visibility.Visible : Visibility.Collapsed;
        MouseDiag.Text = MouseDiagText();
    }
}
