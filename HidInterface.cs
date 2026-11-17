using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;

namespace RigControl;

/// <summary>
/// Один HID-интерфейс клавиатуры. На AK820 Pro их несколько, и работать нужно
/// с двумя вендорскими: 0xFF13 (команды, 64-байтные feature-отчёты) и
/// 0xFF68 (данные экрана, 4096-байтные output-отчёты).
/// </summary>
public sealed class HidInterface : IDisposable
{
    private const uint GenericRead = 0x80000000;
    private const uint GenericWrite = 0x40000000;
    private const uint FileShareReadWrite = 0x00000003;
    private const uint OpenExisting = 3;
    private const uint FileFlagOverlapped = 0x40000000;
    private const int ErrorIoPending = 997;
    private const uint WaitObject0 = 0;
    private const uint WaitTimeout = 0x102;

    private readonly SafeFileHandle _handle;
    private readonly ManualResetEvent _ioDone = new(false);
    private readonly object _ioLock = new();
    private byte[] _writeBuffer;

    public string Path { get; }
    public ushort VendorId { get; }
    public ushort ProductId { get; }
    public ushort UsagePage { get; }
    public int FeatureReportLength { get; }
    public int OutputReportLength { get; }
    public int InputReportLength { get; }

    private HidInterface(string path, SafeFileHandle handle, ushort vid, ushort pid, HIDP_CAPS caps)
    {
        Path = path;
        _handle = handle;
        VendorId = vid;
        ProductId = pid;
        UsagePage = caps.UsagePage;
        FeatureReportLength = caps.FeatureReportByteLength;
        OutputReportLength = caps.OutputReportByteLength;
        InputReportLength = caps.InputReportByteLength;
        _writeBuffer = new byte[Math.Max(1, OutputReportLength)];
    }

    /// <summary>Все HID-интерфейсы указанных VID/PID, которые сейчас на шине.</summary>
    public static List<HidInterface> Enumerate(ushort vendorId, params ushort[] productIds) =>
        EnumerateWhere(i => i.VendorId == vendorId && (productIds.Length == 0 || productIds.Contains(i.ProductId)));

    /// <summary>Вендорские интерфейсы (usage page 0xFF00+) всех HID-устройств — чтобы увидеть, что отдаёт донгл 2,4 ГГц.</summary>
    public static List<string> DescribeVendorInterfaces()
    {
        var items = EnumerateWhere(i => i.UsagePage >= 0xFF00);
        var lines = items
            .Select(i => $"{i.VendorId:X4}:{i.ProductId:X4} 0x{i.UsagePage:X4} (in {i.InputReportLength}, out {i.OutputReportLength}, feat {i.FeatureReportLength})")
            .Distinct()
            .ToList();

        foreach (var item in items) item.Dispose();
        return lines;
    }

    private static List<HidInterface> EnumerateWhere(Func<HidInterface, bool> keep, Func<string, bool>? pathFilter = null)
    {
        var found = new List<HidInterface>();
        var guid = HidGuid();
        var set = SetupDiGetClassDevs(ref guid, IntPtr.Zero, IntPtr.Zero,
            DigcfPresent | DigcfDeviceInterface);

        if (set == IntPtr.Zero || set == new IntPtr(-1))
            throw new Win32Exception(Marshal.GetLastWin32Error(), "SetupDiGetClassDevs");

        try
        {
            for (uint index = 0; ; index++)
            {
                var data = new SP_DEVICE_INTERFACE_DATA { cbSize = Marshal.SizeOf<SP_DEVICE_INTERFACE_DATA>() };

                if (!SetupDiEnumDeviceInterfaces(set, IntPtr.Zero, ref guid, index, ref data))
                {
                    var err = Marshal.GetLastWin32Error();
                    if (err == 259) break; // ERROR_NO_MORE_ITEMS
                    throw new Win32Exception(err, "SetupDiEnumDeviceInterfaces");
                }

                var path = GetDevicePath(set, ref data);
                if (path is null) continue;
                if (pathFilter is not null && !pathFilter(path)) continue;   // чужие устройства даже не открываем

                var probe = TryOpen(path);
                if (probe is null) continue;

                if (!keep(probe))
                {
                    probe.Dispose();
                    continue;
                }

                found.Add(probe);
            }
        }
        finally
        {
            SetupDiDestroyDeviceInfoList(set);
        }

        return found;
    }

