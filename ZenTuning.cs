using System.IO;
using System.Runtime.InteropServices;
using System.Runtime.Intrinsics.X86;
using System.Text;
using Microsoft.Win32.SafeHandles;

namespace RigControl;

/// <summary>Тайминги памяти одного канала контроллера (UMC) в тактах MEMCLK.</summary>
public sealed record DramTimings(
    int DataRate, double MemClkMhz, bool Cmd2T, bool Gdm, bool PowerDown,
    uint Cl, uint RcdRd, uint RcdWr, uint Rp, uint Ras, uint Rc,
    uint Rrds, uint Rrdl, uint Faw, uint Wtrs, uint Wtrl, uint Wr, uint Rtp, uint Cwl,
    uint Rfc, uint Rfc2, uint RfcSb, uint Refi)
{
    /// <summary>Такты → наносекунды: у DDR5 MEMCLK вдвое ниже скорости обмена.</summary>
    public double Ns(uint clocks) => DataRate > 0 ? clocks * 2000.0 / DataRate : 0;
}

/// <summary>Curve Optimizer по ядрам (в порядке ядер Windows) и тайминги памяти. null — прочитать не удалось, см. заметки.</summary>
public sealed record ZenReport(IReadOnlyList<int>? CurveOptimizer, string? CoNote, DramTimings? Timings, string? TimingsNote);

/// <summary>
/// Curve Optimizer и тайминги памяти Ryzen 7000 — только чтение, через драйвер PawnIO и подписанные модули,
/// которые уже лежат в LibreHardwareMonitorLib 0.9.6 (RyzenSMU, AMDFamily17).
/// Адреса и команды — как в ZenStates-Core (ZenTimings, SMUDebugTool).
/// </summary>
public static class ZenTuning
{
    private const string NoPawnIo = "Нужен драйвер PawnIO (pawnio.eu) и запуск от администратора.";

    // RyzenSMU, RSMU-ящик Raphael: GetDldoPsmMargin. Только чтение; команды записи сюда не попадают.
    private const long SmuGetCurveOptimizer = 0xD5;

    // Номер Raphael в перечислении кодовых имён модуля RyzenSMU (CPU_Raphael).
    private const long CodeNameRaphael = 16;

    // Предохранители: какие CCD и ядра отключены на заводе (Zen 4).
    private const uint CcdFusePresent = 0x5D3BC;
    private const uint CcdFuseDown = 0x5D3C0;
    private const uint CoreFuse = 0x30081CD0;
    private const uint CoreFuseCcdStride = 1u << 25;

    public static ZenReport Read()
    {
        var (family, model) = CpuFamilyModel();
        var raphael = family == 0x19 && model == 0x61;
        var ddr5Umc = raphael || (family == 0x1A && model == 0x44);   // Ryzen 7000 / 9000: DDR5, одна раскладка UMC

        if (!ddr5Umc)
        {
            const string unsupported = "Только для Ryzen 7000 (AM5).";
            return new ZenReport(null, unsupported, null, "Только для Ryzen 7000 и 9000 (AM5, DDR5).");
        }

        using var amd = PawnModule.Load("LibreHardwareMonitor.Resources.PawnIo.AMDFamily17.bin");
        if (amd is null) return new ZenReport(null, NoPawnIo, null, NoPawnIo);

        using var pci = PciLock.Take();
        if (pci is null)
        {
            const string busy = "Шина занята другой программой — откройте страницу ещё раз.";
            return new ZenReport(null, busy, null, busy);
        }

        string? timingsNote = null;
        DramTimings? timings = null;
        try { timings = ReadTimings(amd); }
        catch (Exception ex) { QuietLog.Write(ex); }
        if (timings is null) timingsNote = "Не удалось прочитать тайминги.";

        IReadOnlyList<int>? co = null;
        string? coNote;
        if (!raphael) coNote = "Только для Ryzen 7000.";
        else
        {
            try { co = ReadCurveOptimizer(amd, out coNote); }
            catch (Exception ex)
            {
                QuietLog.Write(ex);
                coNote = "Не удалось прочитать Curve Optimizer.";
            }
        }

        return new ZenReport(co, coNote, timings, timingsNote);
    }

    private static List<int>? ReadCurveOptimizer(PawnModule amd, out string? note)
    {
        note = null;
        var slots = EnabledCoreSlots(amd);
        if (slots is null)
        {
            note = "Не удалось определить, какие ядра включены.";
            return null;
        }

        using var smu = PawnModule.Load("LibreHardwareMonitor.Resources.PawnIo.RyzenSMU.bin");
        if (smu is null)
        {
            note = NoPawnIo;
            return null;
        }

        // Модуль сам определяет процессор; команду шлём только если и он видит Raphael (его RSMU-ящик).
        var codeName = new long[1];
        if (!smu.Execute("ioctl_get_code_name", Array.Empty<long>(), codeName) || codeName[0] != CodeNameRaphael)
        {
            note = "Модуль PawnIO не распознал Ryzen 7000.";
            return null;
        }

        var result = new List<int>();
        foreach (var (ccd, core) in slots)
        {
            var args = new long[6];
            var mask = (long)((uint)ccd << 28 | (uint)core << 20);
            if (!smu.Execute("ioctl_send_smu_command", new[] { SmuGetCurveOptimizer, mask, 0, 0, 0, 0, 0 }, args))
            {
                note = "Процессор не ответил на запрос Curve Optimizer.";
                return null;
            }

            var value = unchecked((int)(uint)args[0]);
            if (value is < -60 or > 60)
            {
                note = $"Неожиданное значение Curve Optimizer ({value}).";
                return null;
            }
            result.Add(value);
        }

        return result;
    }

