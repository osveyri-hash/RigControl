using System.Windows;
using System.Windows.Media;
using System.Windows.Media.Imaging;

namespace RigControl;

/// <summary>
/// Правильная сборка кадров GIF: у оптимизированных гифок кадр — лишь кусочек со смещением
/// (Left/Top) и способом очистки (disposal). Без этого анимация «ломается» — кусочки растягиваются
/// на весь экран. Возвращает полные кадры размером с «логический экран» гифки.
/// </summary>
public static class GifFrames
{
    /// <param name="maxFrames">Сколько кадров файла читать (остальные отбрасываются).</param>
    /// <param name="map">Что сделать с каждым готовым кадром (например, уменьшить) — чтобы не держать в памяти большие кадры.</param>
    /// <param name="keep">
    /// Сколько кадров оставить (0 — все). Лишние прореживаются ещё до <paramref name="map"/>: каждый оставленный кадр
    /// держится столько, сколько вся группа, которую он заменил, — так в памяти не лежат сотни больших кадров.
    /// </param>
    public static List<(T Image, int DelayMs)> Decode<T>(BitmapDecoder decoder, int maxFrames,
        Func<BitmapSource, T> map, int keep = 0)
    {
        var result = new List<(T, int)>();
        var total = Math.Min(decoder.Frames.Count, Math.Max(1, maxFrames));
        if (total == 0) return result;

        var target = keep > 0 ? Math.Min(keep, total) : total;
        var group = -1;

        // Кадр i попадает в группу i·target/total: первый кадр группы оставляем, задержки остальных прибавляем к нему.
        bool Starts(int i) => i * (long)target / total != group;

        void Add(int i, Func<T> image, int delay)
        {
            if (Starts(i))
            {
                group = (int)(i * (long)target / total);
                result.Add((image(), delay));
            }
            else
            {
                result[^1] = (result[^1].Item1, result[^1].Item2 + delay);
            }
        }

        var first = decoder.Frames[0];
        if (decoder is not GifBitmapDecoder || total == 1)
        {
            for (var i = 0; i < total; i++)
            {
                var frame = decoder.Frames[i];
                Add(i, () => map(frame), Delay(frame));
            }

            return result;
        }

        var (w, h) = ScreenSize(decoder, first);
        BitmapSource? canvas = null;

        for (var i = 0; i < total; i++)
        {
            var frame = decoder.Frames[i];
            var (left, top, disposal) = Descriptor(frame);
            var rect = new Rect(left, top, frame.PixelWidth, frame.PixelHeight);
            var before = canvas;
            var keepThis = Starts(i);

            // Пропускаемый кадр с очисткой «вернуть как было» ни на что не влияет — его даже не рисуем.
            if (!keepThis && disposal == 3)
            {
                Add(i, () => default!, Delay(frame));
                continue;
            }

            var visual = new DrawingVisual();
            using (var dc = visual.RenderOpen())
            {
                if (canvas is not null) dc.DrawImage(canvas, new Rect(0, 0, w, h));
                dc.DrawImage(frame, rect);
            }

            var rendered = Rasterize(visual, w, h);
            Add(i, () => map(rendered), Delay(frame));

            canvas = disposal switch
            {
                2 => Clear(rendered, rect, w, h),   // вернуть фон: область кадра становится прозрачной
                3 => before,                        // вернуть то, что было до кадра
                _ => rendered
            };
        }

        return result;
    }

    private static (int W, int H) ScreenSize(BitmapDecoder decoder, BitmapSource first)
    {
        int w = first.PixelWidth, h = first.PixelHeight;
        try
        {
            if (decoder.Metadata is BitmapMetadata m)
            {
                if (m.GetQuery("/logscrdesc/Width") is ushort lw && lw > 0) w = lw;
                if (m.GetQuery("/logscrdesc/Height") is ushort lh && lh > 0) h = lh;
            }
        }
        catch
        {
            // нет метаданных — размер первого кадра
        }

        return (w, h);
    }

    private static (int Left, int Top, int Disposal) Descriptor(BitmapFrame frame)
    {
        int left = 0, top = 0, disposal = 0;
        try
        {
            if (frame.Metadata is BitmapMetadata m)
            {
                if (m.GetQuery("/imgdesc/Left") is ushort l) left = l;
                if (m.GetQuery("/imgdesc/Top") is ushort t) top = t;
                if (m.GetQuery("/grctlext/Disposal") is byte d) disposal = d;
            }
        }
        catch
        {
            // битые метаданные — кадр целиком
        }

        return (left, top, disposal);
    }

    /// <summary>Задержка кадра: WPF хранит её в сотых долях секунды.</summary>
    public static int Delay(BitmapFrame frame)
    {
        try
        {
            if (frame.Metadata is BitmapMetadata metadata
                && metadata.GetQuery("/grctlext/Delay") is ushort delay
                && delay > 1)
                return delay * 10;
        }
        catch
        {
            // стандартная скорость
        }

        return 100;
    }

    private static BitmapSource Clear(BitmapSource image, Rect rect, int w, int h)
    {
        var visual = new DrawingVisual();
        using (var dc = visual.RenderOpen())
        {
            dc.PushClip(new CombinedGeometry(GeometryCombineMode.Exclude,
                new RectangleGeometry(new Rect(0, 0, w, h)), new RectangleGeometry(rect)));
            dc.DrawImage(image, new Rect(0, 0, w, h));
            dc.Pop();
        }

        return Rasterize(visual, w, h);
    }

    public static BitmapSource Rasterize(DrawingVisual visual, int width, int height)
    {
        var bitmap = new RenderTargetBitmap(width, height, 96, 96, PixelFormats.Pbgra32);
        bitmap.Render(visual);
        bitmap.Freeze();
        return bitmap;
    }

    /// <summary>
    /// Уменьшает кадр ровно настолько, чтобы он всё ещё закрывал экран width×height (как Stretch.UniformToFill)
    /// в обеих ориентациях — после поворота экрана перечитывать не нужно. Крупнее исходника не делает.
    /// </summary>
    public static BitmapSource Cover(BitmapSource image, int width, int height)
    {
        double iw = image.PixelWidth, ih = image.PixelHeight;
        var scale = Math.Max(Math.Max(width / iw, height / ih), Math.Max(height / iw, width / ih));
        return Scale(image, Math.Min(1.0, scale));
    }

    private static BitmapSource Scale(BitmapSource image, double scale)
    {
        var w = Math.Max(1, (int)Math.Round(image.PixelWidth * scale));
        var h = Math.Max(1, (int)Math.Round(image.PixelHeight * scale));

        var visual = new DrawingVisual();
        using (var dc = visual.RenderOpen())
            dc.DrawImage(image, new Rect(0, 0, w, h));
        return Rasterize(visual, w, h);
    }
}