    /// <summary>Все HID-интерфейсы, подходящие под условие (например, мыши разных производителей).</summary>
    public static List<HidInterface> EnumerateMatching(Func<HidInterface, bool> keep, Func<string, bool>? pathFilter = null) =>
        EnumerateWhere(keep, pathFilter);

    /// <summary>Имя устройства из USB (как «MCHOSE A7 Ultra»), null — не отдало.</summary>
    public string? ProductName()
    {
        ThrowIfDisposed();
        var buffer = new byte[256];
        lock (_ioLock)
        {
            if (!HidD_GetProductString(_handle, buffer, buffer.Length)) return null;
        }

        var text = System.Text.Encoding.Unicode.GetString(buffer);
        var end = text.IndexOf('\0');
        return (end >= 0 ? text[..end] : text).Trim();
    }

    /// <summary>
    /// Feature-отчёт с настоящим Report ID: [id, данные…]. Сначала длиной ровно под данные,
    /// если Windows не примет — длиной самого большого feature-отчёта интерфейса.
    /// </summary>
    public void SetFeatureReport(byte reportId, ReadOnlySpan<byte> payload, byte pad = 0)
    {
        ThrowIfDisposed();

        foreach (var length in FeatureLengths(payload.Length + 1))
        {
            var buffer = new byte[length];
            if (pad != 0) Array.Fill(buffer, pad);
            buffer[0] = reportId;
            payload[..Math.Min(payload.Length, length - 1)].CopyTo(buffer.AsSpan(1));

            lock (_ioLock)
            {
                if (HidD_SetFeature(_handle, buffer, buffer.Length)) return;
            }
        }

        throw new Win32Exception(Marshal.GetLastWin32Error(), "HidD_SetFeature");
    }

    /// <summary>Чтение feature-отчёта с Report ID: возвращает [id, данные…].</summary>
    public byte[] GetFeatureReport(byte reportId, int payloadLength)
    {
        ThrowIfDisposed();

        foreach (var length in FeatureLengths(payloadLength + 1))
        {
            var buffer = new byte[length];
            buffer[0] = reportId;

            lock (_ioLock)
            {
                if (HidD_GetFeature(_handle, buffer, buffer.Length)) return buffer;
            }
        }

        throw new Win32Exception(Marshal.GetLastWin32Error(), "HidD_GetFeature");
    }

    private IEnumerable<int> FeatureLengths(int exact)
    {
        yield return exact;
        if (FeatureReportLength > 0 && FeatureReportLength != exact) yield return FeatureReportLength;
    }

    /// <summary>Интерфейсы производителя с нужной usage page (например, ASUS Aura — 0x0B05, 0xFF72).</summary>
    public static List<HidInterface> EnumerateUsagePage(ushort vendorId, ushort usagePage) =>
        EnumerateWhere(i => i.VendorId == vendorId && i.UsagePage == usagePage);

    private static HidInterface? TryOpen(string path)
    {
        var handle = CreateFile(path, GenericRead | GenericWrite, FileShareReadWrite,
            IntPtr.Zero, OpenExisting, FileFlagOverlapped, IntPtr.Zero);

        if (handle.IsInvalid)
        {
            handle.Dispose();
            return null;
        }

        if (!HidD_GetAttributes(handle, out var attr) || !HidD_GetPreparsedData(handle, out var preparsed))
        {
            handle.Dispose();
            return null;
        }

        try
        {
            var caps = new HIDP_CAPS();
            if (HidP_GetCaps(preparsed, ref caps) != 0x00110000)
            {
                handle.Dispose();
                return null;
            }

            return new HidInterface(path, handle, attr.VendorID, attr.ProductID, caps);
        }
        finally
        {
            HidD_FreePreparsedData(preparsed);
        }
    }

