using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Interop;
using System.Windows.Threading;

namespace RigControl;

/// <summary>
/// Значок в трее на чистом Win32 (Shell_NotifyIcon) — без WinForms: та библиотека ради одного значка
/// загружала в память несколько мегабайт. Меню по правой кнопке — обычное WPF-меню в стиле программы.
/// </summary>
public sealed class TrayIcon : IDisposable
{
    private const int CallbackMessage = 0x8000 + 1; // WM_APP + 1
    private const int WmLButtonDblClk = 0x0203, WmRButtonUp = 0x0205;
    private const int NimAdd = 0, NimModify = 1, NimDelete = 2;
    private const int NifMessage = 0x01, NifIcon = 0x02, NifTip = 0x04, NifInfo = 0x10;
    private const int NiifInfo = 0x01;
    private const int MsgfltAllow = 1;

    /// <summary>Проводник перезапустился (или панель задач появилась позже нас) — значок надо добавить заново.</summary>
    private static readonly int TaskbarCreated = RegisterWindowMessage("TaskbarCreated");

    private readonly HwndSource _window;
    private string _tip;
    private IntPtr _icon;
    private bool _ownsIcon;
    private bool _added;
    private bool _disposed;
    private DispatcherTimer? _retry;
    private int _retries;

    /// <summary>Строит меню при каждом открытии: так оно всегда в текущей теме (светлая/тёмная).</summary>
    public Func<ContextMenu?>? MenuProvider { get; set; }

    private ContextMenu? _menu;

    public event Action? DoubleClick;

    public TrayIcon(string tip)
    {
        _tip = tip;

        // Невидимое окно верхнего уровня (без WS_VISIBLE): только принимает сообщения от значка.
        // Не «message-only»: такое окно не получает рассылку TaskbarCreated.
        _window = new HwndSource(new HwndSourceParameters("RigControl.Tray")
        {
            Width = 0,
            Height = 0,
            WindowStyle = 0,
            ExtendedWindowStyle = 0x80 // WS_EX_TOOLWINDOW — не появляется в Alt+Tab
        });
        _window.AddHook(WndProc);

        // Программа работает от администратора, а проводник — нет: без разрешения Windows не пропустила бы
        // его сообщения к нам (щелчки по значку и «панель задач перезапущена»).
        try
        {
            ChangeWindowMessageFilterEx(_window.Handle, CallbackMessage, MsgfltAllow, IntPtr.Zero);
            if (TaskbarCreated != 0)
                ChangeWindowMessageFilterEx(_window.Handle, TaskbarCreated, MsgfltAllow, IntPtr.Zero);
        }
        catch
        {
            // старая Windows — фильтра сообщений нет
        }

        LoadAppIcon();
        Add();
    }

    /// <summary>Текст при наведении на значок (например, заряд клавиатуры).</summary>
    public string Tip
    {
        get => _tip;
        set
        {
            value = value.Length > 127 ? value[..127] : value;
            if (_tip == value) return;
            _tip = value;
            if (!_added) return;

            var data = NewData(NifTip);
            data.szTip = _tip;
            Shell_NotifyIcon(NimModify, ref data);
        }
    }

    /// <summary>Всплывающая подсказка у значка (на Windows 10/11 — уведомление).</summary>
    public void ShowBalloon(string title, string text)
    {
        if (!_added) return;

        var data = NewData(NifInfo);
        data.szInfo = text;
        data.szInfoTitle = title;
        data.dwInfoFlags = NiifInfo;
        data.uTimeoutOrVersion = 2500;
        Shell_NotifyIcon(NimModify, ref data);
    }

    private void LoadAppIcon()
    {
        try
        {
            var path = Environment.ProcessPath;
            if (!string.IsNullOrEmpty(path))
            {
                var small = new IntPtr[1];
                if (ExtractIconEx(path, 0, null, small, 1) > 0 && small[0] != IntPtr.Zero)
                {
                    _icon = small[0];
                    _ownsIcon = true;
                    return;
                }
            }
        }
        catch (Exception ex)
        {
            // возьмём стандартный значок
            QuietLog.Write(ex);
        }

        _icon = LoadIcon(IntPtr.Zero, (IntPtr)32512); // IDI_APPLICATION
        _ownsIcon = false;
    }

