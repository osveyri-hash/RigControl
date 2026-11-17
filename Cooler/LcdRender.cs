using System.Globalization;
using System.IO;
using System.Windows;
using System.Windows.Media;
using System.Windows.Media.Imaging;

namespace RigControl;

/// <summary>Один кадр для экрана помпы: готовый JPEG и задержка показа.</summary>
public readonly record struct LcdFrame(byte[] Jpeg, int DelayMs);

/// <summary>Рисование картинок для экрана помпы: фото/гифка, JPEG-кодирование.</summary>
public static class LcdRender
{
    // ── Картинка / гифка ─────────────────────────────────────────────────────

    /// <summary>Картинка или гифка → JPEG-кадры под экран (с поворотом). Первым возвращается и предпросмотр.</summary>
    public static (List<LcdFrame> Frames, BitmapSource Preview) LoadImage(
        string path, int width, int height, int rotation, bool crop, int maxFrames)
    {
        using var stream = File.OpenRead(path);

        // Кадры читаются из открытого файла по одному — без копии всей гифки в памяти.
        var decoder = BitmapDecoder.Create(stream,
            BitmapCreateOptions.PreservePixelFormat, BitmapCacheOption.None);

        if (decoder.Frames.Count == 0)
            throw new InvalidOperationException("В файле нет ни одного кадра");

        // Рисуем в «повёрнутых» размерах, потом поворачиваем под панель.
        var swap = rotation is 90 or 270;
        var drawW = swap ? height : width;
        var drawH = swap ? width : height;

        // Кадры гифки собираются целиком (смещения и очистка), прореживаются по времени (чтобы не слать лишнего),
        // и только оставшиеся вписываются в экран и поворачиваются.
        // Каждый кадр сразу сжимается в JPEG для помпы — готовые картинки в памяти не копятся.
        BitmapSource? preview = null;
        var frames = GifFrames.Decode(decoder, 300, full =>
            {
                var image = Rotate(Compose(full, null, drawW, drawH, crop), rotation);
                preview ??= image;
                return EncodeJpeg(image);
            }, maxFrames)
            .Select(f => new LcdFrame(f.Image, Math.Max(20, f.DelayMs))).ToList();

        return (frames, preview!);
    }

    private static RenderTargetBitmap Compose(BitmapSource frame, BitmapSource? previous, int width, int height, bool crop)
    {
        var scale = crop
            ? Math.Max((double)width / frame.PixelWidth, (double)height / frame.PixelHeight)
            : Math.Min((double)width / frame.PixelWidth, (double)height / frame.PixelHeight);

        var w = frame.PixelWidth * scale;
        var h = frame.PixelHeight * scale;
        var rect = new Rect((width - w) / 2, (height - h) / 2, w, h);

        var visual = new DrawingVisual();
        using (var dc = visual.RenderOpen())
        {
            dc.DrawRectangle(Brushes.Black, null, new Rect(0, 0, width, height));
            if (previous is not null) dc.DrawImage(previous, new Rect(0, 0, width, height));
            dc.DrawImage(frame, rect);
        }

        return Rasterize(visual, width, height);
    }

    private static RenderTargetBitmap Rasterize(DrawingVisual visual, int width, int height)
    {
        var bitmap = new RenderTargetBitmap(width, height, 96, 96, PixelFormats.Pbgra32);
        bitmap.Render(visual);
        bitmap.Freeze();
        return bitmap;
    }

    /// <summary>Встроенная картинка программы (pack://…), а не файл на диске.</summary>
    public static bool IsResource(string path) => path.StartsWith("pack://", StringComparison.OrdinalIgnoreCase);

    /// <summary>
    /// Фон экрана из картинки или гифки. Кадры уменьшаются ровно до размера, закрывающего экран width×height,
    /// гифка прореживается до maxFrames ещё до уменьшения, а её кадры хранятся сжатыми (JPEG) —
    /// в памяти это в десятки раз меньше готовых картинок. Animation — null у обычной картинки.
    /// </summary>
    public static (BitmapSource First, List<(byte[] Jpeg, int DelayMs)>? Animation) LoadBackground(
        string path, int width, int height, int maxFrames)
    {
        using var stream = IsResource(path)
            ? Application.GetResourceStream(new Uri(path))?.Stream ?? throw new FileNotFoundException(path)
            : File.OpenRead(path);
        var decoder = BitmapDecoder.Create(stream, BitmapCreateOptions.PreservePixelFormat, BitmapCacheOption.None);
        if (decoder.Frames.Count == 0)
            throw new InvalidOperationException("В файле нет ни одного кадра");

        if (decoder.Frames.Count == 1)
            return (GifFrames.Cover(decoder.Frames[0], width, height), null);

        BitmapSource? first = null;
        var frames = GifFrames.Decode(decoder, 300, full =>
        {
            var image = GifFrames.Cover(full, width, height);
            first ??= image;
            return EncodeJpeg(image, 92);
        }, maxFrames);

        return frames.Count > 1 ? (first!, frames) : (first!, null);
    }

    /// <summary>
    /// Распаковывает сжатый кадр фона-гифки в одну и ту же картинку <paramref name="target"/> (создаётся при первом
    /// вызове) — без новой картинки и нового буфера на каждый кадр.
    /// </summary>
    public static WriteableBitmap DecodeJpegInto(byte[] data, ref WriteableBitmap? target, ref byte[]? pixels)
    {
        using var memory = new MemoryStream(data, writable: false);
        var frame = BitmapFrame.Create(memory, BitmapCreateOptions.None, BitmapCacheOption.None);
        BitmapSource source = frame.Format == PixelFormats.Pbgra32
            ? frame
            : new FormatConvertedBitmap(frame, PixelFormats.Pbgra32, null, 0);

        int w = source.PixelWidth, h = source.PixelHeight, stride = w * 4;
        if (target is null || target.PixelWidth != w || target.PixelHeight != h || pixels is null || pixels.Length != stride * h)
        {
            target = new WriteableBitmap(w, h, 96, 96, PixelFormats.Pbgra32, null);
            pixels = new byte[stride * h];
        }

        source.CopyPixels(pixels, stride, 0);
        target.WritePixels(new Int32Rect(0, 0, w, h), pixels, stride, 0);
        return target;
    }

    /// <summary>Поворот по часовой стрелке на 0/90/180/270°.</summary>
    public static BitmapSource Rotate(BitmapSource source, int degrees)
    {
        if (degrees % 360 == 0) return source;

        var rotated = new TransformedBitmap(source, new RotateTransform(degrees));
        rotated.Freeze();
        return rotated;
    }

    public static byte[] EncodeJpeg(BitmapSource image, int quality = 85)
    {
        var encoder = new JpegBitmapEncoder { QualityLevel = quality };
        encoder.Frames.Add(BitmapFrame.Create(image));

        using var memory = new MemoryStream();
        encoder.Save(memory);
        return memory.ToArray();
    }

}