    private static string? GetDevicePath(IntPtr set, ref SP_DEVICE_INTERFACE_DATA data)
    {
        SetupDiGetDeviceInterfaceDetail(set, ref data, IntPtr.Zero, 0, out var size, IntPtr.Zero);
        if (size == 0) return null;

        var buffer = Marshal.AllocHGlobal((int)size);
        try
        {
            // cbSize у этой структуры — 6 на x86 и 8 на x64 (без массива пути).
            Marshal.WriteInt32(buffer, IntPtr.Size == 8 ? 8 : 6);

            if (!SetupDiGetDeviceInterfaceDetail(set, ref data, buffer, size, out _, IntPtr.Zero))
                return null;

            return Marshal.PtrToStringUni(buffer + 4);
        }
        finally
        {
            Marshal.FreeHGlobal(buffer);
        }
    }

    /// <summary>
    /// Feature-отчёт: команда (64 байта данных). Настоящий Report ID — 0,
    /// поэтому в буфер для Windows он идёт первым байтом.
    /// </summary>
    public void SetFeature(ReadOnlySpan<byte> data)
    {
        ThrowIfDisposed();

        var buffer = new byte[65];
        data[..Math.Min(data.Length, 64)].CopyTo(buffer.AsSpan(1));

        lock (_ioLock)
        {
            if (!HidD_SetFeature(_handle, buffer, buffer.Length))
                throw new Win32Exception(Marshal.GetLastWin32Error(), "HidD_SetFeature");
        }
    }

    /// <summary>Чтение feature-отчёта: панель отвечает коротким ACK, содержимое нам не нужно.</summary>
    public byte[] GetFeature()
    {
        ThrowIfDisposed();

        var buffer = new byte[65];
        buffer[0] = 0;

        lock (_ioLock)
        {
            if (!HidD_GetFeature(_handle, buffer, buffer.Length))
                throw new Win32Exception(Marshal.GetLastWin32Error(), "HidD_GetFeature");

            return buffer[1..];
        }
    }

    /// <summary>Output-отчёт 4096 байт (кадр экрана) — через перекрытую запись с таймаутом.</summary>
    public void WriteOutput(ReadOnlySpan<byte> data, int timeoutMs = 2000)
    {
        ThrowIfDisposed();

        var buffer = new byte[1 + data.Length];
        data.CopyTo(buffer.AsSpan(1));

        lock (_ioLock)
        {
            var handle = GCHandle.Alloc(buffer, GCHandleType.Pinned);
            try
            {
                var written = Transfer(handle.AddrOfPinnedObject(), buffer.Length, write: true, timeoutMs);
                if (written != buffer.Length)
                    throw new IOException($"Записано {written} из {buffer.Length} байт отчёта");
            }
            finally
            {
                handle.Free();
            }
        }
    }

