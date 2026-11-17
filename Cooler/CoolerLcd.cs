using System.Diagnostics;
using System.IO;
using System.Windows.Media;
using System.Windows.Media.Imaging;

namespace RigControl;

/// <summary>
/// Экран на помпе Thermalright (Frozen Warframe 360 и родственные), HID «Type 2»,
/// USB 0416:5302. Протокол: рукопожатие 512 байт с магией DA DB DC DD, затем кадры —
/// 20-байтный заголовок + JPEG, добитые нулями до размера отчёта.
/// Версии с USB-ID 0402:3922 (SCSI) этим классом не поддерживаются.
/// </summary>
public sealed class CoolerLcd : IDisposable
{
    public const ushort VendorId = 0x0416;
    public const ushort ProductId = 0x5302;

    private static readonly byte[] Magic = { 0xDA, 0xDB, 0xDC, 0xDD };

    private readonly HidInterface _hid;
    private readonly int _chunk;
    private readonly object _protocolLock = new();
    private bool _batchDisabled;

    public int Pm { get; private set; }
    public int Sub { get; private set; }
    public string Serial { get; private set; } = "";
    public bool HandshakeOk { get; private set; }

    /// <summary>Счётчики для сторожа и диагностики: по ним видно, идут ли кадры на самом деле.</summary>
    public long FramesSent { get; private set; }
    public int Handshakes { get; private set; }

    /// <summary>Когда ушёл последний целый кадр (Environment.TickCount64); 0 — ещё ни одного.</summary>
    public long LastFrameTick { get; private set; }

    /// <summary>Последняя ошибка передачи: программа её пережила, но кадр мог не дойти.</summary>
    public string LastError { get; private set; } = "";
    public int Errors { get; private set; }

    /// <summary>Текст для диагностики: интерфейсы, ответ рукопожатия, счётчики, последняя ошибка.</summary>
    public string Diagnostics
    {
        get
        {
            var text = _interfaceInfo + _handshakeInfo +
                       $"Кадров отправлено: {FramesSent}, рукопожатий: {Handshakes}\n";

            if (LastFrameTick != 0)
                text += $"Последний кадр: {(Environment.TickCount64 - LastFrameTick) / 1000.0:0.0} с назад\n";

            if (Errors > 0) text += $"Ошибок передачи: {Errors}, последняя: {LastError}\n";

            return text;
        }
    }

    /// <summary>Помпам с таким PM нужен RGB565, а не JPEG.</summary>
    public bool UseRgb565 => Pm == 58;

    private string _interfaceInfo = "";
    private string _handshakeInfo = "";

    /// <summary>Разрешение панели по ответу рукопожатия (по умолчанию 320×320 — 2,4″ у Warframe 360).</summary>
    public (int Width, int Height) Resolution { get; private set; } = (320, 320);

    public string Description =>
        $"USB {VendorId:X4}:{ProductId:X4} · PM {Pm} · SUB {Sub}" + (Serial.Length > 0 ? $" · {Serial}" : "");

    private CoolerLcd(HidInterface hid)
    {
        _hid = hid;

        // Один отчёт = Report ID (1 байт) + данные. Если устройство объявило иной размер — берём его.
        _chunk = hid.OutputReportLength > 64 ? hid.OutputReportLength - 1 : 512;
    }

