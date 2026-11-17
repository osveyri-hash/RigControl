using System.Runtime.InteropServices;

namespace RigControl;

/// <summary>
/// Точный таймер Windows (1 мс) на время потока живых кадров подсветки. По умолчанию Sleep(2) спит ~15,6 мс,
/// и кадр из 12 пакетов уходил бы ~190 мс вместо ~40. Вызовы парные: Begin в начале потока, End в finally.
/// </summary>
internal static class TimerResolution
{
    [DllImport("winmm.dll")]
    private static extern uint timeBeginPeriod(uint period);

    [DllImport("winmm.dll")]
    private static extern uint timeEndPeriod(uint period);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetProcessInformation(IntPtr process, int informationClass,
        ref PowerThrottlingState information, uint size);

    [DllImport("kernel32.dll")]
    private static extern IntPtr GetCurrentProcess();

    [StructLayout(LayoutKind.Sequential)]
    private struct PowerThrottlingState
    {
        public uint Version;
        public uint ControlMask;
        public uint StateMask;
    }

    private const int ProcessPowerThrottling = 4;
    private const uint PowerThrottlingCurrentVersion = 1;
    private const uint PowerThrottlingIgnoreTimerResolution = 0x4;

    /// <summary>
    /// Windows 11 не выполняет запрос точного таймера, пока окно программы скрыто или свёрнуто, а подсветка работает
    /// и из трея — анимации начинали дёргаться. Просим выполнять запрос всегда (ControlMask = флаг, StateMask = 0).
    /// На Windows 10 такого флага нет — вызов просто вернёт false.
    /// </summary>
    static TimerResolution()
    {
        try
        {
            var state = new PowerThrottlingState
            {
                Version = PowerThrottlingCurrentVersion,
                ControlMask = PowerThrottlingIgnoreTimerResolution,
                StateMask = 0
            };
            SetProcessInformation(GetCurrentProcess(), ProcessPowerThrottling, ref state, (uint)Marshal.SizeOf<PowerThrottlingState>());
        }
        catch (Exception ex)
        {
            QuietLog.Write(ex);
        }
    }

    public static void Begin()
    {
        try { _ = timeBeginPeriod(1); } catch { /* без точного таймера просто медленнее */ }
    }

    public static void End()
    {
        try { _ = timeEndPeriod(1); } catch { /* не критично */ }
    }
}