    private NotifyIconData NewData(int flags) => new()
    {
        cbSize = Marshal.SizeOf<NotifyIconData>(),
        hWnd = _window.Handle,
        uID = 1,
        uFlags = flags,
        szTip = "",
        szInfo = "",
        szInfoTitle = ""
    };

    private void Add()
    {
        if (_disposed) return;

        var data = NewData(NifMessage | NifIcon | NifTip);
        data.uCallbackMessage = CallbackMessage;
        data.hIcon = _icon;
        data.szTip = _tip;

        // Значок мог остаться с прошлой попытки (проводник ответил с опозданием) — тогда хватит обновить.
        _added = Shell_NotifyIcon(NimAdd, ref data) || Shell_NotifyIcon(NimModify, ref data);
        if (_added)
        {
            _retry?.Stop();
            return;
        }

        // При автозапуске панель задач бывает ещё не готова — пробуем снова каждые 2 секунды (до минуты).
        if (_retry is null)
        {
            _retry = new DispatcherTimer(DispatcherPriority.Background) { Interval = TimeSpan.FromSeconds(2) };
            _retry.Tick += (_, _) =>
            {
                if (_added || ++_retries > 30)
                {
                    _retry.Stop();
                    return;
                }
                Add();
            };
        }

        if (!_retry.IsEnabled) _retry.Start();
    }

    private IntPtr WndProc(IntPtr hwnd, int msg, IntPtr wParam, IntPtr lParam, ref bool handled)
    {
        if (msg == CallbackMessage)
        {
            switch ((int)(lParam.ToInt64() & 0xFFFF))
            {
                case WmLButtonDblClk:
                    DoubleClick?.Invoke();
                    break;
                case WmRButtonUp:
                    ShowMenu();
                    break;
            }

            handled = true;
        }
        else if (TaskbarCreated != 0 && msg == TaskbarCreated)
        {
            _added = false;
            _retries = 0;
            Add();
        }

        return IntPtr.Zero;
    }

    private void ShowMenu()
    {
        if (_menu is { IsOpen: true } open) open.IsOpen = false;

        var menu = MenuProvider?.Invoke();
        _menu = menu;
        if (menu is null) return;

        menu.Placement = PlacementMode.MousePoint;
        menu.IsOpen = true;

        // Меню должно стать активным окном — иначе оно не закроется по щелчку мимо.
        var handle = (PresentationSource.FromVisual(menu) as HwndSource)?.Handle ?? IntPtr.Zero;
        SetForegroundWindow(handle != IntPtr.Zero ? handle : _window.Handle);
    }

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;

        _retry?.Stop();
        if (_menu is { IsOpen: true } menu) menu.IsOpen = false;
        _menu = null;

        if (_added)
        {
            var data = NewData(0);
            Shell_NotifyIcon(NimDelete, ref data);
            _added = false;
        }

        if (_ownsIcon && _icon != IntPtr.Zero) DestroyIcon(_icon);
        _icon = IntPtr.Zero;

        _window.RemoveHook(WndProc);
        _window.Dispose();
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct NotifyIconData
    {
        public int cbSize;
        public IntPtr hWnd;
        public int uID;
        public int uFlags;
        public int uCallbackMessage;
        public IntPtr hIcon;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 128)] public string szTip;
        public int dwState;
        public int dwStateMask;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 256)] public string szInfo;
        public int uTimeoutOrVersion;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 64)] public string szInfoTitle;
        public int dwInfoFlags;
        public Guid guidItem;
        public IntPtr hBalloonIcon;
    }

    [DllImport("shell32.dll", EntryPoint = "Shell_NotifyIconW", CharSet = CharSet.Unicode)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool Shell_NotifyIcon(int message, ref NotifyIconData data);

    [DllImport("shell32.dll", EntryPoint = "ExtractIconExW", CharSet = CharSet.Unicode)]
    private static extern uint ExtractIconEx(string file, int index, IntPtr[]? large, IntPtr[]? small, uint count);

    [DllImport("user32.dll", EntryPoint = "LoadIconW")]
    private static extern IntPtr LoadIcon(IntPtr instance, IntPtr name);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool DestroyIcon(IntPtr icon);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetForegroundWindow(IntPtr hwnd);

    [DllImport("user32.dll", EntryPoint = "RegisterWindowMessageW", CharSet = CharSet.Unicode)]
    private static extern int RegisterWindowMessage(string name);

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool ChangeWindowMessageFilterEx(IntPtr hwnd, int message, int action, IntPtr filter);
}
