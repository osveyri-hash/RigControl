using System.Runtime.InteropServices;
using System.Threading;

namespace RigControl;

/// <summary>
/// Подключение к клавиатуре: два вендорских HID-интерфейса и все команды.
/// Протокол восстановлен из официального конфигуратора AJAZZ (см. ak820-linux).
/// </summary>
public sealed class Ak820Device : IDisposable
{
    private readonly HidInterface _control;
    private readonly HidInterface _data;

    /// <summary>
    /// Одна команда за раз. Каждая команда — сессия из нескольких пакетов (04 18 … 04 02);
    /// если два потока (анимация, «Применить», часы, заливка) пишут одновременно, пакеты
    /// перемешиваются и прошивка молча отбрасывает обе команды — отсюда «работает через раз».
    /// </summary>
    private readonly object _sync = new();
    private bool _disposed;

    private void Locked(Action action)
    {
        lock (_sync)
        {
            ObjectDisposedException.ThrowIf(_disposed, this);
            action();
        }
    }

    public string Description { get; }

    private Ak820Device(HidInterface control, HidInterface data)
    {
        _control = control;
        _data = data;
        Description = $"AK820 Pro  {control.VendorId:X4}:{control.ProductId:X4}";
    }

    /// <summary>
    /// Ищет клавиатуру: по кабелю или через донгл 2,4 ГГц. Подходит любой HID-интерфейс Sonix (VID 0C45) с вендорскими
    /// страницами 0xFF13 и 0xFF68 — у донгла PID другой. Отдаёт ли донгл эти страницы, зависит от прошивки.
    /// </summary>
    public static Ak820Device? TryOpen(out string? error)
    {
        error = null;

        try
        {
            var interfaces = HidInterface.Enumerate(Ak820Protocol.VendorId);

            var control = interfaces.FirstOrDefault(i => i.UsagePage == Ak820Protocol.ControlUsagePage);
            var data = interfaces.FirstOrDefault(i => i.UsagePage == Ak820Protocol.DataUsagePage);

            if (control is null || data is null)
            {
                foreach (var item in interfaces) item.Dispose();

                var seen = HidInterface.DescribeVendorInterfaces();
                var list = seen.Count == 0 ? "вендорских HID-интерфейсов в системе нет" : string.Join("; ", seen);

                var dongle = interfaces.Count > 0 && interfaces.All(i => !Ak820Protocol.ProductIds.Contains(i.ProductId));

                error = interfaces.Count == 0
                    ? "Клавиатура не найдена: подключите кабель USB-C или включите режим 2,4 ГГц с донглом."
                    : dongle
                        ? "Найден донгл 2,4 ГГц, но он не отдаёт интерфейсы команд (0xFF13/0xFF68), по радио подсветкой и экраном управлять нельзя — подключите кабель USB-C. Настройки хранятся в клавиатуре и останутся после возврата на 2,4 ГГц."
                        : "Найдена клавиатура, но интерфейсы команд (0xFF13/0xFF68) не видны — проверьте проводный режим.";

                error += " Вендорские интерфейсы: " + list + ".";
                return null;
            }

            foreach (var item in interfaces)
            {
                if (!ReferenceEquals(item, control) && !ReferenceEquals(item, data))
                    item.Dispose();
            }

            return new Ak820Device(control, data);
        }
        catch (Exception ex)
        {
            error = ex.Message;
            return null;
        }
    }

    /// <summary>
    /// Свои цвета по клавишам (кабель). Команда 0x23 с count=9, затем 9 пакетов
    /// по 64 байта: [код клавиши, R, G, B] на каждую клавишу, в конце 0xAA 0x55.
    /// </summary>
    public void SetKeyColors(byte[] frame, bool commit = false, bool start = false)
    {
        Locked(() =>
        {
            SavedLight = null;
            if (start) Command(0x18);
            Command(0x23, 9);

            for (var i = 0; i < 9; i++)
            {
                _control.SetFeature(frame.AsSpan(i * 64, 64));
                TryReadFeature();
            }

            if (commit) Command(0x02);
        });
    }

