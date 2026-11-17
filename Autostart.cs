using System.Diagnostics;
using System.IO;
using System.Security;
using System.Security.AccessControl;
using System.Security.Principal;
using System.Text;

namespace RigControl;

/// <summary>
/// Автозапуск через планировщик заданий: задача «при входе в систему» с наивысшими правами —
/// так программа стартует от администратора без запроса UAC и без ограничения «остановить через 3 дня».
/// Раз задача запускает exe с правами администратора, сам exe должен лежать там, где его не подменит
/// обычная программа (например, в C:\Program Files), — это проверяет <see cref="IsProtectedLocation"/>.
/// </summary>
public static class Autostart
{
    private const string TaskName = "RigControl";

    /// <summary>Подсказка под переключателем, когда exe или его папку могут менять без прав администратора.</summary>
    public const string UnsafeLocationNote =
        "Внимание: RigControl.exe или его папку могут менять программы без прав администратора. " +
        "Для автозапуска безопаснее скопировать программу в C:\\Program Files\\RigControl и запускать оттуда.";

    public static bool IsEnabled() => Run("/Query /TN " + TaskName) == 0;

    /// <summary>
    /// Задача есть, но указывает на другой exe (программу перенесли в другую папку) — пересоздаёт её
    /// с текущим путём. Возвращает, включён ли автозапуск.
    /// На незащищённое место (см. <see cref="IsProtectedLocation"/>) задачу молча не переводим:
    /// <paramref name="needsConfirm"/> = true — пусть пользователь включит автозапуск заново и увидит предупреждение.
    /// </summary>
    public static bool IsEnabledRepair(out bool repaired, out bool needsConfirm)
    {
        repaired = false;
        needsConfirm = false;
        if (!IsEnabled()) return false;
        var xml = TaskXml();
        if (xml is null) return true;   // не прочитали — ничего не трогаем

        var exe = Environment.ProcessPath;
        if (string.IsNullOrEmpty(exe)) return true;

        try
        {
            var doc = System.Xml.Linq.XDocument.Parse(xml.Trim());
            var command = doc.Descendants().FirstOrDefault(e => e.Name.LocalName == "Command")?.Value?.Trim().Trim('"');
            if (command is not null && string.Equals(Path.GetFullPath(command), Path.GetFullPath(exe), StringComparison.OrdinalIgnoreCase))
                return true;
        }
        catch (Exception ex)
        {
            // Не разобрали XML — пересоздадим на всякий случай.
            QuietLog.Write(ex);
        }

        if (!IsProtectedLocation(exe))
        {
            needsConfirm = true;
            return true;
        }

        repaired = Set(true, out _);
        return true;
    }

    /// <summary>XML задачи через COM планировщика (schtasks выводит в кодировке консоли — кириллица в пути ломается).</summary>
    private static string? TaskXml()
    {
        try
        {
            var type = Type.GetTypeFromProgID("Schedule.Service");
            if (type is null) return null;
            dynamic service = Activator.CreateInstance(type)!;
            service.Connect();
            dynamic task = service.GetFolder("\\").GetTask(TaskName);
            return (string)task.Xml;
        }
        catch
        {
            return null;
        }
    }

    public static bool Set(bool enable, out string message)
    {
        message = "";

        try
        {
            if (!enable)
            {
                var code = Run("/Delete /TN " + TaskName + " /F");
                if (code == 0 || !IsEnabled()) return true;

                message = "Не удалось удалить задачу автозапуска (код " + code + ").";
                return false;
            }

            var exe = Environment.ProcessPath;
            if (string.IsNullOrEmpty(exe))
            {
                message = "Не удалось определить путь к программе.";
                return false;
            }

            var path = WriteTaskFile(BuildTaskXml(exe));

            try
            {
                var code = Run("/Create /TN " + TaskName + " /XML \"" + path + "\" /F");
                if (code == 0) return true;

                message = "Не удалось создать задачу автозапуска (код " + code + "). Запустите программу от администратора.";
                return false;
            }
            finally
            {
                try { File.Delete(path); } catch { /* временный файл — не критично */ }
            }
        }
        catch (Exception ex)
        {
            message = ex.Message;
            return false;
        }
    }

