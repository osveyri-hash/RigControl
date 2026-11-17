using System.Windows.Input;

namespace RigControl.Mouse;

/// <summary>Функция для кнопки мыши (как список справа во вкладке «Настройка клавиш» M HUB).</summary>
public sealed record MouseAction(string Group, string Name, MouseKey Key, string? Hint = null);

/// <summary>Кнопки мыши, каталог функций и коды клавиш (HID usage) для назначений.</summary>
public static class MouseKeys
{
    public const int Left = 0, Middle = 1, Right = 2, Forward = 3, Back = 4, Count = 5;

    public static readonly string[] Short = { "ЛКМ", "СКМ", "ПКМ", "Вперёд", "Назад" };

    public static readonly string[] Titles =
        { "Левая кнопка", "Колесо (нажатие)", "Правая кнопка", "Боковая передняя", "Боковая задняя" };

    private static readonly MouseKey LeftClick = new(1, 0x010000);

    /// <summary>Родные функции кнопок (тип 0 в памяти мыши означает именно их).</summary>
    public static readonly MouseKey[] Defaults =
        { LeftClick, new(1, 0x040000), new(1, 0x020000), new(1, 0x100000), new(1, 0x080000) };

    public static readonly MouseAction[] Actions =
    {
        new("Мышь", "Левый клик", LeftClick),
        new("Мышь", "Правый клик", new(1, 0x020000)),
        new("Мышь", "Средний клик", new(1, 0x040000)),
        new("Мышь", "Вперёд", new(1, 0x100000)),
        new("Мышь", "Назад", new(1, 0x080000)),
        new("Мышь", "Прокрутка вверх", new(1, 0x000200)),
        new("Мышь", "Прокрутка вниз", new(1, 0x00FE00)),

        new("DPI", "DPI по кругу", new(5, 0x010000)),
        new("DPI", "DPI +", new(5, 0x020000)),
        new("DPI", "DPI −", new(5, 0x030000)),

        new("Встроенные профили", "Профиль 1", new(10, 0x010000)),
        new("Встроенные профили", "Профиль 2", new(10, 0x020000)),
        new("Встроенные профили", "Профиль 3", new(10, 0x030000)),
        new("Встроенные профили", "Следующий профиль", new(10, 0x040000)),

        new("Мультимедиа", "Пауза / воспроизведение", new(3, 0xCD0000)),
        new("Мультимедиа", "Следующий трек", new(3, 0xB50000)),
        new("Мультимедиа", "Предыдущий трек", new(3, 0xB60000)),
        new("Мультимедиа", "Стоп", new(3, 0xB70000)),
        new("Мультимедиа", "Громче", new(3, 0xE90000)),
        new("Мультимедиа", "Тише", new(3, 0xEA0000)),
        new("Мультимедиа", "Без звука", new(3, 0xE20000)),

        new("Правка", "Копировать", new(8, 0x070106), "Ctrl+C"),
        new("Правка", "Вставить", new(8, 0x070119), "Ctrl+V"),
        new("Правка", "Вырезать", new(8, 0x07011B), "Ctrl+X"),
        new("Правка", "Отменить", new(2, 0x011D00), "Ctrl+Z"),
        new("Правка", "Повторить", new(2, 0x011C00), "Ctrl+Y"),
        new("Правка", "Выделить всё", new(2, 0x010400), "Ctrl+A"),

        new("Windows", "Рабочий стол", new(2, 0x080700), "Win+D"),
        new("Windows", "Проводник", new(2, 0x080800), "Win+E"),
        new("Windows", "Представление задач", new(2, 0x082B00), "Win+Tab"),
        new("Windows", "Заблокировать ПК", new(2, 0x080F00), "Win+L"),
        new("Windows", "Яркость +", new(8, 0x0C6F00)),
        new("Windows", "Яркость −", new(8, 0x0C7000)),

        new("Другое", "Отключить кнопку", new(9, 0xFFFFFF)),
    };

    /// <summary>Что сейчас делает кнопка.</summary>
    public static string Describe(int button, MouseKey key)
    {
        if (key.Type == 0) key = Defaults[Math.Clamp(button, 0, Count - 1)];
        foreach (var action in Actions)
            if (action.Key == key) return action.Name;
        return key.Type switch
        {
            2 => Shortcut(key.Value),
            4 => "Макрос",
            9 => "Отключена",
            _ => $"Своя функция ({key.Type}:{key.Value:X6})",
        };
    }

    /// <summary>Назначение кнопки равно родному — в мышь пишем тип 0, как M HUB.</summary>
    public static MouseKey Normalize(int button, MouseKey key) =>
        button is >= 0 and < Count && key == Defaults[button] ? MouseKey.Default : key;

    public static bool IsLeftClick(int button, MouseKey key) => key.IsLeftClick || (button == Left && key.Type == 0);

    // ── Клавиши: тип 2, значение [модификаторы, HID usage, 0] ──

    public static MouseKey KeyboardKey(int modifiers, int usage) => new(2, (modifiers & 0xFF) << 16 | (usage & 0xFF) << 8);