    /// <summary>
    /// Живой кадр цветов по клавишам — как страница «Музыка» официальной программы (DeviceDriver.exe, 0x427E10):
    /// заголовок 04 20 (8 пакетов следом), 512 байт записей [код, R, G, B] подряд, затем 04 02.
    /// Без 04 18 / 04 F0 и без записи во флеш — кадры можно слать 20 раз в секунду. Когда кадры перестают
    /// приходить, клавиатура сама возвращается к сохранённому эффекту.
    /// Паузы как у официальной программы: 5 мс вокруг заголовков, 2 мс между пакетами данных.
    /// Прежний путь (таблица 0x23 в режиме 20) клавиатура как живые кадры не принимает: светился только F-ряд.
    /// </summary>
    /// <returns>false — клавиатура дважды ответила «занята», кадр пропущен.</returns>
    public bool SendLiveFrame(byte[] live, CancellationToken token = default)
    {
        if (live.Length < Ak820Protocol.LiveFrameSize)
            throw new ArgumentException("Живой кадр короче 512 байт", nameof(live));

        lock (_sync)
        {
            ObjectDisposedException.ThrowIf(_disposed, this);
            token.ThrowIfCancellationRequested();

            if (LiveCommand(Ak820Protocol.Command(0x20, Ak820Protocol.LiveFrameSize / 64)) == 0xFF) return false;

            for (var i = 0; i < Ak820Protocol.LiveFrameSize / 64; i++)
            {
                _control.SetFeature(live.AsSpan(i * 64, 64));
                Thread.Sleep(2);
            }

            LiveCommand(Ak820Protocol.Command(0x02));
            LightMirror.Live(live);
            return true;
        }
    }

    /// <summary>Статус последнего заголовка живого кадра: байт 3 ответа (1 — принято), −1 — ответа не было.</summary>
    public int LastLiveStatus { get; private set; } = -1;

    /// <summary>Заголовок живого кадра: 5 мс, запись, 5 мс, ответ. 0xFF — «занята», повторяем один раз.</summary>
    private int LiveCommand(byte[] packet)
    {
        var status = -1;

        for (var attempt = 0; attempt < 2; attempt++)
        {
            Thread.Sleep(5);
            _control.SetFeature(packet);
            Thread.Sleep(5);

            try { status = _control.GetFeature()[3]; }
            catch { status = -1; }

            if (status != 0xFF) break;
        }

        LastLiveStatus = status;
        return status;
    }

    /// <summary>
    /// Проводная клавиатура сейчас на шине? Смотрим список устройств USB (cfgmgr32) и НЕ открываем
    /// HID-интерфейсы: повторное открытие ломало текущий канал. Возвращает null, если проверить не удалось —
    /// тогда текущее подключение лучше не трогать.
    /// Важно: фильтр по перечислителю «USB» работает только с флагом CM_GETIDLIST_FILTER_ENUMERATOR;
    /// с одним флагом PRESENT фильтр игнорируется, и функция отвечала «да» на любое устройство в системе.
    /// </summary>
    public static bool? IsWiredPresent()
    {
        try
        {
            const uint flags = CM_GETIDLIST_FILTER_ENUMERATOR | CM_GETIDLIST_FILTER_PRESENT;

            if (CM_Get_Device_ID_List_SizeW(out var size, "USB", flags) != 0) return null;
            if (size <= 1) return false;

            var buffer = new char[size];
            if (CM_Get_Device_ID_ListW("USB", buffer, size, flags) != 0) return null;

            // Список — набор строк, разделённых нулями: USB\VID_0C45&PID_8009\серийник
            foreach (var id in new string(buffer).Split('\0', StringSplitOptions.RemoveEmptyEntries))
            {
                foreach (var pid in Ak820Protocol.ProductIds)
                {
                    var prefix = $@"USB\VID_{Ak820Protocol.VendorId:X4}&PID_{pid:X4}";
                    if (id.StartsWith(prefix, StringComparison.OrdinalIgnoreCase)) return true;
                }
            }

            return false;
        }
        catch
        {
            return null;
        }
    }

    private const uint CM_GETIDLIST_FILTER_ENUMERATOR = 0x00000001;
    private const uint CM_GETIDLIST_FILTER_PRESENT = 0x00000100;

    [DllImport("cfgmgr32.dll", CharSet = CharSet.Unicode)]
    private static extern int CM_Get_Device_ID_List_SizeW(out uint length, string filter, uint flags);

    [DllImport("cfgmgr32.dll", CharSet = CharSet.Unicode)]
    private static extern int CM_Get_Device_ID_ListW(string filter, char[] buffer, uint length, uint flags);

    // ── Команды ──────────────────────────────────────────────────────────────

