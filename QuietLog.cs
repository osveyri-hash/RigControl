using System.IO;
using System.Runtime.CompilerServices;

namespace RigControl;

/// <summary>
/// Журнал ошибок, которые программа намеренно пропускает и работает дальше: устройство не ответило,
/// датчик пропал, настройки не сохранились. Раньше такие ошибки терялись без следа.
/// Пишется в %AppData%\RigControl\quiet.log: одно и то же место в коде — не чаще раза в минуту.
/// Хранится только текущий календарный день. Сам журнал исключений не бросает.
/// </summary>
internal static class QuietLog
{
    private const long RepeatMs = 60_000;
    private const long MaxBytes = 512 * 1024;

    private static readonly object Gate = new();
    private static readonly Dictionary<string, long> LastWritten = new();

    public static void Write(Exception ex, [CallerFilePath] string file = "", [CallerLineNumber] int line = 0)
    {
        try
        {
            var site = Path.GetFileName(file) + ":" + line;
            var key = site + " " + ex.GetType().FullName;
            var now = Environment.TickCount64;

            lock (Gate)
            {
                if (LastWritten.TryGetValue(key, out var last) && now - last < RepeatMs) return;
                LastWritten[key] = now;

                LogFile.Append("quiet.log",
                    $"{DateTime.Now:yyyy-MM-dd HH:mm:ss}  {site}  {ex}{Environment.NewLine}{Environment.NewLine}",
                    MaxBytes);
            }
        }
        catch
        {
            // журнал не должен мешать работе
        }
    }
}