    /// <summary>
    /// Кадр порциями по <paramref name="reportSize"/> байт, держа в полёте до <paramref name="pending"/>
    /// записей одновременно: следующая порция уходит в драйвер, пока предыдущая ещё идёт по шине,
    /// и пауза на круг «запись — готово» перестаёт складываться на каждой порции.
    /// </summary>
    public void WriteOutputBatch(ReadOnlySpan<byte> data, int reportSize, int pending, int timeoutMs = 2000)
    {
        ThrowIfDisposed();

        ArgumentOutOfRangeException.ThrowIfNegativeOrZero(reportSize);
        if (data.Length == 0) return;
        if (data.Length % reportSize != 0)
            throw new ArgumentException("Длина данных должна быть кратна размеру отчёта", nameof(data));

        var count = data.Length / reportSize;
        pending = Math.Clamp(pending, 1, Math.Min(count, 32));

        if (pending == 1)
        {
            for (var offset = 0; offset < data.Length; offset += reportSize)
                WriteOutput(data.Slice(offset, reportSize), timeoutMs);
            return;
        }

        // Один общий буфер: перед каждой порцией — нулевой Report ID. Буфер пришпилен на всё время
        // передачи, потому что драйвер читает его уже после возврата из WriteFile.
        var stride = reportSize + 1;
        var buffer = new byte[count * stride];
        for (var i = 0; i < count; i++)
            data.Slice(i * reportSize, reportSize).CopyTo(buffer.AsSpan(i * stride + 1));

        var events = new ManualResetEvent[pending];
        var handles = new IntPtr[pending];
        for (var i = 0; i < pending; i++)
        {
            events[i] = new ManualResetEvent(false);
            handles[i] = events[i].SafeWaitHandle.DangerousGetHandle();
        }

        var overlappedSize = Marshal.SizeOf<NativeOverlapped>();
        var overlappedPtr = Marshal.AllocHGlobal(overlappedSize * pending);
        var pinned = GCHandle.Alloc(buffer, GCHandleType.Pinned);
        var inFlight = new bool[pending];
        var waitHandles = new IntPtr[pending];
        var waitSlots = new int[pending];

        try
        {
            var bufferPtr = pinned.AddrOfPinnedObject();

            lock (_ioLock)
            {
                var next = 0;
                var done = 0;
                var active = 0;

                while (done < count)
                {
                    while (active < pending && next < count)
                    {
                        var slot = Array.IndexOf(inFlight, false);
                        var overlapped = IntPtr.Add(overlappedPtr, slot * overlappedSize);

                        Marshal.StructureToPtr(new NativeOverlapped { EventHandle = handles[slot] },
                            overlapped, false);
                        events[slot].Reset();

                        var ptr = IntPtr.Add(bufferPtr, next * stride);
                        if (!WriteFile(_handle, ptr, (uint)stride, out _, overlapped))
                        {
                            var error = Marshal.GetLastWin32Error();
                            if (error != ErrorIoPending)
                            {
                                CancelPending(overlappedPtr, overlappedSize, inFlight);
                                throw new Win32Exception(error, "WriteFile (HID)");
                            }
                        }

                        inFlight[slot] = true;
                        next++;
                        active++;
                    }

                    if (active == 0) break;

                    var waiting = 0;
                    for (var i = 0; i < pending; i++)
                    {
                        if (!inFlight[i]) continue;
                        waitHandles[waiting] = handles[i];
                        waitSlots[waiting] = i;
                        waiting++;
                    }

                    var result = WaitForMultipleObjects((uint)waiting, waitHandles, false, (uint)timeoutMs);
                    if (result == WaitTimeout)
                    {
                        CancelPending(overlappedPtr, overlappedSize, inFlight);
                        throw new IOException("Таймаут записи кадра на экран помпы");
                    }

                    if (result < WaitObject0 || result >= WaitObject0 + (uint)waiting)
                    {
                        var error = Marshal.GetLastWin32Error();
                        CancelPending(overlappedPtr, overlappedSize, inFlight);
                        throw new Win32Exception(error, "WaitForMultipleObjects");
                    }

                    var finished = waitSlots[result - WaitObject0];
                    var finishedOverlapped = IntPtr.Add(overlappedPtr, finished * overlappedSize);
                    if (!GetOverlappedResult(_handle, finishedOverlapped, out var transferred, false))
                    {
                        var error = Marshal.GetLastWin32Error();
                        CancelPending(overlappedPtr, overlappedSize, inFlight);
                        throw new Win32Exception(error, "GetOverlappedResult");
                    }

                    if (transferred != stride)
                    {
                        CancelPending(overlappedPtr, overlappedSize, inFlight);
                        throw new IOException($"Записано {transferred} из {stride} байт отчёта");
                    }

                    inFlight[finished] = false;
                    active--;
                    done++;
                }
            }
        }
        finally
        {
            pinned.Free();
            Marshal.FreeHGlobal(overlappedPtr);
            foreach (var item in events) item.Dispose();
        }
    }

    /// <summary>Отменяет незавершённые записи и дожидается их, чтобы можно было освободить буферы.</summary>
    private void CancelPending(IntPtr overlappedPtr, int overlappedSize, bool[] inFlight)
    {
        for (var i = 0; i < inFlight.Length; i++)
        {
            if (!inFlight[i]) continue;

            var overlapped = IntPtr.Add(overlappedPtr, i * overlappedSize);
            CancelIoEx(_handle, overlapped);

            // Do not free the pinned buffer or OVERLAPPED structure until the
            // kernel has completed the cancelled request.
            GetOverlappedResult(_handle, overlapped, out _, wait: true);
            inFlight[i] = false;
        }
    }