    /// <summary>Сессия: 04 18 → команда → 04 02. Каждая команда с паузой 35 мс.</summary>
    private void Command(byte op, int count = 0, byte extra0 = 0)
    {
        Thread.Sleep(Ak820Protocol.CommandDelayMs);
        _control.SetFeature(Ak820Protocol.Command(op, count, extra0));
        Thread.Sleep(Ak820Protocol.CommandDelayMs);
        TryReadFeature();
    }

    private void TryReadFeature()
    {
        try { _control.GetFeature(); }
        catch { /* ответ необязателен */ }
    }

    /// <summary>Встроенный эффект подсветки. rainbow=true — цвет не используется.</summary>
    public void SetEffect(int mode, string color, bool rainbow, int brightness, int speed, int direction)
    {
        Locked(() =>
        {
            var (r, g, b) = Ak820Protocol.ParseColor(color, (255, 255, 255));

            SavedLight = null;
            Command(0x18);
            Command(0x13, 1);
            Thread.Sleep(Ak820Protocol.CommandDelayMs);
            _control.SetFeature(Ak820Protocol.LightPacket(mode, r, g, b, rainbow, brightness, speed, direction));
            Command(0x02);
            Finish();
            SavedLight = EffectKey(mode, color, rainbow, brightness, speed, direction);

            // Режим своей раскраски сам по себе ничего не рисует — его цвета запишет SetUserLight.
            if (mode != KeyPaint.UserLightMode)
                LightMirror.Effect(mode, r, g, b, rainbow, brightness, speed, direction, restart: true);
        });
    }

    /// <summary>
    /// Быстрая смена цвета/эффекта без полной сессии и без записи во флеш.
    /// Нужна для синхронизации подсветки с кадрами гифки: каждую секунду
    /// приходит несколько новых цветов, а сохранять их в память нельзя.
    /// </summary>
    public void SetEffectFast(int mode, byte r, byte g, byte b, bool rainbow,
        int brightness = 5, int speed = 3, int direction = 0)
    {
        Locked(() =>
        {
            SavedLight = null;
            _control.SetFeature(Ak820Protocol.LightPacket(mode, r, g, b, rainbow, brightness, speed, direction));
            TryReadFeature();
            LightMirror.Effect(mode, r, g, b, rainbow, brightness, speed, direction);
        });
    }

    /// <summary>
    /// Пользовательская подсветка (своя раскраска): режим 0x80, затем таблица цветов командой 0x23.
    /// Режим 0x80 и формат таблицы — как у официальной программы. Прошивка хранит раскраску сама,
    /// повторять кадры не нужно. Прежний режим 20 красил всю клавиатуру в цвет закрашенной клавиши.
    /// </summary>
    public void SetUserLight(byte[] frame)
    {
        // Весь обмен под одной блокировкой (она реентерабельна), чтобы между режимом и таблицей
        // не вклинилась другая команда.
        lock (_sync)
        {
            ObjectDisposedException.ThrowIf(_disposed, this);
            SetEffect(KeyPaint.UserLightMode, "#000000", false, 5, 0, 0);
            SetKeyColors(frame, commit: true, start: true);
            Finish();
            SavedLight = UserLightKey(frame);
            LightMirror.UserLight(frame);
        }
    }

    /// <summary>
    /// Что сейчас сохранено в клавиатуре (эффект или раскраска) — чтобы перед потоком живых кадров
    /// не переписывать флеш тем же самым. null — неизвестно (переподключились, быстрые команды).
    /// </summary>
    public string? SavedLight { get; private set; }

    private static string EffectKey(int mode, string color, bool rainbow, int brightness, int speed, int direction) =>
        $"fx|{mode}|{color.Trim().ToUpperInvariant()}|{rainbow}|{brightness}|{speed}|{direction}";

    private static string UserLightKey(byte[] frame)
    {
        var hash = new HashCode();
        hash.AddBytes(frame);
        return $"user|{frame.Length}|{hash.ToHashCode()}";
    }

    /// <summary>Сохраняет эффект, только если в клавиатуре сейчас другой.</summary>
    public void EnsureEffect(int mode, string color, bool rainbow, int brightness, int speed, int direction)
    {
        if (SavedLight == EffectKey(mode, color, rainbow, brightness, speed, direction))
        {
            // Уже сохранён — но схема на вкладке «Раскраска» должна знать, что горит.
            var (r, g, b) = Ak820Protocol.ParseColor(color, (255, 255, 255));
            LightMirror.Effect(mode, r, g, b, rainbow, brightness, speed, direction);
            return;
        }

        SetEffect(mode, color, rainbow, brightness, speed, direction);
    }

