namespace RigControl;

public enum LcdKind
{
    Time,
    Date,
    CpuLoad,
    CpuTemp,
    GpuLoad,
    GpuTemp,
    Ram,
    Text,
    // Новые объекты (добавляются в конец — в настройках тип хранится числом)
    Weekday,
    CpuClock,
    CpuPower,
    GpuPower,
    Vram,
    NetDown,
    NetUp,
    Disk,
    Uptime,
    CpuBar,
    GpuBar,
    RamBar
}

/// <summary>Один объект на экране помпы: время, дата, показание датчика или свой текст.</summary>
public sealed class LcdItem
{
    public LcdKind Kind { get; set; }
    public double X { get; set; }
    public double Y { get; set; }
    public double Scale { get; set; } = 1;

    /// <summary>Цвет #RRGGBB. Пусто — «авто»: у температур цвет зависит от нагрева.</summary>
    public string Color { get; set; } = "#FFFFFF";

    public string Font { get; set; } = "Segoe UI";
    public bool Bold { get; set; } = true;

    /// <summary>Мелкая подпись перед значением («CPU», «GPU °C»…).</summary>
    public bool Label { get; set; } = true;

    /// <summary>Только для времени: показывать секунды.</summary>
    public bool Seconds { get; set; }

    /// <summary>Только для «своего текста».</summary>
    public string Text { get; set; } = "Текст";

    public double BaseSize() => Kind switch
    {
        LcdKind.Time => 46,
        LcdKind.Date or LcdKind.Weekday => 18,
        LcdKind.Text => 24,
        LcdKind.NetDown or LcdKind.NetUp or LcdKind.Disk or LcdKind.Uptime => 20,
        _ => 26
    };

    /// <summary>Полоска-индикатор вместо текста.</summary>
    public bool IsBar() => Kind is LcdKind.CpuBar or LcdKind.GpuBar or LcdKind.RamBar;

    public bool HasLabel() => LabelText().Length > 0;

    public string LabelText() => Kind switch
    {
        LcdKind.CpuLoad or LcdKind.CpuBar => "CPU",
        LcdKind.CpuTemp => "CPU °C",
        LcdKind.GpuLoad or LcdKind.GpuBar => "GPU",
        LcdKind.GpuTemp => "GPU °C",
        LcdKind.Ram or LcdKind.RamBar => "RAM",
        LcdKind.CpuClock => "CPU",
        LcdKind.CpuPower => "CPU",
        LcdKind.GpuPower => "GPU",
        LcdKind.Vram => "VRAM",
        LcdKind.NetDown => "↓",
        LcdKind.NetUp => "↑",
        LcdKind.Disk => "C:",
        LcdKind.Uptime => "ПК",
        _ => ""
    };

    public static string KindName(LcdKind kind) => kind switch
    {
        LcdKind.Time => "Время",
        LcdKind.Date => "Дата",
        LcdKind.CpuLoad => "Загрузка CPU",
        LcdKind.CpuTemp => "Температура CPU",
        LcdKind.GpuLoad => "Загрузка GPU",
        LcdKind.GpuTemp => "Температура GPU",
        LcdKind.Ram => "Память (RAM)",
        LcdKind.Weekday => "День недели",
        LcdKind.CpuClock => "Частота CPU",
        LcdKind.CpuPower => "Мощность CPU",
        LcdKind.GpuPower => "Мощность GPU",
        LcdKind.Vram => "Видеопамять",
        LcdKind.NetDown => "Сеть: загрузка",
        LcdKind.NetUp => "Сеть: отдача",
        LcdKind.Disk => "Диск C:",
        LcdKind.Uptime => "Время работы ПК",
        LcdKind.CpuBar => "Полоска CPU",
        LcdKind.GpuBar => "Полоска GPU",
        LcdKind.RamBar => "Полоска RAM",
        _ => "Свой текст"
    };

    /// <summary>Новый объект с цветом и видом по умолчанию для своего типа.</summary>
    public static LcdItem Create(LcdKind kind) => new()
    {
        Kind = kind,
        Color = kind switch
        {
            LcdKind.Date => "#8A90A0",
            LcdKind.CpuLoad => "#4DA3FF",
            LcdKind.CpuTemp => "",
            LcdKind.GpuLoad => "#7BD36B",
            LcdKind.GpuTemp => "",
            LcdKind.Ram or LcdKind.RamBar or LcdKind.Vram => "#C084FF",
            LcdKind.CpuBar or LcdKind.CpuClock or LcdKind.CpuPower => "#4DA3FF",
            LcdKind.GpuBar or LcdKind.GpuPower => "#7BD36B",
            LcdKind.NetDown => "#4DE0E0",
            LcdKind.NetUp => "#FF6FB5",
            LcdKind.Weekday or LcdKind.Uptime or LcdKind.Disk => "#8A90A0",
            _ => "#FFFFFF"
        },
        Bold = kind is not (LcdKind.Date or LcdKind.Weekday)
    };

    /// <summary>Расстановка по умолчанию для экрана 320×240.</summary>
    public static List<LcdItem> DefaultLayout()
    {
        var items = new List<LcdItem>
        {
            Create(LcdKind.Time),
            Create(LcdKind.Date),
            Create(LcdKind.CpuLoad),
            Create(LcdKind.CpuTemp),
            Create(LcdKind.GpuLoad),
            Create(LcdKind.GpuTemp),
            Create(LcdKind.Ram)
        };

        (double X, double Y)[] spots =
        {
            (16, 6), (188, 26), (16, 82), (170, 82), (16, 130), (170, 130), (16, 178)
        };

        for (var i = 0; i < items.Count; i++)
        {
            items[i].X = spots[i].X;
            items[i].Y = spots[i].Y;
        }

        return items;
    }
}