    /// <summary>
    /// Output-отчёт как есть: первый байт — настоящий Report ID (у ASUS Aura — 0xEC).
    /// Короткий отчёт дополняется нулями до длины, которую ждёт устройство.
    /// </summary>
    public void WriteReport(ReadOnlySpan<byte> report, int timeoutMs = 500)
    {
        ThrowIfDisposed();

        lock (_ioLock)
        {
            if (_writeBuffer.Length < report.Length)
                _writeBuffer = new byte[report.Length];

            // HID-отчёты короче буфера должны быть дополнены нулями.
            // Буфер принадлежит одному интерфейсу и защищён _ioLock, поэтому
            // ARGB больше не создаёт новый массив на каждой отправке кадра.
            Array.Clear(_writeBuffer);
            report.CopyTo(_writeBuffer);

            var handle = GCHandle.Alloc(_writeBuffer, GCHandleType.Pinned);
            try
            {
                var written = Transfer(handle.AddrOfPinnedObject(), _writeBuffer.Length, write: true, timeoutMs);
                if (written != _writeBuffer.Length)
                    throw new IOException($"Записано {written} из {_writeBuffer.Length} байт отчёта");
            }
            finally
            {
                handle.Free();
            }
        }
    }

    /// <summary>ACK после порции данных: сколько пришло — неважно, важен сам факт чтения.</summary>
    public int ReadInput(int size, int timeoutMs = 300)
    {
        ThrowIfDisposed();

        var buffer = new byte[Math.Max(1, Math.Min(size, 4096))];

        lock (_ioLock)
        {
            var handle = GCHandle.Alloc(buffer, GCHandleType.Pinned);
            try
            {
                return Transfer(handle.AddrOfPinnedObject(), buffer.Length, write: false, timeoutMs);
            }
            finally
            {
                handle.Free();
            }
        }
    }

    /// <summary>Читает input-отчёт и возвращает его байты (первый байт — Report ID); null — таймаут.</summary>
    public byte[]? ReadReport(int size, int timeoutMs = 1000)
    {
        ThrowIfDisposed();

        var buffer = new byte[Math.Max(1, Math.Min(size, 65536))];

        lock (_ioLock)
        {
            var handle = GCHandle.Alloc(buffer, GCHandleType.Pinned);
            try
            {
                var count = Transfer(handle.AddrOfPinnedObject(), buffer.Length, write: false, timeoutMs);
                return count > 0 ? buffer[..count] : null;
            }
            finally
            {
                handle.Free();
            }
        }
    }

    private int Transfer(IntPtr buffer, int length, bool write, int timeoutMs)
    {
        _ioDone.Reset();
        var overlapped = new NativeOverlapped { EventHandle = _ioDone.SafeWaitHandle.DangerousGetHandle() };

        var ok = write
            ? WriteFile(_handle, buffer, (uint)length, out _, ref overlapped)
            : ReadFile(_handle, buffer, (uint)length, out _, ref overlapped);

        if (!ok)
        {
            var err = Marshal.GetLastWin32Error();
            if (err != ErrorIoPending)
                throw new Win32Exception(err, write ? "WriteFile (HID)" : "ReadFile (HID)");
        }

        if (!_ioDone.WaitOne(timeoutMs))
        {
            // Отменённую операцию обязательно «собираем», иначе её сигнал
            // достанется следующему вызову — событие одно на интерфейс.
            CancelIoEx(_handle, ref overlapped);
            GetOverlappedResult(_handle, ref overlapped, out _, wait: true);
            return 0;
        }

        if (!GetOverlappedResult(_handle, ref overlapped, out var transferred, false))
            throw new Win32Exception(Marshal.GetLastWin32Error(), "GetOverlappedResult");

        return (int)transferred;
    }

    private void ThrowIfDisposed()
    {
        ObjectDisposedException.ThrowIf(_disposed, this);
    }

    private bool _disposed;

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;

        lock (_ioLock)
        {
            CancelIoEx(_handle, IntPtr.Zero);
            _handle.Dispose();
        }

