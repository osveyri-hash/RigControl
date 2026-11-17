using System.IO;
using System.Windows;
using System.Windows.Media;
using System.Windows.Media.Imaging;

namespace RigControl;

/// <summary>
/// Картинка или гифка → кадры 128×128 в RGB565 для экрана клавиатуры.
/// Кадры гифки собираются поверх предыдущего кадра, чтобы прозрачные
/// области не превращались в чёрные дыры.
/// </summary>
public static class ImageFrames
{
    public static (List<ScreenFrame> Frames, BitmapSource Preview) Load(string path, bool crop, int maxFrames = 0)
    {
        using var stream = File.OpenRead(path);

        var decoder = BitmapDecoder.Create(stream,
            BitmapCreateOptions.PreservePixelFormat, BitmapCacheOption.OnLoad);

        if (decoder.Frames.Count == 0)
            throw new InvalidOperationException("В файле нет ни одного кадра");

        var frames = new List<ScreenFrame>();
        BitmapSource? preview = null;

        // Гифка собирается в полные кадры (смещения и очистка), затем каждый вписывается в 128×128.
        foreach (var (rendered, delay) in GifFrames.Decode(decoder, Ak820Protocol.MaxFrames, full => Render(full, null, crop)))
        {
            preview ??= rendered;
            frames.Add(new ScreenFrame(ToRgb565(rendered), delay));
        }

        return (Decimate(frames, maxFrames), preview!);
    }

    /// <summary>
    /// Оставляет не больше <paramref name="max"/> кадров, равномерно по времени:
    /// каждый оставшийся кадр держится столько же, сколько вся группа, которую он заменил.
    /// Размер заливки пропорционален числу кадров (32 КиБ на кадр).
    /// </summary>
    private static List<ScreenFrame> Decimate(List<ScreenFrame> frames, int max)
    {
        if (max <= 0 || frames.Count <= max) return frames;

        var result = new List<ScreenFrame>(max);
        for (var i = 0; i < max; i++)
        {
            var start = i * frames.Count / max;
            var end = (i + 1) * frames.Count / max;

            var delay = 0;
            for (var j = start; j < end; j++) delay += frames[j].DelayMs;

            result.Add(new ScreenFrame(frames[start].Rgb565, delay));
        }

        return result;
    }

    private static RenderTargetBitmap Render(BitmapSource frame, BitmapSource? previous, bool crop)
    {
        const double size = Ak820Protocol.ScreenSize;

        var scale = crop
            ? Math.Max(size / frame.PixelWidth, size / frame.PixelHeight)
            : Math.Min(size / frame.PixelWidth, size / frame.PixelHeight);

        var width = frame.PixelWidth * scale;
        var height = frame.PixelHeight * scale;
        var rect = new Rect((size - width) / 2, (size - height) / 2, width, height);

        var visual = new DrawingVisual();
        using (var dc = visual.RenderOpen())
        {
            dc.DrawRectangle(Brushes.Black, null, new Rect(0, 0, size, size));
            if (previous is not null) dc.DrawImage(previous, new Rect(0, 0, size, size));
            dc.DrawImage(frame, rect);
        }

        var bitmap = new RenderTargetBitmap(Ak820Protocol.ScreenSize, Ak820Protocol.ScreenSize,
            96, 96, PixelFormats.Pbgra32);
        bitmap.Render(visual);
        bitmap.Freeze();
        return bitmap;
    }

    /// <summary>BGRA32 → RGB565 little-endian, построчно, без выравнивания.</summary>
    private static byte[] ToRgb565(BitmapSource bitmap)
    {
        var width = bitmap.PixelWidth;
        var height = bitmap.PixelHeight;
        var bgra = new byte[width * height * 4];

        var converted = new FormatConvertedBitmap(bitmap, PixelFormats.Bgra32, null, 0);
        converted.CopyPixels(bgra, width * 4, 0);

        var rgb = new byte[width * height * 2];
        for (int i = 0, o = 0; i < bgra.Length; i += 4, o += 2)
        {
            var b = bgra[i];
            var g = bgra[i + 1];
            var r = bgra[i + 2];

            var value = (ushort)(((r & 0xF8) << 8) | ((g & 0xFC) << 3) | (b >> 3));
            rgb[o] = (byte)(value & 0xFF);
            rgb[o + 1] = (byte)(value >> 8);
        }

        return rgb;
    }

}