    /// <summary>
    /// XML задачи для schtasks. Задача получит наивысшие права, поэтому файл не должны подменить между записью
    /// и чтением: кладём его в C:\Windows\Temp (обычные программы чужие файлы там не читают, не меняют и не удаляют),
    /// имя случайное, а CreateNew не откроет файл, подложенный заранее.
    /// </summary>
    private static string WriteTaskFile(string xml)
    {
        var name = "RigControl-task-" + Guid.NewGuid().ToString("N") + ".xml";
        var windowsTemp = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.Windows), "Temp");

        foreach (var folder in new[] { windowsTemp, Path.GetTempPath() })
        {
            var path = Path.Combine(folder, name);
            try
            {
                using var writer = new StreamWriter(new FileStream(path, FileMode.CreateNew, FileAccess.Write, FileShare.None), Encoding.Unicode);
                writer.Write(xml);
                return path;
            }
            catch (Exception ex) when (folder == windowsTemp)
            {
                // Нет доступа к C:\Windows\Temp — запасной вариант: временная папка пользователя.
                QuietLog.Write(ex);
            }
        }

        throw new IOException("Не удалось записать файл задачи автозапуска.");
    }

    /// <summary>
    /// exe и его папку могут менять только администраторы и система (как в Program Files)?
    /// Если нет, любая программа без прав администратора подменит exe или подложит рядом библиотеку,
    /// а задача автозапуска запустит это с наивысшими правами.
    /// </summary>
    public static bool IsProtectedLocation(string? exe)
    {
        if (string.IsNullOrEmpty(exe)) return false;

        try
        {
            var file = new FileInfo(exe);
            var folder = file.Directory;
            if (!file.Exists || folder is null) return false;

            return !LetsOthersWrite(file.GetAccessControl()) && !LetsOthersWrite(folder.GetAccessControl());
        }
        catch (Exception ex)
        {
            // Не смогли проверить — считаем, что место не защищено.
            QuietLog.Write(ex);
            return false;
        }
    }

    // Права, с которыми можно подменить файл: запись, удаление и переименование, смена владельца и разрешений.
    private const FileSystemRights ReplaceRights =
        FileSystemRights.WriteData | FileSystemRights.AppendData | FileSystemRights.DeleteSubdirectoriesAndFiles |
        FileSystemRights.Delete | FileSystemRights.ChangePermissions | FileSystemRights.TakeOwnership;

    private const int GenericAll = 0x10000000;
    private const int GenericWrite = 0x40000000;

    private const string TrustedInstallerSid = "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464";

    private static bool LetsOthersWrite(FileSystemSecurity security)
    {
        foreach (FileSystemAccessRule rule in security.GetAccessRules(true, true, typeof(SecurityIdentifier)))
        {
            if (rule.AccessControlType != AccessControlType.Allow) continue;
            if ((rule.PropagationFlags & PropagationFlags.InheritOnly) != 0) continue;   // к самому объекту не относится
            if (((int)rule.FileSystemRights & ((int)ReplaceRights | GenericAll | GenericWrite)) == 0) continue;

            if (rule.IdentityReference is SecurityIdentifier sid
                && (sid.IsWellKnown(WellKnownSidType.LocalSystemSid)
                    || sid.IsWellKnown(WellKnownSidType.BuiltinAdministratorsSid)
                    || sid.Value == TrustedInstallerSid))
                continue;

            return true;
        }

        return false;
    }

    private static string BuildTaskXml(string exe)
    {
        var user = SecurityElement.Escape(WindowsIdentity.GetCurrent().Name);
        var command = SecurityElement.Escape(exe);
        var folder = SecurityElement.Escape(Path.GetDirectoryName(exe) ?? "");

        return
            "<?xml version=\"1.0\" encoding=\"UTF-16\"?>\n" +
            "<Task version=\"1.4\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">\n" +
            "  <RegistrationInfo><Description>Rig Control: экран помпы и клавиатуры</Description></RegistrationInfo>\n" +
            "  <Triggers>\n" +
            "    <LogonTrigger>\n" +
            "      <Enabled>true</Enabled>\n" +
            "      <UserId>" + user + "</UserId>\n" +
            "      <Delay>PT10S</Delay>\n" +
            "    </LogonTrigger>\n" +
            "  </Triggers>\n" +
            "  <Principals>\n" +
            "    <Principal id=\"Author\">\n" +
            "      <UserId>" + user + "</UserId>\n" +
            "      <LogonType>InteractiveToken</LogonType>\n" +
            "      <RunLevel>HighestAvailable</RunLevel>\n" +
            "    </Principal>\n" +
            "  </Principals>\n" +
            "  <Settings>\n" +
            "    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>\n" +
            "    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>\n" +
            "    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>\n" +
            "    <AllowHardTerminate>true</AllowHardTerminate>\n" +
            "    <StartWhenAvailable>false</StartWhenAvailable>\n" +
            "    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>\n" +
            "    <AllowStartOnDemand>true</AllowStartOnDemand>\n" +
            "    <Enabled>true</Enabled>\n" +
            "    <Hidden>false</Hidden>\n" +
            "    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>\n" +
            "    <Priority>7</Priority>\n" +
            "  </Settings>\n" +
            "  <Actions Context=\"Author\">\n" +
            "    <Exec>\n" +
            "      <Command>" + command + "</Command>\n" +
            "      <Arguments>--tray</Arguments>\n" +
            "      <WorkingDirectory>" + folder + "</WorkingDirectory>\n" +
            "    </Exec>\n" +
            "  </Actions>\n" +
            "</Task>\n";
    }

    private static int Run(string arguments)
    {
        var info = new ProcessStartInfo("schtasks.exe", arguments)
        {
            CreateNoWindow = true,
            UseShellExecute = false,
            RedirectStandardOutput = true,
            RedirectStandardError = true
        };

        using var process = Process.Start(info);
        if (process is null) return -1;

        // Оба потока читаем одновременно и в фоне: если ждать конца stdout, а schtasks заполнит буфер stderr,
        // зависнут оба процесса — и ожидание с тайм-аутом ниже не сработает никогда.
        var output = process.StandardOutput.ReadToEndAsync();
        var error = process.StandardError.ReadToEndAsync();

        if (!process.WaitForExit(15000))
        {
            try { process.Kill(); } catch { /* уже завершился */ }
            return -2;
        }

        try { Task.WaitAll(new Task[] { output, error }, 2000); }
        catch (AggregateException) { /* вывод schtasks нам не нужен */ }

        return process.ExitCode;
    }
}
