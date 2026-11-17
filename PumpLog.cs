namespace RigControl;

/// <summary>
/// Журнал экрана помпы: запуск и остановка показа, паузы в кадрах, повторные рукопожатия,
/// переподключения и сбросы USB. После «поймал логотип» по нему видно, что происходило.
/// Пишется в %AppData%\RigControl\pump.log. Хранится только текущий календарный день.
/// Сам журнал исключений не бросает.
/// </summary>
internal static class PumpLog
{
    public static void Write(string message)
    {
        LogFile.Append("pump.log", $"{DateTime.Now:yyyy-MM-dd HH:mm:ss}  {message}{Environment.NewLine}");
    }
}