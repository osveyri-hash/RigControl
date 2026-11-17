using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Media;
using System.Windows.Shapes;
using System.Windows.Threading;
using RigControl.Devices;

namespace RigControl;

/// <summary>Главная «Устройства»: плитки с картинкой и состоянием, нажатие — открыть настройки (как в M HUB).</summary>
public partial class MainWindow
{
    private sealed record DeviceTile(Button Button, TextBlock Name, TextBlock Detail, WrapPanel Status);

    private DeviceTile? _tileMouse, _tileKeyboard, _tilePump, _tileArgb, _tileFans;
    private DispatcherTimer? _devicesTimer;
    private bool _argbOk;
    private int _fanCount;

    private bool DevicesPageVisible => PageDevices is { IsVisible: true };

    private void InitDevicesPage()
    {
        var art = new Thickness(0, 4, 0, 4);
        _tileMouse = AddDeviceTile(DeviceArt.Picture(DeviceArt.Mouse, "\uE962"), "Мышь", NavMouse, art);
        _tileKeyboard = AddDeviceTile(DeviceArt.Picture(DeviceArt.Keyboard, "\uE765"), "Клавиатура", NavKeyboard, art);
        _tilePump = AddDeviceTile(DeviceArt.Picture(DeviceArt.Pump, "\uE7F4"), "Экран помпы", NavPump, art);
        _tileArgb = AddDeviceTile(DeviceArt.Picture(DeviceArt.Argb, "\uE706"), "Подсветка СЖО", NavArgb, art);
        _tileFans = AddDeviceTile(DeviceArt.Picture(DeviceArt.Fans, "\uE9CA"), "Вентиляторы", NavFans, art);

        _devicesTimer = new DispatcherTimer { Interval = TimeSpan.FromSeconds(2) };
        _devicesTimer.Tick += (_, _) => UpdateDeviceTiles();
        PageDevices.IsVisibleChanged += (_, _) =>
        {
            if (DevicesPageVisible)
            {
                UpdateDeviceTiles();
                _devicesTimer.Start();
            }
            else
            {
                _devicesTimer.Stop();
            }
        };
        if (DevicesPageVisible) _devicesTimer.Start();
        UpdateDeviceTiles();
    }

    private DeviceTile AddDeviceTile(FrameworkElement art, string title, ToggleButton nav, Thickness artMargin)
    {
        var grid = new Grid();
        grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        grid.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        var category = new TextBlock { Text = title.ToUpperInvariant(), FontSize = 10.5, FontWeight = FontWeights.SemiBold };
        category.SetResourceReference(TextBlock.ForegroundProperty, "MutedBrush");
        grid.Children.Add(category);
        var arrow = new TextBlock { Text = "\uE76C", FontFamily = new FontFamily("Segoe MDL2 Assets"),
            FontSize = 12, HorizontalAlignment = HorizontalAlignment.Right, VerticalAlignment = VerticalAlignment.Center };
        arrow.SetResourceReference(TextBlock.ForegroundProperty, "MutedBrush");
        grid.Children.Add(arrow);
        art.Margin = artMargin;
        art.HorizontalAlignment = HorizontalAlignment.Center;
        art.VerticalAlignment = VerticalAlignment.Center;
        Grid.SetRow(art, 1);
        grid.Children.Add(art);
        var name = new TextBlock
        {
            Text = title, FontSize = 19, FontWeight = FontWeights.SemiBold, Margin = new Thickness(0, 8, 0, 0),
            TextTrimming = TextTrimming.CharacterEllipsis
        };
        Grid.SetRow(name, 2);
        grid.Children.Add(name);
        // Что сейчас настроено: DPI мыши, эффект подсветки, режим вентиляторов…
        var detail = new TextBlock { FontSize = 12.5, Margin = new Thickness(0, 3, 0, 0), TextTrimming = TextTrimming.CharacterEllipsis };
        detail.SetResourceReference(TextBlock.ForegroundProperty, "MutedBrush");
        Grid.SetRow(detail, 3);
        grid.Children.Add(detail);
        var status = new WrapPanel { Margin = new Thickness(0, 8, 0, 0) };
        Grid.SetRow(status, 4);
        grid.Children.Add(status);
        var button = new Button { Style = (Style)FindResource("DeviceTile"), Content = grid, ToolTip = "Открыть настройки" };
        System.Windows.Automation.AutomationProperties.SetName(button, title + ": открыть настройки");
        button.Click += (_, _) => nav.IsChecked = true;
        DeviceTiles.Children.Add(button);
        return new DeviceTile(button, name, detail, status);
    }

