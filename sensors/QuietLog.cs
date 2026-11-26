namespace RigControl.Sensors;

/// <summary>Ошибки чтения в помощнике не показываем: страница сама напишет «не удалось прочитать».</summary>
internal static class QuietLog
{
    public static void Write(Exception _) { }
}