        _ioDone.Dispose();
    }

    // ── Win32 ────────────────────────────────────────────────────────────────

    private const uint DigcfPresent = 0x00000002;
    private const uint DigcfDeviceInterface = 0x00000010;

    private static Guid HidGuid()
    {
        HidD_GetHidGuid(out var guid);
        return guid;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct SP_DEVICE_INTERFACE_DATA
    {
        public int cbSize;
        public Guid InterfaceClassGuid;
        public int Flags;
        public IntPtr Reserved;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct HIDD_ATTRIBUTES
    {
        public int Size;
        public ushort VendorID;
        public ushort ProductID;
        public ushort VersionNumber;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct HIDP_CAPS
    {
        public ushort Usage;
        public ushort UsagePage;
        public ushort InputReportByteLength;
        public ushort OutputReportByteLength;
        public ushort FeatureReportByteLength;

        [MarshalAs(UnmanagedType.ByValArray, SizeConst = 17)]
        public ushort[] Reserved;

        public ushort NumberLinkCollectionNodes;
        public ushort NumberInputButtonCaps;
        public ushort NumberInputValueCaps;
        public ushort NumberInputDataIndices;
        public ushort NumberOutputButtonCaps;
        public ushort NumberOutputValueCaps;
        public ushort NumberOutputDataIndices;
        public ushort NumberFeatureButtonCaps;
        public ushort NumberFeatureValueCaps;
        public ushort NumberFeatureDataIndices;
    }

    [DllImport("hid.dll")]
    private static extern void HidD_GetHidGuid(out Guid guid);

    [DllImport("hid.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool HidD_GetAttributes(SafeFileHandle handle, out HIDD_ATTRIBUTES attributes);

    [DllImport("hid.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool HidD_GetPreparsedData(SafeFileHandle handle, out IntPtr preparsed);

    [DllImport("hid.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool HidD_FreePreparsedData(IntPtr preparsed);

    [DllImport("hid.dll")]
    private static extern int HidP_GetCaps(IntPtr preparsed, ref HIDP_CAPS caps);

    [DllImport("hid.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool HidD_SetFeature(SafeFileHandle handle, byte[] buffer, int length);

    [DllImport("hid.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool HidD_GetProductString(SafeFileHandle handle, byte[] buffer, int length);

    [DllImport("hid.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool HidD_GetFeature(SafeFileHandle handle, byte[] buffer, int length);

    [DllImport("setupapi.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr SetupDiGetClassDevs(ref Guid classGuid, IntPtr enumerator,
        IntPtr hwndParent, uint flags);

    [DllImport("setupapi.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetupDiEnumDeviceInterfaces(IntPtr deviceInfoSet, IntPtr deviceInfoData,
        ref Guid interfaceClassGuid, uint memberIndex, ref SP_DEVICE_INTERFACE_DATA deviceInterfaceData);

    [DllImport("setupapi.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetupDiGetDeviceInterfaceDetail(IntPtr deviceInfoSet,
        ref SP_DEVICE_INTERFACE_DATA deviceInterfaceData, IntPtr deviceInterfaceDetailData,
        uint deviceInterfaceDetailDataSize, out uint requiredSize, IntPtr deviceInfoData);

    [DllImport("setupapi.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetupDiDestroyDeviceInfoList(IntPtr deviceInfoSet);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern SafeFileHandle CreateFile(string path, uint access, uint share, IntPtr sa,
        uint disposition, uint flags, IntPtr template);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool WriteFile(SafeFileHandle handle, IntPtr buffer, uint count,
        out uint written, ref NativeOverlapped overlapped);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool WriteFile(SafeFileHandle handle, IntPtr buffer, uint count,
        out uint written, IntPtr overlapped);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern uint WaitForMultipleObjects(uint count, IntPtr[] handles,
        [MarshalAs(UnmanagedType.Bool)] bool waitAll, uint milliseconds);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool ReadFile(SafeFileHandle handle, IntPtr buffer, uint count,
        out uint read, ref NativeOverlapped overlapped);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetOverlappedResult(SafeFileHandle handle, ref NativeOverlapped overlapped,
        out uint transferred, [MarshalAs(UnmanagedType.Bool)] bool wait);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetOverlappedResult(SafeFileHandle handle, IntPtr overlapped,
        out uint transferred, [MarshalAs(UnmanagedType.Bool)] bool wait);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CancelIoEx(SafeFileHandle handle, IntPtr overlapped);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CancelIoEx(SafeFileHandle handle, ref NativeOverlapped overlapped);
}
