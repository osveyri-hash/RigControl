using System.Windows;
using System.Windows.Threading;

namespace RigControl;

/// <summary>Заряд батареи клавиатуры: по радио, раз в 15 секунд (как официальная программа).</summary>
public partial class MainWindow
{
    private DispatcherTimer? _batteryTimer;
    private bool _batteryBusy;
    private int? _battery;
    private bool _batteryWarned;
    private Ak820Dongle? _batteryDongle;

    private const int BatteryLowPercent = 15;

    private void StartBatteryWatch()
    {
        _batteryTimer = new DispatcherTimer { Interval = TimeSpan.FromSeconds(15) };
        _batteryTimer.Tick += async (_, _) => await UpdateBatteryAsync();
        _batteryTimer.Start();

        // Первое показание — сразу после подключения, не через 15 секунд.
        var first = new DispatcherTimer { Interval = TimeSpan.FromSeconds(3) };
        first.Tick += async (_, _) =>
        {
            first.Stop();
            await UpdateBatteryAsync();
        };
        first.Start();
    }

    private async Task UpdateBatteryAsync()
    {
        if (_batteryBusy) return;

        var dongle = _dongle;

        // По кабелю (или донгла нет) — заряд не узнать: клавиатура отвечает о нём только по радио.
        if (dongle is null || _device is not null)
        {
            ShowBattery(null, wired: _device is not null);
            return;
        }

        // Новый донгл (переподключение) — сразу пробуем, старое показание не держим.
        if (!ReferenceEquals(dongle, _batteryDongle))
        {
            _batteryDongle = dongle;
            _battery = null;
        }

        _batteryBusy = true;
        try
        {
            var level = await Task.Run(() =>
            {
                try { return dongle.ReadBattery(); }
                catch { return null; }
            });

            // Не ответила один раз — оставляем прошлое показание (клавиатура могла задремать).
            if (level is not null) _battery = level;
            ShowBattery(_battery, wired: false);
        }
        finally
        {
            _batteryBusy = false;
        }
    }

    private void ShowBattery(int? level, bool wired)
    {
        if (level is not { } percent)
        {
            // Клавиатура заряд не сообщила (её донгл просто повторяет запрос) — индикатор прячем,
            // а не рисуем выдуманные 100 %, как официальная программа.
            BatteryPanel.Visibility = Visibility.Collapsed;
            if (_tray is not null) _tray.Tip = "Rig Control";
            if (wired) _batteryWarned = false;   // по кабелю заряжается
            return;
        }

        BatteryPanel.Visibility = Visibility.Visible;
        BatteryText.Text = $"{percent} %";
        BatteryText.ClearValue(System.Windows.Controls.TextBlock.ForegroundProperty);
        BatteryFill.Width = Math.Max(1.5, 18 * percent / 100.0);
        BatteryFill.SetResourceReference(System.Windows.Controls.Border.BackgroundProperty,
            percent <= BatteryLowPercent ? "BadBrush" : "GoodBrush");

        if (_tray is not null) _tray.Tip = $"Rig Control · клавиатура {percent} %";

        // Один раз за разряд — уведомление; снова — после зарядки.
        if (percent <= BatteryLowPercent && !_batteryWarned)
        {
            _batteryWarned = true;
            _tray?.ShowBalloon("Клавиатура садится", $"Заряд {percent} % — пора подключить кабель.");
        }
        else if (percent > BatteryLowPercent + 10)
        {
            _batteryWarned = false;
        }
    }
}
