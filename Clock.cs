using System.Diagnostics;

namespace RigControl;

/// <summary>
/// Точные миллисекунды для анимаций. Environment.TickCount64 шагает по ~15,6 мс — кадры эффектов
/// получали неровное время, и подсветка вентиляторов «дёргалась».
/// </summary>
internal static class Clock
{
    private static readonly double MsPerTick = 1000.0 / Stopwatch.Frequency;

    public static long Ms => (long)(Stopwatch.GetTimestamp() * MsPerTick);

    public static double Seconds => Stopwatch.GetTimestamp() * MsPerTick / 1000.0;
}