    /// <summary>Сохраняет раскраску, только если в клавиатуре сейчас другая.</summary>
    public void EnsureUserLight(byte[] frame)
    {
        if (SavedLight == UserLightKey(frame))
        {
            LightMirror.UserLight(frame);
            return;
        }

        SetUserLight(frame);
    }

    /// <summary>Синхронизация часов на экране с этим ПК.</summary>
    public void SyncTime(DateTime now)
    {
        Locked(() =>
        {
            Command(0x18);
            Command(0x28, 1);
            Thread.Sleep(Ak820Protocol.CommandDelayMs);
            _control.SetFeature(Ak820Protocol.TimePacket(now));
            TryReadFeature();
            Command(0x02);
        });
    }

    /// <summary>
    /// Заливка картинки/гифки на экран. После каждой 4-килобайтной порции
    /// читаем ACK — иначе прошивка не дописывает данные во флеш.
    /// </summary>
    public UploadStats UploadScreen(byte[] blob, IProgress<(int Done, int Total)>? progress = null,
        CancellationToken token = default)
    {
        lock (_sync)
        {
            ObjectDisposedException.ThrowIf(_disposed, this);
            return UploadScreenCore(blob, progress, token);
        }
    }

    private UploadStats UploadScreenCore(byte[] blob, IProgress<(int Done, int Total)>? progress,
        CancellationToken token)
    {
        var clock = System.Diagnostics.Stopwatch.StartNew();
        long writeTicks = 0, ackTicks = 0;
        var acks = 0;

        var total = blob.Length / Ak820Protocol.Chunk;
        _ackTimeoutMs = 300;

        Command(0x18);
        Command(0x72, total, extra0: 0x01);

        try
        {
            for (var i = 0; i < total; i++)
            {
                token.ThrowIfCancellationRequested();

                var t0 = clock.ElapsedTicks;
                _data.WriteOutput(blob.AsSpan(i * Ak820Protocol.Chunk, Ak820Protocol.Chunk));
                var t1 = clock.ElapsedTicks;
                if (TryReadAck()) acks++;
                var t2 = clock.ElapsedTicks;

                writeTicks += t1 - t0;
                ackTicks += t2 - t1;

                progress?.Report((i + 1, total));
            }
        }
        finally
        {
            // При отмене или ошибке всё равно закрываем сессию, чтобы
            // клавиатура не осталась в режиме приёма данных.
            try { Command(0x02); } catch { /* уже не критично */ }
        }

        double Seconds(long ticks) => (double)ticks / System.Diagnostics.Stopwatch.Frequency;
        return new UploadStats(total, acks, Seconds(writeTicks), Seconds(ackTicks), clock.Elapsed.TotalSeconds);
    }

    private int _ackTimeoutMs = 300;

    private bool TryReadAck()
    {
        // Windows требует буфер ровно InputReportByteLength — с 64 байтами
        // ReadFile падала бы с ошибкой, и ACK тихо терялся.
        var length = _data.InputReportLength;
        if (length <= 0) return false;

        try
        {
            var got = _data.ReadInput(length, _ackTimeoutMs);

            // Пришёл ACK — ждём его и дальше как раньше; нет — не тормозим
            // заливку по 300 мс на каждую порцию.
            _ackTimeoutMs = got > 0 ? 300 : 20;
            return got > 0;
        }
        catch { return false; /* ACK бывает пустым — не повод рвать заливку */ }
    }

    /// <summary>Финальный 04 F0: официальный конфигуратор шлёт его после смены подсветки.</summary>
    private void Finish()
    {
        try
        {
            Thread.Sleep(Ak820Protocol.CommandDelayMs);
            _control.SetFeature(Ak820Protocol.Command(0xF0));
        }
        catch { /* не критично */ }
    }

    public void Dispose()
    {
        // Ждём, пока текущая команда допишется: закрыть хэндл посреди сессии — верный способ
        // оставить клавиатуру в режиме приёма до следующего переподключения.
        lock (_sync)
        {
            if (_disposed) return;
            _disposed = true;
            _control.Dispose();
            _data.Dispose();
        }
    }
}

/// <summary>Итоги заливки: сколько порций, сколько ACK и куда ушло время.</summary>
public readonly record struct UploadStats(int Chunks, int Acks, double WriteSeconds, double AckSeconds, double TotalSeconds);
