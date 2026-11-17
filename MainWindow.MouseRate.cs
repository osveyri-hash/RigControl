using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Input;
using System.Windows.Interop;
using System.Windows.Media;
using System.Windows.Threading;

namespace RigControl;

/// <summary>
/// Тест частоты опроса: пока курсор над полем, считаем отчёты мыши через Raw Input (WM_INPUT) —
/// так же, как онлайн-тестеры Гц. Паузы в движении (больше 50 мс) не учитываются.
/// </summary>
public partial class MainWindow
{
    private const int WmInput = 0x00FF;
    private const uint RidInput = 0x10000003;
    private const uint RidevRemove = 0x00000001;
    private const int RateBufferSize = 256;
    private const int RateHistory = 60;

    private static readonly int RawHeaderSize = 8 + 2 * IntPtr.Size;
    private static readonly long RateGap = Stopwatch.Frequency / 20;

    private HwndSource? _rateSource;
    private DispatcherTimer? _rateTimer;
    private IntPtr _rateBuffer;
    private long _rateLast, _rateSpan;
    private int _rateEvents, _ratePeak;
    private readonly List<int> _rateRaw = new();
    private readonly List<int> _rateHistory = new();

    [StructLayout(LayoutKind.Sequential)]
    private struct RawInputDevice
    {
        public ushort UsagePage;
        public ushort Usage;
        public uint Flags;
        public IntPtr Target;
    }

    [DllImport("user32.dll", SetLastError = true)]
    private static extern bool RegisterRawInputDevices([In] RawInputDevice[] devices, uint count, uint size);

    [DllImport("user32.dll")]
    private static extern uint GetRawInputData(IntPtr input, uint command, IntPtr data, ref uint size, uint headerSize);

    private void OnRatePadEnter(object sender, MouseEventArgs e) => StartRateTest();

    private void OnRatePadLeave(object sender, MouseEventArgs e) => StopRateTest();

    private void StartRateTest()
    {
        if (_rateSource is not null) return;
        var handle = new WindowInteropHelper(this).Handle;
        if (handle == IntPtr.Zero) return;

        var device = new[] { new RawInputDevice { UsagePage = 0x01, Usage = 0x02, Flags = 0, Target = handle } };
        if (!RegisterRawInputDevices(device, 1, (uint)Marshal.SizeOf<RawInputDevice>()))
        {
            MouseRateSub.Text = "Raw Input недоступен";
            return;
        }

        _rateBuffer = Marshal.AllocHGlobal(RateBufferSize);
        _rateSource = HwndSource.FromHwnd(handle);
        _rateSource?.AddHook(RateHook);
        _rateRaw.Clear();
        _rateHistory.Clear();
        _rateEvents = _ratePeak = 0;
        _rateLast = _rateSpan = 0;
        MouseRateNow.Text = "—";
        MouseRateSub.Text = "водите мышью по кругу";
        MouseRateGraph.Points = null;

        if (_rateTimer is null)
        {
            _rateTimer = new DispatcherTimer(DispatcherPriority.Background) { Interval = TimeSpan.FromMilliseconds(100) };
            _rateTimer.Tick += (_, _) => RateTick();
        }
        _rateTimer.Start();
    }

    private void StopRateTest()
    {
        _rateTimer?.Stop();
        if (_rateSource is null) return;

        _rateSource.RemoveHook(RateHook);
        _rateSource = null;
        var device = new[] { new RawInputDevice { UsagePage = 0x01, Usage = 0x02, Flags = RidevRemove, Target = IntPtr.Zero } };
        RegisterRawInputDevices(device, 1, (uint)Marshal.SizeOf<RawInputDevice>());
        Marshal.FreeHGlobal(_rateBuffer);
        _rateBuffer = IntPtr.Zero;
        if (_rateHistory.Count == 0)
        {
            MouseRateNow.Text = "—";
            MouseRateSub.Text = "Гц";
        }
    }

    private IntPtr RateHook(IntPtr hwnd, int msg, IntPtr wParam, IntPtr lParam, ref bool handled)
    {
        if (msg != WmInput || _rateBuffer == IntPtr.Zero) return IntPtr.Zero;

        var size = (uint)RateBufferSize;
        var read = GetRawInputData(lParam, RidInput, _rateBuffer, ref size, (uint)RawHeaderSize);
        if (read == uint.MaxValue || read < RawHeaderSize + 20) return IntPtr.Zero;
        if (Marshal.ReadInt32(_rateBuffer) != 0) return IntPtr.Zero;   // RIM_TYPEMOUSE
        if (Marshal.ReadInt32(_rateBuffer, RawHeaderSize + 12) == 0 && Marshal.ReadInt32(_rateBuffer, RawHeaderSize + 16) == 0)
            return IntPtr.Zero;   // только движение, без кнопок и колеса

        var now = Stopwatch.GetTimestamp();
        var gap = now - _rateLast;
        _rateLast = now;
        if (gap < RateGap)
        {
            _rateEvents++;
            _rateSpan += gap;
        }

        return IntPtr.Zero;   // WM_INPUT дальше идёт в DefWindowProc
    }

    private void RateTick()
    {
        var events = _rateEvents;
        var span = _rateSpan;
        _rateEvents = 0;
        _rateSpan = 0;
        if (events < 6 || span < Stopwatch.Frequency / 50) return;

        _rateRaw.Add((int)Math.Round(events * (double)Stopwatch.Frequency / span));
        if (_rateRaw.Count > 5) _rateRaw.RemoveAt(0);

        // Медиана последних замеров: один «склеенный» пакет сообщений не даёт ложного пика.
        var sorted = _rateRaw.OrderBy(v => v).ToArray();
        var hz = sorted[sorted.Length / 2];
        _rateHistory.Add(hz);
        if (_rateHistory.Count > RateHistory) _rateHistory.RemoveAt(0);
        _ratePeak = Math.Max(_ratePeak, hz);

        MouseRateNow.Text = hz.ToString();
        MouseRateSub.Text = $"Гц  ·  пик {_ratePeak}";
        DrawRateGraph();
    }

    private void DrawRateGraph()
    {
        var w = MouseRateCanvas.ActualWidth;
        var h = MouseRateCanvas.ActualHeight;
        if (w <= 0 || h <= 0 || _rateHistory.Count < 2)
        {
            MouseRateGraph.Points = null;
            return;
        }

        var top = Math.Max(_rateHistory.Max(), 125) * 1.25;
        var step = w / (RateHistory - 1);
        var x0 = w - (_rateHistory.Count - 1) * step;
        var points = new PointCollection(_rateHistory.Count);
        for (var i = 0; i < _rateHistory.Count; i++)
            points.Add(new Point(x0 + i * step, h - 8 - _rateHistory[i] / top * (h - 16)));
        points.Freeze();
        MouseRateGraph.Points = points;
    }
}