    /// <summary>Ищет помпу, делает рукопожатие. null + текст ошибки, если не вышло.</summary>
    public static CoolerLcd? TryOpen(out string? error)
    {
        error = null;

        try
        {
            var interfaces = HidInterface.Enumerate(VendorId, ProductId);

            if (interfaces.Count == 0)
            {
                error = "Экран помпы (USB 0416:5302) не найден. Проверьте внутренний USB-кабель помпы и закройте " +
                        "программу производителя (TRCC) — она занимает устройство. Если у вас модель с ID 0402:3922 " +
                        "(SCSI), эта версия её не поддерживает.";
                return null;
            }

            var info = new System.Text.StringBuilder();
            foreach (var item in interfaces)
            {
                info.AppendLine($"HID-интерфейс: usage page 0x{item.UsagePage:X4}, " +
                                $"input {item.InputReportLength}, output {item.OutputReportLength}, feature {item.FeatureReportLength}");
            }

            // Нужен интерфейс с большими отчётами; остальные закрываем.
            var main = interfaces.OrderByDescending(i => i.OutputReportLength).First();
            foreach (var item in interfaces)
            {
                if (!ReferenceEquals(item, main)) item.Dispose();
            }

            CoolerLcd? lcd = null;
            try
            {
                lcd = new CoolerLcd(main) { _interfaceInfo = info.ToString() };
                lcd.Handshake();
                return lcd;
            }
            catch
            {
                // Handshake may throw after the HID handle was opened. Do not leak it
                // on every watchdog/reconnect attempt.
                if (lcd is not null) lcd.Dispose();
                else main.Dispose();
                throw;
            }
        }
        catch (Exception ex)
        {
            error = ex.Message;
            return null;
        }
    }

    /// <summary>Повторное рукопожатие (например, при каждом запуске показа).</summary>
    public void Reinit() => Handshake();

    /// <summary>
    /// Рукопожатие с коротким ожиданием ответа: для сторожа и профилактики перед кадром.
    /// Возвращает true, если помпа ответила по протоколу Type 2.
    /// </summary>
    public bool Reinit(int responseTimeoutMs) => Handshake(responseTimeoutMs);

    /// <summary>Есть ли помпа на USB-шине (не открывая её).</summary>
    public static bool IsPresent()
    {
        try
        {
            var list = HidInterface.Enumerate(VendorId, ProductId);
            foreach (var item in list) item.Dispose();
            return list.Count > 0;
        }
        catch
        {
            return false;
        }
    }

    private bool Handshake(int responseTimeoutMs = 1500)
    {
        lock (_protocolLock)
        {
            return HandshakeCore(responseTimeoutMs);
        }
    }

    private bool HandshakeCore(int responseTimeoutMs)
    {
        Handshakes++;
        HandshakeOk = false;
        _handshakeInfo = "";

        var init = new byte[_chunk];
        Magic.CopyTo(init, 0);
        init[12] = 0x01;

        try
        {
            _hid.WriteOutput(init);
        }
        catch (Exception ex)
        {
            _handshakeInfo = $"Рукопожатие: ошибка записи — {ex.Message}\n";
            return false;
        }

        var response = _hid.ReadReport(Math.Max(_hid.InputReportLength, _chunk + 1), responseTimeoutMs);
        if (response is null)
        {
            // Ответа нет — всё равно попробуем слать кадры.
            _handshakeInfo = "Рукопожатие: ответа нет (таймаут 1,5 с)\n";
            return false;
        }

        _handshakeInfo = $"Рукопожатие: ответ {response.Length} байт: " +
                         Convert.ToHexString(response, 0, Math.Min(48, response.Length)) + "\n";

        // На Windows первым байтом приходит Report ID, поэтому магию ищем со сдвигом 0 или 1.
        var offset = -1;
        for (var i = 0; i <= 1 && i + 6 <= response.Length; i++)
        {
            if (response.AsSpan(i, 4).SequenceEqual(Magic))
            {
                offset = i;
                break;
            }
        }

        if (offset < 0)
        {
            _handshakeInfo += "Магия DA DB DC DD в ответе не найдена — это не протокол Type 2.\n";
            return false;
        }

        HandshakeOk = true;
        Sub = response[offset + 4];
        Pm = response[offset + 5];

        if (response.Length >= offset + 36 && response[offset + 16] == 0x10)
            Serial = Convert.ToHexString(response, offset + 20, 16);

        Resolution = Pm switch
        {
            5 or 50 or 58 => (320, 240),
            7 => (640, 480),
            _ => (320, 320)
        };

        return true;
    }