    /// <summary>
    /// Включённые ядра в порядке Windows: (номер CCD, слот ядра 0–7). Отключённые на заводе слоты пропускаем —
    /// у 7500F это 2 из 8. Если по предохранителям выходит не столько ядер, сколько видит Windows, — null.
    /// </summary>
    private static List<(int Ccd, int Core)>? EnabledCoreSlots(PawnModule amd)
    {
        var cores = Environment.ProcessorCount / ThreadsPerCore();

        var ccdMap = 1u;
        if (amd.ReadSmn(CcdFusePresent, out var present) && amd.ReadSmn(CcdFuseDown, out var down) && present != 0xFFFFFFFF)
        {
            var disabled = (present >> 30 & 0x3) | (down & 0x3F) << 2;
            var map = (present >> 22 & 0x3) & ~disabled;
            if (map != 0) ccdMap = map;
        }

        var slots = new List<(int, int)>();
        var logicalCcd = 0;
        for (var physical = 0; physical < 2; physical++)
        {
            if ((ccdMap & 1u << physical) == 0) continue;
            if (!amd.ReadSmn(CoreFuse + CoreFuseCcdStride * (uint)physical, out var fuse) || fuse == 0xFFFFFFFF) return null;

            for (var core = 0; core < 8; core++)
                if ((fuse >> core & 1) == 0) slots.Add((logicalCcd, core));
            logicalCcd++;
        }

        return slots.Count == cores ? slots : null;
    }

    /// <summary>Тайминги первого включённого канала: у AM5 они одинаковые на всех каналах.</summary>
    private static DramTimings? ReadTimings(PawnModule amd)
    {
        for (uint channel = 0; channel < 12; channel++)
        {
            var o = channel << 20;
            if (!amd.ReadSmn(o | 0x50DF0, out var config) || (config >> 19 & 1) != 0) continue;
            amd.ReadSmn(o | 0x50000, out var dimm1);
            amd.ReadSmn(o | 0x50008, out var dimm2);
            if (((dimm1 | dimm2) & 1) == 0 || dimm1 == 0xFFFFFFFF) continue;

            uint R(uint address) => amd.ReadSmn(o | address, out var v) ? v : 0;
            static uint Bits(uint value, int hi, int lo) => value >> lo & (uint)((1L << (hi - lo + 1)) - 1);

            var r200 = R(0x50200);
            double memClk = Bits(r200, 15, 0);   // множитель ×100 при BCLK 100 МГц — это и есть MEMCLK в МГц
            if (memClk <= 0) return null;

            var r204 = R(0x50204);
            var r208 = R(0x50208);
            var r20C = R(0x5020C);
            var r214 = R(0x50214);

            // tRFC лежит в одном из четырёх регистров (по рангам), 0x00C00138 — значение-заглушка.
            uint rfc = 0, rfc2 = 0, rfcSb = 0;
            foreach (var address in new uint[] { 0x50260, 0x50264, 0x50268, 0x5026C })
            {
                var value = R(address);
                if (value == 0 || value == 0x00C00138) continue;
                rfc = Bits(value, 15, 0);
                rfc2 = Bits(value, 31, 16);
                break;
            }
            foreach (var address in new uint[] { 0x502C0, 0x502C4, 0x502C8, 0x502CC })
                if (Bits(R(address), 10, 0) is var sb and > 0) { rfcSb = sb; break; }

            var timings = new DramTimings(
                DataRate: (int)Math.Round(memClk * 2), MemClkMhz: memClk,
                Cmd2T: Bits(r200, 17, 17) == 1, Gdm: Bits(r200, 18, 18) == 1, PowerDown: Bits(R(0x5012C), 28, 28) == 1,
                Cl: Bits(r204, 5, 0), RcdRd: Bits(r204, 21, 16), RcdWr: Bits(r204, 29, 24),
                Rp: Bits(r208, 21, 16), Ras: Bits(r204, 14, 8), Rc: Bits(r208, 7, 0),
                Rrds: Bits(r20C, 4, 0), Rrdl: Bits(r20C, 12, 8), Faw: Bits(R(0x50210), 7, 0),
                Wtrs: Bits(r214, 12, 8), Wtrl: Bits(r214, 22, 16), Wr: Bits(R(0x50218), 7, 0),
                Rtp: Bits(r20C, 28, 24), Cwl: Bits(r214, 5, 0),
                Rfc: rfc, Rfc2: rfc2, RfcSb: rfcSb, Refi: Bits(R(0x50230), 15, 0));

            // Мусор вместо регистров (не тот контроллер, чтение не прошло) — не показываем.
            return timings.Cl is >= 10 and <= 80 && timings.RcdRd > 0 && timings.Ras > 0 ? timings : null;
        }

        return null;
    }

