using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Shapes;
using RigControl.Mouse;
using MouseAction = RigControl.Mouse.MouseAction;

namespace RigControl;

/// <summary>Мышь → «Кнопки»: схема мыши с подписями и список функций справа (как «Настройка клавиш» в M HUB).</summary>
public partial class MainWindow
{
    private sealed record MouseActionItem(Button Button, MouseAction? Action, string Search, TextBlock Group);

    private readonly List<MouseActionItem> _mouseActions = new();
    private readonly List<TextBlock> _mouseActionGroups = new();
    private int _mouseKeySel;
    private Border? _mouseCapture;
    private Button? _mouseCaptureItem;
    private TextBlock? _mouseCaptureHint;
    private TextBox? _mouseCaptureBox;
    private int _mouseCaptureMods;
    private bool _mouseCaptureKey;
    private TextBlock? _mouseSearchEmpty;

    private void InitMouseKeys()
    {
        MouseKeyList.Children.Clear();
        string? group = null;
        TextBlock? header = null;

        foreach (var action in MouseKeys.Actions)
        {
            if (action.Group != group)
            {
                if (action.Group == "Другое") AddKeyboardCaptureGroup();
                group = action.Group;
                header = AddMouseActionGroup(group);
            }
            AddMouseAction(action, header!);
        }

        _mouseSearchEmpty = new TextBlock
        {
            Text = "Ничего не найдено. Нужна клавиша клавиатуры — выберите «Клавиша или сочетание…».",
            TextWrapping = TextWrapping.Wrap, FontSize = 12, Margin = new Thickness(10, 8, 6, 0), Visibility = Visibility.Collapsed,
        };
        _mouseSearchEmpty.SetResourceReference(TextBlock.ForegroundProperty, "MutedBrush");
        MouseKeyList.Children.Add(_mouseSearchEmpty);

        MouseKeyPick0.IsChecked = true;
        SelectMouseKey(0);
    }

    private TextBlock AddMouseActionGroup(string title)
    {
        var header = new TextBlock
        {
            Text = title.ToUpperInvariant(), FontSize = 10.5, FontWeight = FontWeights.SemiBold,
            Margin = new Thickness(10, MouseKeyList.Children.Count == 0 ? 2 : 14, 0, 4),
        };
        header.SetResourceReference(TextBlock.ForegroundProperty, "MutedBrush");
        MouseKeyList.Children.Add(header);
        _mouseActionGroups.Add(header);
        return header;
    }

    private Button AddMouseAction(MouseAction? action, TextBlock group, string? name = null, string? hint = null)
    {
        var check = new TextBlock { Text = "\uE73E", FontFamily = new FontFamily("Segoe MDL2 Assets"), FontSize = 12,
            VerticalAlignment = VerticalAlignment.Center, Margin = new Thickness(8, 0, 0, 0), Visibility = Visibility.Collapsed };
        check.SetResourceReference(TextBlock.ForegroundProperty, "AccentBrush");
        var hintText = new TextBlock { Text = hint ?? action?.Hint ?? "", FontSize = 11.5, VerticalAlignment = VerticalAlignment.Center };
        hintText.SetResourceReference(TextBlock.ForegroundProperty, "MutedBrush");
        var title = new TextBlock { Text = name ?? action!.Name, TextTrimming = TextTrimming.CharacterEllipsis };

        var row = new DockPanel();
        DockPanel.SetDock(check, Dock.Right);
        DockPanel.SetDock(hintText, Dock.Right);
        row.Children.Add(check);
        row.Children.Add(hintText);
        row.Children.Add(title);

        var button = new Button { Style = (Style)FindResource("ActionItem"), Content = row, Tag = check };
        if (action is null) _mouseCaptureHint = hintText;
        if (action is not null) button.Click += async (_, _) => await AssignMouseKeyAsync(action.Key);
        MouseKeyList.Children.Add(button);
        _mouseActions.Add(new MouseActionItem(button, action,
            $"{group.Text} {name ?? action!.Name} {hint ?? action?.Hint}".ToLowerInvariant(), group));
        return button;
    }