    /// <summary>Отправляет один кадр (готовый JPEG) на экран.</summary>
    public void SendJpeg(byte[] jpeg, int width, int height)
    {
        if (UseRgb565)
        {
            var decoder = new JpegBitmapDecoder(new MemoryStream(jpeg), BitmapCreateOptions.PreservePixelFormat,
                BitmapCacheOption.OnLoad);
            SendRgb565(decoder.Frames[0]);
            return;
        }

        var header = new byte[20];
        Magic.CopyTo(header, 0);
        header[4] = 0x02;                                   // команда «картинка»
        header[6] = 0x00;                                   // режим: JPEG
        BitConverter.GetBytes((ushort)width).CopyTo(header, 8);
        BitConverter.GetBytes((ushort)height).CopyTo(header, 10);
        header[12] = 0x02;
        BitConverter.GetBytes((uint)jpeg.Length).CopyTo(header, 16);

        var total = header.Length + jpeg.Length;
        var padded = new byte[(total + _chunk - 1) / _chunk * _chunk];
        header.CopyTo(padded, 0);
        jpeg.CopyTo(padded, header.Length);

        WriteFrame(padded);
        FramesSent++;
    }

    /// <summary>
    /// Сколько порций кадра держим в полёте одновременно. Замер на PM58: по одной — 106 мс на кадр,
    /// две и больше — 43,5 мс (дальше не быстрее, шина уже на пределе), поэтому двух достаточно.
    /// </summary>
    public int BatchDepth { get; set; } = 2;

    /// <summary>Сколько заняла передача последнего кадра, мс (без паузы после кадра).</summary>
    public double LastFrameMs { get; private set; }

    /// <summary>
    /// Отправляет готовый кадр порциями по размеру отчёта. Если порция не ушла (таймаут USB,
    /// сбой драйвера), кадр обрывается на середине — и помпа остаётся на своём логотипе.
    /// Поэтому при сбое один раз повторяем рукопожатие и кадр целиком: это возвращает экран
    /// к показу без отключения USB.
    /// </summary>
    private void WriteFrame(byte[] padded)
    {
        // Пока идёт кадр, рукопожатие вставлять нельзя: помпа получит половину кадра и логотип.
        lock (_protocolLock)
        {
            for (var attempt = 0; ; attempt++)
            {
                try
                {
                    var started = Stopwatch.GetTimestamp();

                    // Some HID drivers accept two overlapped reports for hours and
                    // then stop completing them. Once that happens, keep the
                    // connection alive but use the conservative sequential path.
                    var batchDepth = _batchDisabled ? 1 : BatchDepth;
                    if (batchDepth > 1)
                    {
                        _hid.WriteOutputBatch(padded, _chunk, batchDepth);
                    }
                    else
                    {
                        var index = 0;
                        for (var offset = 0; offset < padded.Length; offset += _chunk)
                        {
                            _hid.WriteOutput(padded.AsSpan(offset, _chunk));
                            if (++index % 128 == 0) Thread.Sleep(1);
                        }
                    }

                    LastFrameMs = (Stopwatch.GetTimestamp() - started) * 1000.0 / Stopwatch.Frequency;
                    LastFrameTick = Environment.TickCount64;
                    return;
                }
                catch (Exception ex) when (attempt == 0 && ex is not OperationCanceledException)
                {
                    Errors++;
                    LastError = ex.Message;
                    _batchDisabled = true;
                    HandshakeCore(1500);
                }
            }
        }
    }

    /// <summary>Отправляет готовую картинку: сырой RGB565 для помп вроде PM 58, иначе JPEG.</summary>
    private BitmapSource? _jpegSource;
    private byte[]? _jpegCache;

    public void SendBitmap(BitmapSource image, int width, int height)
    {
        if (UseRgb565)
        {
            SendRgb565(image);
            return;
        }

        // Тот же кадр, что и в прошлый раз, — не кодируем JPEG заново.
        if (!ReferenceEquals(image, _jpegSource) || _jpegCache is null)
        {
            _jpegCache = LcdRender.EncodeJpeg(image);
            _jpegSource = image;
        }

        SendJpeg(_jpegCache, width, height);
    }

    private BitmapSource? _rgbSource;
    private byte[]? _rgbCache;

