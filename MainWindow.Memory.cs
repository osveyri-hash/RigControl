using System.Runtime;
using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Threading;

namespace RigControl;

/// <summary>Сколько памяти занимает программа и как отдать лишнее системе, пока окно спрятано или не в фокусе.</summary>
public partial class MainWindow
{
    /// <summary>PROCESS_MEMORY_COUNTERS_EX2 (PrivateWorkingSetSize есть с Windows 10 1809).</summary>
    [StructLayout(LayoutKind.Sequential)]
    private struct ProcessMemoryCounters
    {
        public uint Size;
        public uint PageFaultCount;
        public nuint PeakWorkingSetSize;
        public nuint WorkingSetSize;
        public nuint QuotaPeakPagedPoolUsage;
        public nuint QuotaPagedPoolUsage;
        public nuint QuotaPeakNonPagedPoolUsage;
        public nuint QuotaNonPagedPoolUsage;
        public nuint PagefileUsage;
        public nuint PeakPagefileUsage;
        public nuint PrivateUsage;
        public nuint PrivateWorkingSetSize;
        public ulong SharedCommitUsage;
    }

    [DllImport("kernel32.dll")]
    private static extern IntPtr GetCurrentProcess();

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool K32GetProcessMemoryInfo(IntPtr process, ref ProcessMemoryCounters counters, uint size);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool SetProcessWorkingSetSize(IntPtr process, nint minimum, nint maximum);

    private DispatcherTimer? _memoryTimer;
    private DispatcherTimer? _trimTimer;

    private bool WindowShown => IsVisible && WindowState != WindowState.Minimized;

    private void StartMemoryWatch()
    {
        _memoryTimer = new DispatcherTimer(DispatcherPriority.Background) { Interval = TimeSpan.FromSeconds(2) };
        _memoryTimer.Tick += (_, _) => UpdateMemoryText();
        _memoryTimer.Start();

        StateChanged += (_, _) =>
        {
            if (WindowState == WindowState.Minimized)
            {
                if (CoolerRunning) PumpLog.Write("окно свёрнуто (экран помпы продолжает работать)");
                ScheduleMemoryTrim();
            }
            else UpdateMemoryText();
        };
        IsVisibleChanged += (_, _) =>
        {
            if (IsVisible) UpdateMemoryText();
        };

        // Окно открыто, но вы работаете в другой программе: через минуту тоже отдаём лишнее.
        Deactivated += (_, _) =>
        {
            if (WindowShown) ScheduleMemoryTrim(TimeSpan.FromMinutes(1));
        };
        Activated += (_, _) =>
        {
            if (WindowShown) _trimTimer?.Stop();
        };

        UpdateMemoryText();
    }

    /// <summary>Подпись «ОЗУ: N МБ» внизу боковой панели; пока окно скрыто, не обновляется.</summary>
    private void UpdateMemoryText()
    {
        if (!WindowShown) return;

        var bytes = MemoryUsage();
        MemoryText.Text = $"ОЗУ: {Math.Max(1, (bytes + 512 * 1024) / (1024 * 1024))} МБ";
    }

    /// <summary>
    /// Частный рабочий набор — то же число, что «Память» в диспетчере задач.
    /// На старых Windows без него — весь рабочий набор процесса.
    /// </summary>
    private static long MemoryUsage()
    {
        try
        {
            var counters = new ProcessMemoryCounters { Size = (uint)Marshal.SizeOf<ProcessMemoryCounters>() };
            if (K32GetProcessMemoryInfo(GetCurrentProcess(), ref counters, counters.Size))
                return (long)(counters.PrivateWorkingSetSize > 0 ? counters.PrivateWorkingSetSize : counters.WorkingSetSize);
        }
        catch
        {
            // нет функции — ниже запасной способ
        }

        return Environment.WorkingSet;
    }

    /// <summary>
    /// Окно убрали в трей или свернули (через 3 секунды) либо оно просто не в фокусе (через минуту):
    /// собираем мусор, сжимаем кучу и отдаём системе страницы, которые сейчас не нужны.
    /// Потом повторяем раз в 5 минут, пока окно снова не станет активным.
    /// </summary>
    private void ScheduleMemoryTrim(TimeSpan? delay = null)
    {
        if (_trimTimer is null)
        {
            _trimTimer = new DispatcherTimer(DispatcherPriority.Background);
            _trimTimer.Tick += async (_, _) =>
            {
                if (WindowShown && IsActive)
                {
                    _trimTimer.Stop();
                    return;
                }

                _trimTimer.Interval = TimeSpan.FromMinutes(5);
                await Task.Run(() => TrimMemory(gentle: CoolerRunning));
                UpdateMemoryText();
            };
        }

        _trimTimer.Stop();
        _trimTimer.Interval = delay ?? TimeSpan.FromSeconds(3);
        _trimTimer.Start();
    }

    private static void TrimMemory(bool gentle = false)
    {
        try
        {
            if (gentle)
            {
                // Экран помпы питается непрерывным потоком кадров. Полная компактирующая сборка
                // ставила UI и отправку на паузу на секунды, из-за чего прошивка могла показать логотип.
                // Пока экран работает — не компактим LOH и не ждём финализаторы.
                GC.Collect(0, GCCollectionMode.Optimized, blocking: false, compacting: false);
                SetProcessWorkingSetSize(GetCurrentProcess(), -1, -1);
                return;
            }

            GCSettings.LargeObjectHeapCompactionMode = GCLargeObjectHeapCompactionMode.CompactOnce;
            GC.Collect(2, GCCollectionMode.Forced, blocking: true, compacting: true);
            GC.WaitForPendingFinalizers();
            GC.Collect(2, GCCollectionMode.Forced, blocking: true, compacting: true);

            // Страницы уходят в резерв Windows и вернутся сами, если понадобятся.
            SetProcessWorkingSetSize(GetCurrentProcess(), -1, -1);
        }
        catch
        {
            // не критично
        }
    }
}