    private void UpdateDeviceTiles()
    {
        if (_tileMouse is null || !DevicesPageVisible) return;

        // Мышь
        var st = _mouseStatus;
        var mouseOnline = _mouse is not null && st is { HasMouse: true };
        _tileMouse.Name.Text = _mouse is null ? "Мышь" : _mouse.Model(st);
        if (mouseOnline)
            SetTileStatus(_tileMouse, null, st!.Battery, st.Charging, MouseWireless ? "2.4G" : "USB");
        else
            SetTileStatus(_tileMouse, _mouse is null ? "Не найдена" : "Выключена");

        // Клавиатура
        _tileKeyboard!.Name.Text = "AK820 Pro";
        if (_device is not null || _dongle is not null)
            SetTileStatus(_tileKeyboard, null, _device is null ? _battery : null, false, _device is not null ? "USB" : "2.4G");
        else
            SetTileStatus(_tileKeyboard, StatusText.Text);

        // Помпа
        var cpu = _snapshot.CpuTemp;
        SetTileStatus(_tilePump!, PumpStatusText.Text, ok: _cooler is not null);

        // Подсветка
        SetTileStatus(_tileArgb!, ArgbStatusText.Text, ok: _argbOk);

        // Вентиляторы
        SetTileStatus(_tileFans!, _fanCount > 0
            ? $"{_fanCount} {Plural(_fanCount, "канал", "канала", "каналов")}" + (cpu is { } c ? $" · CPU {c:0} °C" : "")
            : "Нет датчиков", ok: _fanCount > 0);

        // Вторая строка: что сейчас настроено
        _tileMouse.Detail.Text = mouseOnline && _mouseConfig is { } cfg
            ? $"Профиль {cfg.Profile + 1} · {MouseDpiText(cfg)} DPI" + (MouseRateHz(cfg) is { } hz ? $" · {hz} Гц" : "")
            : "";
        _tileKeyboard.Detail.Text = _lightOff
            ? "Подсветка выключена"
            : _settings.SyncLight ? "Подсветка в такт гифке"
            : "Эффект: " + Ak820Protocol.Effects[Math.Clamp(_settings.Effect, 0, Ak820Protocol.Effects.Length - 1)].Name;
        _tilePump!.Detail.Text = CoolerRunning ? "Экран работает" : "Экран остановлен";
        _tileArgb!.Detail.Text = ArgbModeCombo.SelectedItem is ComboBoxItem { Content: string mode } ? "Режим: " + mode : "";
        _tileFans!.Detail.Text = "Режим: " + FanSummary(_settings.FanModes, _settings.FanModeAll).ToLowerInvariant();

        UpdateDevicesSummary();
    }

    /// <summary>«На связи 3 из 5 · профиль «Игра»» под заголовком главной.</summary>
    private void UpdateDevicesSummary()
    {
        if (DevicesSummary is null || _tileMouse is null) return;

        var online = 0;
        if (_mouse is not null && _mouseStatus is { HasMouse: true }) online++;
        if (_device is not null || _dongle is not null) online++;
        if (_cooler is not null) online++;
        if (_argbOk) online++;
        if (_fanCount > 0) online++;

        var text = $"На связи {online} из 5";
        if (ActiveProfile is { } profile) text += $" · профиль «{profile.Name}»";
        DevicesSummary.Text = text + ". Нажмите на плитку, чтобы открыть настройки.";
    }

    /// <summary>Строки состояния в боковой панели открывают страницу своего устройства.</summary>
    private void OnStatusRowClick(object sender, System.Windows.Input.MouseButtonEventArgs e)
    {
        if (sender is FrameworkElement { Tag: string nav } && FindName(nav) is RadioButton button)
            button.IsChecked = true;
    }

    private static string Plural(int n, string one, string few, string many) =>
        (n % 100) is >= 11 and <= 14 ? many : (n % 10) switch { 1 => one, >= 2 and <= 4 => few, _ => many };

    /// <summary>Строка состояния под именем: точка + текст, или заряд и тип связи (как «80% · 2.4G» в M HUB).</summary>
    private static void SetTileStatus(DeviceTile tile, string? text, int? battery = null, bool charging = false,
        string? link = null, bool? ok = null)
    {
        var panel = tile.Status;
        panel.Children.Clear();
        var good = ok ?? text is null;

        if (text is not null)
        {
            var dot = new Ellipse { Width = 9, Height = 9, Margin = new Thickness(0, 0, 8, 0), VerticalAlignment = VerticalAlignment.Center };
            dot.SetResourceReference(Shape.FillProperty, good ? "GoodBrush" : "BadBrush");
            panel.Children.Add(dot);
            panel.Children.Add(StatusText2(text));
            return;
        }

        if (battery is { } percent)
        {
            var shell = new Border
            {
                Width = 12, Height = 20, CornerRadius = new CornerRadius(2.5), BorderThickness = new Thickness(1.5),
                Padding = new Thickness(1.5), VerticalAlignment = VerticalAlignment.Center,
            };
            shell.SetResourceReference(Border.BorderBrushProperty, "TextBrush");
            var fill = new Border
            {
                Height = Math.Max(1.5, 14 * percent / 100.0), VerticalAlignment = VerticalAlignment.Bottom,
                CornerRadius = new CornerRadius(1),
            };
            fill.SetResourceReference(Border.BackgroundProperty, percent <= BatteryLowPercent ? "BadBrush" : "TextBrush");
            shell.Child = fill;
            panel.Children.Add(shell);
            panel.Children.Add(StatusText2($"{percent}%", new Thickness(7, 0, 0, 0)));
            if (charging) panel.Children.Add(Glyph("\uE945", new Thickness(4, 0, 0, 0)));
        }

        if (link is not null)
        {
            panel.Children.Add(Glyph(link == "USB" ? "\uE88E" : "\uE701", new Thickness(battery is null ? 0 : 18, 0, 0, 0)));
            panel.Children.Add(StatusText2(link, new Thickness(7, 0, 0, 0)));
        }
    }

    private static TextBlock StatusText2(string text, Thickness? margin = null) => new()
    {
        Text = text, FontSize = 14, VerticalAlignment = VerticalAlignment.Center,
        Margin = margin ?? new Thickness(0), TextTrimming = TextTrimming.CharacterEllipsis, MaxWidth = 210,
    };

    private static TextBlock Glyph(string glyph, Thickness margin) => new()
    {
        Text = glyph, FontFamily = new FontFamily("Segoe MDL2 Assets"), FontSize = 15,
        VerticalAlignment = VerticalAlignment.Center, Margin = margin,
    };
}