    private static (int Family, int Model) CpuFamilyModel()
    {
        if (!X86Base.IsSupported) return (0, 0);

        var (_, vb, vc, vd) = X86Base.CpuId(0, 0);
        var vendor = Encoding.ASCII.GetString(BitConverter.GetBytes(vb).Concat(BitConverter.GetBytes(vd)).Concat(BitConverter.GetBytes(vc)).ToArray());
        if (vendor != "AuthenticAMD") return (0, 0);

        var (eax, _, _, _) = X86Base.CpuId(1, 0);
        var family = (eax >> 8 & 0xF) + (eax >> 20 & 0xFF);
        var model = (eax >> 4 & 0xF) | (eax >> 12 & 0xF0);
        return (family, model);
    }

    private static int ThreadsPerCore()
    {
        var (_, ebx, _, _) = X86Base.CpuId(unchecked((int)0x8000001E), 0);
        return Math.Max(1, (ebx >> 8 & 0xFF) + 1);
    }

    /// <summary>
    /// Общий для мониторинговых программ мьютекс шины PCI (его же берёт LibreHardwareMonitor):
    /// без него запросы к SMU/SMN разных программ перемешаются.
    /// </summary>
    private sealed class PciLock : IDisposable
    {
        private readonly Mutex _mutex;
        private PciLock(Mutex mutex) => _mutex = mutex;

        public static PciLock? Take()
        {
            Mutex mutex;
            try { mutex = Mutex.OpenExisting(@"Global\Access_PCI"); }
            catch (WaitHandleCannotBeOpenedException) { mutex = new Mutex(false, @"Global\Access_PCI"); }

            try
            {
                if (mutex.WaitOne(5000)) return new PciLock(mutex);
            }
            catch (AbandonedMutexException)
            {
                return new PciLock(mutex);
            }

            mutex.Dispose();
            return null;
        }

        public void Dispose()
        {
            try { _mutex.ReleaseMutex(); } catch { /* уже отпущен */ }
            _mutex.Dispose();
        }
    }

    /// <summary>Модуль PawnIO из ресурсов LibreHardwareMonitorLib (подписан, драйвер грузит только такие).</summary>
    private sealed class PawnModule : IDisposable
    {
        private const uint IoctlLoadBinary = 2712805508u;
        private const uint IoctlExecute = 2712805636u;

        private readonly SafeFileHandle _handle;
        private PawnModule(SafeFileHandle handle) => _handle = handle;

        public static PawnModule? Load(string resource)
        {
            var handle = CreateFileW(@"\\?\GLOBALROOT\Device\PawnIO", 3, 3, IntPtr.Zero, 3, 0x80, IntPtr.Zero);
            if (handle.IsInvalid)
            {
                handle.Dispose();
                return null;
            }

            using var stream = typeof(LibreHardwareMonitor.Hardware.Computer).Assembly.GetManifestResourceStream(resource);
            if (stream is null)
            {
                handle.Dispose();
                return null;
            }

            using var memory = new MemoryStream();
            stream.CopyTo(memory);
            var blob = memory.ToArray();
            if (!DeviceIoControl(handle, IoctlLoadBinary, blob, blob.Length, null, 0, out _, IntPtr.Zero))
            {
                handle.Dispose();
                return null;
            }

            return new PawnModule(handle);
        }

        /// <summary>Вызов функции модуля; false — драйвер вернул ошибку (тогда output не трогаем).</summary>
        public bool Execute(string function, long[] input, long[] output)
        {
            var inBuffer = new byte[32 + input.Length * 8];
            Encoding.ASCII.GetBytes(function, 0, Math.Min(31, function.Length), inBuffer, 0);
            Buffer.BlockCopy(input, 0, inBuffer, 32, input.Length * 8);

            var outBuffer = new byte[output.Length * 8];
            if (!DeviceIoControl(_handle, IoctlExecute, inBuffer, inBuffer.Length, outBuffer, outBuffer.Length, out var returned, IntPtr.Zero)
                || returned < outBuffer.Length)
                return false;

            Buffer.BlockCopy(outBuffer, 0, output, 0, outBuffer.Length);
            return true;
        }

        public bool ReadSmn(uint address, out uint value)
        {
            var output = new long[1];
            var ok = Execute("ioctl_read_smn", new long[] { address }, output);
            value = ok ? (uint)output[0] : 0;
            return ok;
        }

        public void Dispose() => _handle.Dispose();

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern SafeFileHandle CreateFileW(string name, uint access, uint share, IntPtr security,
            uint creation, uint flags, IntPtr template);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool DeviceIoControl(SafeFileHandle device, uint code, byte[] inBuffer, int inSize,
            byte[]? outBuffer, int outSize, out int returned, IntPtr overlapped);
    }
}