    public static string Shortcut(int value)
    {
        var mods = value >> 16 & 0xFF;
        var usage = value >> 8 & 0xFF;
        var parts = new List<string>();
        if ((mods & 0x11) != 0) parts.Add("Ctrl");
        if ((mods & 0x22) != 0) parts.Add("Shift");
        if ((mods & 0x44) != 0) parts.Add("Alt");
        if ((mods & 0x88) != 0) parts.Add("Win");
        if (usage != 0) parts.Add(UsageName(usage));
        return parts.Count == 0 ? "Клавиша" : string.Join("+", parts);
    }

    public static int Modifiers(ModifierKeys keys) =>
        (keys.HasFlag(ModifierKeys.Control) ? 1 : 0) | (keys.HasFlag(ModifierKeys.Shift) ? 2 : 0)
        | (keys.HasFlag(ModifierKeys.Alt) ? 4 : 0) | (keys.HasFlag(ModifierKeys.Windows) ? 8 : 0);

    public static bool IsModifier(Key key) => key is Key.LeftCtrl or Key.RightCtrl or Key.LeftShift or Key.RightShift
        or Key.LeftAlt or Key.RightAlt or Key.LWin or Key.RWin;

    /// <summary>Клавиша WPF → код HID (страница 0x07). 0 — такой клавиши в списке нет.</summary>
    public static int Usage(Key key)
    {
        if (key is >= Key.A and <= Key.Z) return 0x04 + (key - Key.A);
        if (key is >= Key.D1 and <= Key.D9) return 0x1E + (key - Key.D1);
        if (key is >= Key.F1 and <= Key.F12) return 0x3A + (key - Key.F1);
        if (key is >= Key.F13 and <= Key.F24) return 0x68 + (key - Key.F13);
        if (key is >= Key.NumPad1 and <= Key.NumPad9) return 0x59 + (key - Key.NumPad1);
        return key switch
        {
            Key.D0 => 0x27, Key.Enter => 0x28, Key.Escape => 0x29, Key.Back => 0x2A, Key.Tab => 0x2B, Key.Space => 0x2C,
            Key.OemMinus => 0x2D, Key.OemPlus => 0x2E, Key.OemOpenBrackets => 0x2F, Key.OemCloseBrackets => 0x30,
            Key.OemPipe => 0x31, Key.OemSemicolon => 0x33, Key.OemQuotes => 0x34, Key.OemTilde => 0x35,
            Key.OemComma => 0x36, Key.OemPeriod => 0x37, Key.OemQuestion => 0x38, Key.CapsLock => 0x39,
            Key.PrintScreen => 0x46, Key.Scroll => 0x47, Key.Pause => 0x48, Key.Insert => 0x49, Key.Home => 0x4A,
            Key.PageUp => 0x4B, Key.Delete => 0x4C, Key.End => 0x4D, Key.PageDown => 0x4E,
            Key.Right => 0x4F, Key.Left => 0x50, Key.Down => 0x51, Key.Up => 0x52, Key.NumLock => 0x53,
            Key.Divide => 0x54, Key.Multiply => 0x55, Key.Subtract => 0x56, Key.Add => 0x57,
            Key.NumPad0 => 0x62, Key.Decimal => 0x63, Key.OemBackslash => 0x64, Key.Apps => 0x65,
            _ => 0,
        };
    }

    public static string UsageName(int usage)
    {
        if (usage is >= 0x04 and <= 0x1D) return ((char)('A' + usage - 0x04)).ToString();
        if (usage is >= 0x1E and <= 0x26) return ((char)('1' + usage - 0x1E)).ToString();
        if (usage is >= 0x3A and <= 0x45) return $"F{usage - 0x39}";
        if (usage is >= 0x68 and <= 0x73) return $"F{usage - 0x68 + 13}";
        if (usage is >= 0x59 and <= 0x61) return $"Num {usage - 0x58}";
        return usage switch
        {
            0x27 => "0", 0x28 => "Enter", 0x29 => "Esc", 0x2A => "Backspace", 0x2B => "Tab", 0x2C => "Пробел",
            0x2D => "-", 0x2E => "=", 0x2F => "[", 0x30 => "]", 0x31 => "\\", 0x33 => ";", 0x34 => "'", 0x35 => "`",
            0x36 => ",", 0x37 => ".", 0x38 => "/", 0x39 => "Caps Lock", 0x46 => "Print Screen", 0x47 => "Scroll Lock",
            0x48 => "Pause", 0x49 => "Insert", 0x4A => "Home", 0x4B => "Page Up", 0x4C => "Delete", 0x4D => "End",
            0x4E => "Page Down", 0x4F => "→", 0x50 => "←", 0x51 => "↓", 0x52 => "↑", 0x53 => "Num Lock",
            0x54 => "Num /", 0x55 => "Num *", 0x56 => "Num −", 0x57 => "Num +", 0x58 => "Num Enter", 0x62 => "Num 0",
            0x63 => "Num .", 0x64 => "\\", 0x65 => "Меню",
            _ => $"0x{usage:X2}",
        };
    }
}
