using System.IO;

namespace RigControl;

/// <summary>
/// Общая запись журналов программы. Храним только записи текущего календарного дня:
/// при первом обращении после полуночи старые .log удаляются.
/// </summary>
internal static class LogFile
{
    private const long DefaultMaxBytes = 512 * 1024;
    private const string DayMarkerName = ".logs-day";
    private static readonly object Gate = new();
    private static DateTime _lastCleanupDate = DateTime.MinValue;

    public static void CleanupOnStartup()
    {
        try
        {
            lock (Gate) CleanupLocked(GetDirectory());
        }
        catch
        {
            // Журнал не должен мешать запуску программы.
        }
    }

    public static void Append(string fileName, string text, long maxBytes = DefaultMaxBytes, bool reset = false)
    {
        try
        {
            lock (Gate)
            {
                var dir = GetDirectory();
                Directory.CreateDirectory(dir);
                CleanupLocked(dir);

                var path = Path.Combine(dir, fileName);
                if (reset || (File.Exists(path) && new FileInfo(path).Length > maxBytes))
                    File.WriteAllText(path, text);
                else
                    File.AppendAllText(path, text);
            }
        }
        catch
        {
            // Журнал не должен мешать работе программы.
        }
    }

    private static string GetDirectory() =>
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "RigControl");

    private static void CleanupLocked(string dir)
    {
        var today = DateTime.Today;
        if (_lastCleanupDate == today) return;

        if (Directory.Exists(dir))
        {
            var marker = Path.Combine(dir, DayMarkerName);
            var isCurrentDay = false;
            try
            {
                isCurrentDay = File.Exists(marker) &&
                                File.ReadAllText(marker).Trim() == today.ToString("yyyy-MM-dd");
            }
            catch
            {
                // If the marker cannot be read, prefer a clean current-day log.
            }

            foreach (var path in Directory.EnumerateFiles(dir, "*.log"))
            {
                try
                {
                    // The marker also clears an old mixed pump.log on the first
                    // launch of this version, even when that file was appended
                    // earlier today by the previous logger.
                    if (!isCurrentDay || File.GetLastWriteTime(path).Date < today)
                        File.Delete(path);
                }
                catch
                {
                    // Один занятый файл не должен мешать остальным.
                }
            }

            try { File.WriteAllText(marker, today.ToString("yyyy-MM-dd")); }
            catch { /* log cleanup is best effort */ }
        }

        _lastCleanupDate = today;
    }
}