    /// <summary>«Клавиша или сочетание…»: ловим нажатие на клавиатуре и назначаем его кнопке мыши.</summary>
    private void AddKeyboardCaptureGroup()
    {
        var header = AddMouseActionGroup("Клавиатура");
        _mouseCaptureItem = AddMouseAction(null, header, "Клавиша или сочетание…", "");
        _mouseCaptureItem.Click += (_, _) => BeginMouseKeyCapture();

        _mouseCaptureBox = new TextBox
        {
            IsReadOnly = true, IsReadOnlyCaretVisible = false, Text = "Нажмите клавишу…", TextAlignment = TextAlignment.Center,
            FontWeight = FontWeights.SemiBold, Cursor = Cursors.Arrow,
        };
        _mouseCaptureBox.PreviewKeyDown += OnMouseCaptureKeyDown;
        _mouseCaptureBox.PreviewKeyUp += OnMouseCaptureKeyUp;
        _mouseCaptureBox.LostKeyboardFocus += (_, _) => EndMouseKeyCapture();
        var hint = new TextBlock { Text = "Можно с Ctrl, Shift, Alt. Esc — отмена.", FontSize = 11.5, Margin = new Thickness(2, 6, 0, 0) };
        hint.SetResourceReference(TextBlock.ForegroundProperty, "MutedBrush");
        _mouseCapture = new Border
        {
            Margin = new Thickness(6, 4, 6, 2), Visibility = Visibility.Collapsed,
            Child = new StackPanel { Children = { _mouseCaptureBox, hint } },
        };
        MouseKeyList.Children.Add(_mouseCapture);
    }

    private void BeginMouseKeyCapture()
    {
        if (_mouseCapture is null || _mouseCaptureBox is null) return;
        _mouseCaptureMods = 0;
        _mouseCaptureKey = false;
        _mouseCaptureBox.Text = "Нажмите клавишу…";
        _mouseCapture.Visibility = Visibility.Visible;
        _mouseCaptureBox.Focus();
        Keyboard.Focus(_mouseCaptureBox);
    }

    private void EndMouseKeyCapture()
    {
        if (_mouseCapture is not null) _mouseCapture.Visibility = Visibility.Collapsed;
    }

    private async void OnMouseCaptureKeyDown(object sender, KeyEventArgs e)
    {
        e.Handled = true;
        var key = e.Key switch { Key.System => e.SystemKey, Key.ImeProcessed => e.ImeProcessedKey, _ => e.Key };
        var mods = MouseKeys.Modifiers(Keyboard.Modifiers);

        if (key == Key.Escape && mods == 0)
        {
            EndMouseKeyCapture();
            return;
        }

        if (MouseKeys.IsModifier(key))
        {
            _mouseCaptureMods = mods;
            _mouseCaptureBox!.Text = MouseKeys.Shortcut(mods << 16) + "+…";
            return;
        }

        var usage = MouseKeys.Usage(key);
        if (usage == 0)
        {
            _mouseCaptureBox!.Text = "Эту клавишу назначить нельзя";
            return;
        }

        _mouseCaptureKey = true;
        var code = MouseKeys.KeyboardKey(mods, usage);
        _mouseCaptureBox!.Text = MouseKeys.Shortcut(code.Value);
        await AssignMouseKeyAsync(code);
        EndMouseKeyCapture();
    }

    /// <summary>Отпустили модификатор, не нажав ничего больше, — назначаем сам модификатор (например, Shift на боковую).</summary>
    private async void OnMouseCaptureKeyUp(object sender, KeyEventArgs e)
    {
        var key = e.Key == Key.System ? e.SystemKey : e.Key;
        if (!MouseKeys.IsModifier(key) || _mouseCaptureKey || _mouseCaptureMods == 0) return;
        e.Handled = true;
        var code = MouseKeys.KeyboardKey(_mouseCaptureMods, 0);
        _mouseCaptureMods = 0;
        await AssignMouseKeyAsync(code);
        EndMouseKeyCapture();
    }

    private void OnMouseKeySearchKey(object sender, KeyEventArgs e)
    {
        if (e.Key != Key.Escape || MouseKeySearch.Text.Length == 0) return;
        MouseKeySearch.Clear();   // Esc — очистить поиск
        e.Handled = true;
    }

    private void OnMouseKeySearch(object sender, TextChangedEventArgs e)
    {
        var query = MouseKeySearch.Text.Trim().ToLowerInvariant();
        MouseKeySearchHint.Visibility = query.Length == 0 ? Visibility.Visible : Visibility.Collapsed;
        foreach (var item in _mouseActions)   // «Клавиша или сочетание…» видна всегда — любая клавиша клавиатуры
            item.Button.Visibility = query.Length == 0 || item.Action is null || item.Search.Contains(query)
                ? Visibility.Visible : Visibility.Collapsed;
        foreach (var group in _mouseActionGroups)
            group.Visibility = _mouseActions.Any(a => a.Group == group && a.Button.Visibility == Visibility.Visible)
                ? Visibility.Visible : Visibility.Collapsed;
        if (_mouseSearchEmpty is not null)
            _mouseSearchEmpty.Visibility = _mouseActions.Any(a => a.Action is not null && a.Button.Visibility == Visibility.Visible)
                ? Visibility.Collapsed : Visibility.Visible;
        if (query.Length > 0) EndMouseKeyCapture();
    }

