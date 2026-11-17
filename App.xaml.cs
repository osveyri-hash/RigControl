using System.Threading;
using System.Windows;

namespace RigControl;

public partial class App : Application
{
    private Mutex? _mutex;
    private EventWaitHandle? _showEvent;

    protected override void OnStartup(StartupEventArgs e)
    {
        // Одна копия программы: две сразу будут драться за USB-экран помпы.
        _mutex = new Mutex(false, "RigControl.SingleInstance");
        bool first;
        try { first = _mutex.WaitOne(0); }
        catch (AbandonedMutexException) { first = true; }
        _showEvent = new EventWaitHandle(false, EventResetMode.AutoReset, "RigControl.ShowWindow");

        if (!first)
        {
            // Уже запущена (возможно, в трее) — просим её показать окно и выходим.
            try { _showEvent.Set(); } catch { /* не критично */ }
            Shutdown();
            return;
        }

        LogFile.CleanupOnStartup();

        // Случайная ошибка в фоне не должна ронять программу: пишем в журнал и работаем дальше.
        DispatcherUnhandledException += (_, args) =>
        {
            LogError(args.Exception);
            args.Handled = true;
        };
        TaskScheduler.UnobservedTaskException += (_, args) =>
        {
            LogError(args.Exception);
            args.SetObserved();
        };

        // Ошибка в фоновом потоке завершает процесс: пишем её в журнал и до выхода отдаём вентиляторы плате.
        AppDomain.CurrentDomain.UnhandledException += (_, args) =>
        {
            if (args.ExceptionObject is Exception ex) LogError(ex);
            Telemetry.RestoreFansNow();
        };

        // Обычный выход отдаёт вентиляторы при закрытии окна; это запасной путь на любой другой выход из процесса.
        AppDomain.CurrentDomain.ProcessExit += (_, _) => Telemetry.RestoreFansNow();

        base.OnStartup(e);

        var waiter = new Thread(() =>
        {
            while (_showEvent.WaitOne())
            {
                Dispatcher.InvokeAsync(() =>
                {
                    if (Current.MainWindow is MainWindow window) window.ShowFromTray();
                });
            }
        })
        {
            IsBackground = true
        };
        waiter.Start();
    }

    private static void LogError(Exception ex)
    {
        LogFile.Append("error.log",
            $"{DateTime.Now:yyyy-MM-dd HH:mm:ss}  {ex}{Environment.NewLine}{Environment.NewLine}");
    }

    protected override void OnExit(ExitEventArgs e)
    {
        try { _mutex?.ReleaseMutex(); } catch { /* уже освобождён */ }
        base.OnExit(e);
    }
}