    /// <summary>
    /// Картинка → сырой RGB565 (младший байт первым) → кадр Type 2. Формат подсмотрен у рабочей TrccSharp:
    /// байт [6] заголовка = 1, размеры в заголовке 240×320, картинка 320×240 повёрнута на 90° по часовой,
    /// после кадра пауза 30 мс. Порции уходят пакетами (см. BatchDepth): по одной кадр идёт вдвое дольше.
    /// </summary>
    private static byte[] ToRgb565(BitmapSource source)
    {
        BitmapSource image = new FormatConvertedBitmap(source, PixelFormats.Bgr32, null, 0);
        image.Freeze();

        var turned = new TransformedBitmap(image, new RotateTransform(90));
        turned.Freeze();
        image = turned;

        var pw = image.PixelWidth;
        var ph = image.PixelHeight;
        var stride = pw * 4;
        var pixels = new byte[stride * ph];
        image.CopyPixels(pixels, stride, 0);

        var data = new byte[pw * ph * 2];
        for (int i = 0, o = 0; i < pixels.Length; i += 4, o += 2)
        {
            var b = pixels[i];
            var g = pixels[i + 1];
            var r = pixels[i + 2];
            var value = (ushort)(((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3));
            data[o] = (byte)(value & 0xFF);       // little-endian: младший байт первым
            data[o + 1] = (byte)(value >> 8);
        }
        return data;
    }

    private void SendRgb565(BitmapSource source)
    {
        // PM58 always expects a 320x240 source which is rotated to a 240x320
        // payload below. The UI permits manual sizes for other panels; normalize
        // here so the declared dimensions and payload length can never diverge.
        source = NormalizeRgb565Source(source);

        // Тот же кадр — используем уже пересчитанные пиксели.
        byte[] data;
        if (ReferenceEquals(source, _rgbSource) && _rgbCache is not null)
        {
            data = _rgbCache;
        }
        else
        {
            data = ToRgb565(source);
            _rgbCache = data;
            _rgbSource = source;
        }

        const int hw = 240, hh = 320;

        var header = new byte[20];
        Magic.CopyTo(header, 0);
        header[4] = 0x02;
        header[6] = 0x01;
        BitConverter.GetBytes((ushort)hw).CopyTo(header, 8);
        BitConverter.GetBytes((ushort)hh).CopyTo(header, 10);
        header[12] = 0x02;
        BitConverter.GetBytes((uint)data.Length).CopyTo(header, 16);

        var total = header.Length + data.Length;
        var padded = new byte[(total + _chunk - 1) / _chunk * _chunk];
        header.CopyTo(padded, 0);
        data.CopyTo(padded, header.Length);

        WriteFrame(padded);
        Thread.Sleep(30);
        FramesSent++;
    }

    private static BitmapSource NormalizeRgb565Source(BitmapSource source)
    {
        const int width = 320;
        const int height = 240;

        if (source.PixelWidth == width && source.PixelHeight == height)
            return source;

        var scale = Math.Max((double)width / source.PixelWidth, (double)height / source.PixelHeight);
        var scaledWidth = source.PixelWidth * scale;
        var scaledHeight = source.PixelHeight * scale;
        var rect = new System.Windows.Rect(
            (width - scaledWidth) / 2,
            (height - scaledHeight) / 2,
            scaledWidth,
            scaledHeight);

        var visual = new DrawingVisual();
        using (var dc = visual.RenderOpen())
        {
            dc.DrawRectangle(Brushes.Black, null, new System.Windows.Rect(0, 0, width, height));
            dc.DrawImage(source, rect);
        }

        var bitmap = new RenderTargetBitmap(width, height, 96, 96, PixelFormats.Pbgra32);
        bitmap.Render(visual);
        bitmap.Freeze();
        return bitmap;
    }

    public void Dispose()
    {
        // Shutdown/reconnect can happen while a background frame is in flight.
        // Serialize disposal with the protocol so the HID handle is not closed
        // underneath WriteFile/ReadFile.
        lock (_protocolLock)
        {
            _hid.Dispose();
        }
    }
}