    private void OnMouseZoneClick(object sender, MouseButtonEventArgs e)
    {
        if (sender is FrameworkElement { Name: var name } && name.Length > 0 && FindName($"MouseKeyPick{name[^1]}") is RadioButton pick)
            pick.IsChecked = true;
    }

    private void OnMouseKeyPick(object sender, RoutedEventArgs e)
    {
        if (sender is RadioButton { Tag: string tag }) SelectMouseKey(int.Parse(tag));
    }

    private void SelectMouseKey(int button)
    {
        _mouseKeySel = Math.Clamp(button, 0, MouseKeys.Count - 1);
        EndMouseKeyCapture();
        DrawMouseKeys(_mouseConfig);
    }

    private MouseKey MouseKeyNow(int button) =>
        _mouseConfig is { } cfg ? cfg.Keys[button] : MouseKey.Default;

    private void DrawMouseKeys(MouseConfig? cfg)
    {
        for (var i = 0; i < MouseKeys.Count; i++)
        {
            var selected = i == _mouseKeySel;
            var key = MouseKeyNow(i);
            if (FindName($"MouseKeyValue{i}") is TextBlock value)
            {
                value.Text = cfg is null ? "—" : MouseKeys.Describe(i, key);
                value.ToolTip = value.Text;
            }
            if (FindName($"MouseZone{i}") is Path zone) zone.Tag = selected ? "sel" : null;
            if (FindName($"MouseKeyLine{i}") is Path line)
            {
                line.SetResourceReference(Shape.StrokeProperty, selected ? "AccentBrush" : "MouseEdgeBrush");
                line.StrokeThickness = selected ? 2 : 1.4;
            }
            if (FindName($"MouseKeyDot{i}") is Ellipse dot)
                dot.SetResourceReference(Shape.FillProperty, selected ? "AccentBrush" : "MouseEdgeBrush");
        }

        var current = MouseKeyNow(_mouseKeySel);
        MouseKeyTitle.Text = MouseKeys.Titles[_mouseKeySel];
        MouseKeyCurrent.Text = cfg is null ? "" : $"Сейчас: {MouseKeys.Describe(_mouseKeySel, current)}" +
                                                  (current.Type == 0 ? " (родная)" : "");
        MouseKeyResetButton.IsEnabled = cfg is not null && current.Type != 0;

        var effective = current.Type == 0 ? MouseKeys.Defaults[_mouseKeySel] : current;
        foreach (var item in _mouseActions)
        {
            var on = cfg is not null && item.Action is { } a && a.Key == effective;
            if (item.Action is null && item.Button == _mouseCaptureItem)
            {
                on = cfg is not null && effective.Type == 2 && MouseKeys.Actions.All(x => x.Key != effective);
                _mouseCaptureHint!.Text = on ? MouseKeys.Shortcut(effective.Value) : "";
            }
            if (item.Button.Tag is TextBlock check) check.Visibility = on ? Visibility.Visible : Visibility.Collapsed;
            if (on) item.Button.SetResourceReference(BackgroundProperty, "AccentDimBrush");
            else item.Button.ClearValue(BackgroundProperty);
        }
    }

    /// <summary>Назначить выбранной кнопке функцию. Левый клик должен остаться хотя бы на одной кнопке.</summary>
    private async Task AssignMouseKeyAsync(MouseKey key)
    {
        if (_mouseConfig is not { } cfg || _mouse is null) return;
        var button = _mouseKeySel;
        key = MouseKeys.Normalize(button, key);

        var keys = cfg.Keys.Take(MouseKeys.Count).ToArray();
        keys[button] = key;
        if (!Enumerable.Range(0, MouseKeys.Count).Any(i => MouseKeys.IsLeftClick(i, keys[i])))
        {
            MouseNote("Без левого клика мышью не пользоваться — сначала назначьте «Левый клик» другой кнопке.", error: true);
            return;
        }

        if (FindName($"MouseKeyValue{button}") is TextBlock value) value.Text = MouseKeys.Describe(button, key);
        await MouseSendAsync(m => m.WriteKey(button, key), $"{MouseKeys.Short[button]} → {MouseKeys.Describe(button, key)}");
    }

    private async void OnMouseKeyReset(object sender, RoutedEventArgs e) => await AssignMouseKeyAsync(MouseKey.Default);

    private async void OnMouseKeysResetAll(object sender, RoutedEventArgs e)
    {
        if (_mouseConfig is null || sender is not Button button || !ConfirmTwice(button, "Точно сбросить?")) return;
        await MouseSendAsync(m => m.ResetKeys(), "Кнопки как с завода", settleMs: 500);
    }
}
