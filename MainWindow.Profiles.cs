using System.IO;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;

namespace RigControl;

/// <summary>
/// Профили одной кнопкой: «Игра», «Работа», «Тихо» и свои. Профиль запоминает подсветку клавиатуры,
/// подсветку СЖО, режимы вентиляторов, экран помпы и встроенный профиль мыши — и переключает всё разом.
/// Переключатели: боковая панель, главная «Устройства» и меню значка в трее.
/// </summary>
public partial class MainWindow
{
    private bool _profilesReady;
    private RigProfile? _editProfile;
    private bool _profileUiLoading;

    private List<RigProfile> Profiles => _settings.Profiles ??= new List<RigProfile>();

    private RigProfile? ActiveProfile =>
        _settings.ActiveProfile is { } name ? Profiles.FirstOrDefault(p => p.Name == name) : null;

    /// <summary>После загрузки помпы и подсветки: все настройки уже в интерфейсе, их можно снимать в профиль.</summary>
    private void InitProfiles()
    {
        if (Profiles.Count == 0)
        {
            // Первые профили — из текущих настроек, отличаются вентиляторами и яркостью.
            var game = NewProfile("Игра", "");
            game.FanModes = null;
            game.FanModeAll = 3;          // производительный
            game.Brightness = 5;
            game.ArgbBrightness = 100;

            var work = NewProfile("Работа", "");
            work.FanModes = null;
            work.FanModeAll = 0;          // авто (BIOS)

            var quiet = NewProfile("Тихо", "");
            quiet.FanModes = null;
            quiet.FanModeAll = 1;         // тихий
            quiet.Brightness = 1;
            quiet.ArgbBrightness = 15;

            Profiles.AddRange(new[] { game, work, quiet });
            _settings.Save();
        }

        foreach (var (glyph, name) in RigProfile.Glyphs)
        {
            var button = new RadioButton
            {
                Style = (Style)FindResource("GlyphChip"),
                Content = glyph,
                Tag = glyph,
                ToolTip = name,
                GroupName = "ProfileGlyph"
            };
            System.Windows.Automation.AutomationProperties.SetName(button, "Значок: " + name);
            button.Click += (_, _) =>
            {
                if (_editProfile is null || _profileUiLoading) return;
                _editProfile.Glyph = glyph;
                _settings.Save();
                RefreshProfileUi();
            };
            ProfileGlyphs.Children.Add(button);
        }

        _editProfile = ActiveProfile ?? Profiles.FirstOrDefault();
        _profilesReady = true;
        RefreshProfileUi();
    }

    private RigProfile NewProfile(string name, string glyph)
    {
        var profile = new RigProfile { Name = name, Glyph = glyph };
        CaptureInto(profile);
        return profile;
    }

    /// <summary>Снимает текущие настройки в профиль (флажки «что меняет» не трогаем).</summary>
    private void CaptureInto(RigProfile p)
    {
        CollectSettings();
        CollectCoolerSettings();

        p.Effect = _settings.Effect;
        p.Color = _settings.Color;
        p.Rainbow = _settings.Rainbow;
        p.Brightness = _settings.Brightness;
        p.Speed = _settings.Speed;
        p.Direction = _settings.Direction;
        p.SyncLight = _settings.SyncLight;
        p.SyncStyle = _settings.SyncStyle;
        p.KeyPaint = _settings.KeyPaint is { } paint ? new Dictionary<string, string>(paint) : null;
        p.PaintAnim = _settings.PaintAnim;
        p.PaintSpeed = _settings.PaintSpeed;

        p.ArgbMode = _settings.ArgbMode;
        p.ArgbColor = _settings.ArgbColor;
        p.ArgbColor2 = _settings.ArgbColor2;
        p.ArgbColor3 = _settings.ArgbColor3;
        p.ArgbBrightness = _settings.ArgbBrightness;
        p.ArgbSpeed = _settings.ArgbSpeed;
        p.ArgbRainbow = _settings.ArgbRainbow;

        p.FanModes = _settings.FanModes is { } modes ? new Dictionary<string, int>(modes) : null;
        p.FanPercent = _settings.FanPercent is { } percent ? new Dictionary<string, int>(percent) : null;
        p.FanCurves = _settings.FanCurves?.ToDictionary(c => c.Key, c => (int[])c.Value.Clone());
        p.FanModeAll = _settings.FanModeAll;

        p.CoolerLayout = RigProfile.CloneLayout(_layout);
        p.CoolerBackground = _settings.CoolerBackground;
        p.CoolerBgTop = _settings.CoolerBgTop;
        p.CoolerBgBottom = _settings.CoolerBgBottom;
        p.CoolerBrightness = _settings.CoolerBrightness;
        p.CoolerFps = _settings.CoolerFps;
        p.CoolerTheme = _settings.CoolerTheme;

        if (_mouseConfig is { } cfg) p.MouseProfile = Math.Clamp(cfg.Profile, 0, 2);
    }

    // ── Применение ───────────────────────────────────────────────────────────

    private async Task ApplyProfileAsync(RigProfile p, bool fromTray = false)
    {
        if (!_profilesReady) return;

        var parts = new List<string>();
        try
        {
            if (p.UseKeyboard) { ApplyKeyboardPart(p); parts.Add("клавиатура"); }
            if (p.UseArgb) { ApplyArgbPart(p); parts.Add("СЖО"); }
            if (p.UseFans) { ApplyFansPart(p); parts.Add("вентиляторы"); }
            if (p.UsePump) { ApplyPumpPart(p); parts.Add("экран помпы"); }
        }
        catch (Exception ex)
        {
            QuietLog.Write(ex);
        }

        _settings.ActiveProfile = p.Name;
        _editProfile = p;
        _settings.Save();
        RefreshProfileUi();

        var text = parts.Count > 0
            ? $"Профиль «{p.Name}»: {string.Join(", ", parts)}."
            : $"Профиль «{p.Name}» ничего не меняет — отметьте, что он переключает.";
        ShowProfileNote(text);
        if (fromTray && !WindowShown) _tray?.ShowBalloon("Rig Control", text);

        // Мышь — последней: запись в неё занимает до полсекунды.
        if (p.UseMouse && p.MouseProfile is { } mp && _mouseConfig is { } cfg && cfg.Profile != mp)
        {
            _dpiSeparate = false;
            await MouseSendAsync(m => m.SwitchProfile(mp), $"Профиль {mp + 1}", settleMs: 300);
        }
    }

    private void ApplyKeyboardPart(RigProfile p)
    {
        _settings.Effect = Math.Clamp(p.Effect, 0, Ak820Protocol.Effects.Length - 1);
        _settings.Color = p.Color;
        _settings.Rainbow = p.Rainbow;
        _settings.Brightness = Math.Clamp(p.Brightness, 0, 5);
        _settings.Speed = Math.Clamp(p.Speed, 0, 5);
        _settings.Direction = Math.Clamp(p.Direction, 0, 3);
        _settings.SyncLight = p.SyncLight;
        _settings.SyncStyle = Math.Clamp(p.SyncStyle, 0, 2);
        _settings.KeyPaint = p.KeyPaint is { } paint ? new Dictionary<string, string>(paint) : null;
        _settings.PaintAnim = Math.Clamp(p.PaintAnim, 0, KeyPaint.Animations.Length - 1);
        _settings.PaintSpeed = Math.Clamp(p.PaintSpeed, 0, 5);

        ApplySettingsToUi();

        _paintAnimLoading = true;
        PaintAnimCombo.SelectedIndex = _settings.PaintAnim;
        PaintSpeedSlider.Value = _settings.PaintSpeed;
        PaintSpeedLabel.Text = $"Скорость: {_settings.PaintSpeed}";
        _paintAnimLoading = false;
        RefreshKeyCaps();
        RefreshPaintCount();

        // Как кнопка «Применить» на странице клавиатуры: эффект, раскраска или подсветка в такт гифке.
        SetLightOffState(false);
        if (_device is not null || _dongle is not null) OnApplyEffect(this, new RoutedEventArgs());
    }

    private void ApplyArgbPart(RigProfile p)
    {
        _settings.ArgbMode = p.ArgbMode;
        _settings.ArgbColor = p.ArgbColor;
        _settings.ArgbColor2 = p.ArgbColor2;
        _settings.ArgbColor3 = p.ArgbColor3;
        _settings.ArgbBrightness = p.ArgbBrightness;
        _settings.ArgbSpeed = p.ArgbSpeed;
        _settings.ArgbRainbow = p.ArgbRainbow;

        var wasLoading = _argbLoading;
        _argbLoading = true;
        var argbMode = p.ArgbMode == (int)ArgbMode.ColorWave ? (int)ArgbMode.RainbowWave : p.ArgbMode;
        ArgbModeCombo.SelectedIndex = Math.Clamp(argbMode, 0, ArgbModeCombo.Items.Count - 1);
        ArgbColorBox.Text = p.ArgbColor;
        ArgbColor2Box.Text = p.ArgbColor2;
        ArgbColor3Box.Text = p.ArgbColor3;
        ArgbRainbowCheck.IsChecked = p.ArgbRainbow;
        ArgbBrightness.Value = Math.Clamp(p.ArgbBrightness, 0, 100);
        ArgbSpeed.Value = Math.Clamp(p.ArgbSpeed, 1, 5);
        _argbLoading = wasLoading;

        // Отправляет новые настройки движку подсветки (если он уже запущен).
        ArgbSettingsChanged();
    }

    private void ApplyFansPart(RigProfile p)
    {
        _settings.FanModes = p.FanModes is { } modes ? new Dictionary<string, int>(modes) : null;
        if (p.FanPercent is { } percent) _settings.FanPercent = new Dictionary<string, int>(percent);
        if (p.FanCurves is { } curves) _settings.FanCurves = curves.ToDictionary(c => c.Key, c => (int[])c.Value.Clone());
        _settings.FanModeAll = Math.Clamp(p.FanModeAll, 0, FanModeNames.Length - 1);

        // Новая скорость — сразу, без плавного спуска; строки страницы перестроятся на следующем опросе.
        _fanLast.Clear();
        _fanRows.Clear();
        FansList.Children.Clear();
    }

    private void ApplyPumpPart(RigProfile p)
    {
        if (RigProfile.CloneLayout(p.CoolerLayout) is { Count: > 0 } layout)
        {
            _layout.Clear();
            _layout.AddRange(layout);
            _settings.CoolerLayout = _layout;
        }
        ClearSelectionState();

        _bgTop = p.CoolerBgTop;
        _bgBottom = p.CoolerBgBottom;
        SetBackgroundFrames(null);
        _backgroundPath = null;
        var bg = p.CoolerBackground;
        if (string.IsNullOrWhiteSpace(bg) || !(LcdRender.IsResource(bg) || File.Exists(bg)) || !LoadBackground(bg))
            BgName.Text = _bgTop is null && _bgBottom is null ? "Стандартный тёмный фон" : "Свой градиент";
        UpdateBgSwatches();
        _settings.CoolerTheme = p.CoolerTheme;

        _coolerLoading = true;
        BgBrightness.Value = Math.Clamp(p.CoolerBrightness, 0, 100);
        FpsSlider.Value = Math.Clamp(p.CoolerFps, 1, 10);
        _coolerLoading = false;
        _settings.CoolerFps = (int)Math.Round(FpsSlider.Value);
        UpdateBrightnessText();
        UpdateFpsLabel();

        RebuildScene();
        UpdateProps();
        SaveSoon();
    }

    // ── Интерфейс: боковая панель, главная, трей ──────────────────────────────

    private void RefreshProfileUi()
    {
        if (!_profilesReady) return;
        _profileUiLoading = true;
        try
        {
            var active = ActiveProfile;

            SideProfiles.Children.Clear();
            foreach (var p in Profiles)
            {
                var chip = new RadioButton
                {
                    Style = (Style)FindResource("ProfileChip"),
                    GroupName = "SideProfile",
                    IsChecked = ReferenceEquals(p, active),
                    ToolTip = $"Профиль «{p.Name}»: {ProfileSummary(p)}",
                    Content = ChipContent(p.Glyph, p.Name)
                };
                System.Windows.Automation.AutomationProperties.SetName(chip, "Профиль " + p.Name);
                chip.Click += async (_, _) => await ApplyProfileAsync(p);
                SideProfiles.Children.Add(chip);
            }

            ProfileCards.Children.Clear();
            foreach (var p in Profiles)
            {
                var card = new RadioButton
                {
                    Style = (Style)FindResource("ProfileCard"),
                    GroupName = "CardProfile",
                    IsChecked = ReferenceEquals(p, _editProfile),
                    Content = CardContent(p, ReferenceEquals(p, active)),
                    ToolTip = "Применить профиль"
                };
                System.Windows.Automation.AutomationProperties.SetName(card, "Профиль " + p.Name);
                card.Click += async (_, _) => await ApplyProfileAsync(p);
                ProfileCards.Children.Add(card);
            }

            var add = new Button { Style = (Style)FindResource("ProfileAddCard"), ToolTip = "Новый профиль из текущих настроек" };
            var addContent = new StackPanel { HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center };
            addContent.Children.Add(new TextBlock
            {
                Text = "", FontFamily = new FontFamily("Segoe MDL2 Assets"), FontSize = 20,
                HorizontalAlignment = HorizontalAlignment.Center
            });
            addContent.Children.Add(new TextBlock { Text = "Новый профиль", Margin = new Thickness(0, 8, 0, 0), FontWeight = FontWeights.SemiBold });
            add.Content = addContent;
            add.Click += (_, _) => OnProfileNew();
            ProfileCards.Children.Add(add);

            // Редактор выбранного профиля
            var edit = _editProfile;
            ProfileEditor.Visibility = edit is null ? Visibility.Collapsed : Visibility.Visible;
            if (edit is not null)
            {
                ProfileEditorTitle.Text = ReferenceEquals(edit, active) ? $"«{edit.Name}» — активный профиль" : $"Профиль «{edit.Name}»";
                if (!ProfileNameBox.IsKeyboardFocused) ProfileNameBox.Text = edit.Name;
                ProfUseKeyboard.IsChecked = edit.UseKeyboard;
                ProfUseArgb.IsChecked = edit.UseArgb;
                ProfUseFans.IsChecked = edit.UseFans;
                ProfUsePump.IsChecked = edit.UsePump;
                ProfUseMouse.IsChecked = edit.UseMouse;
                ProfUseMouse.ToolTip = edit.MouseProfile is { } mp
                    ? $"Переключит мышь на её встроенный профиль {mp + 1}"
                    : "Профиль мыши ещё не записан: подключите мышь и нажмите «Записать текущие настройки»";
                foreach (RadioButton glyph in ProfileGlyphs.Children)
                    glyph.IsChecked = (string)glyph.Tag == edit.Glyph;
                ProfileDetails.Text = ProfileSummary(edit, full: true);
                ProfileDeleteButton.IsEnabled = Profiles.Count > 1;
            }
        }
        finally
        {
            _profileUiLoading = false;
        }

        UpdateDevicesSummary();
    }

    private static StackPanel ChipContent(string glyph, string name)
    {
        var panel = new StackPanel();
        panel.Children.Add(new TextBlock
        {
            Text = glyph, FontFamily = new FontFamily("Segoe MDL2 Assets"), FontSize = 16,
            HorizontalAlignment = HorizontalAlignment.Center
        });
        panel.Children.Add(new TextBlock
        {
            Text = name, FontSize = 11.5, FontWeight = FontWeights.SemiBold, Margin = new Thickness(0, 5, 0, 0),
            HorizontalAlignment = HorizontalAlignment.Center, TextTrimming = TextTrimming.CharacterEllipsis
        });
        return panel;
    }

    private Grid CardContent(RigProfile p, bool active)
    {
        var grid = new Grid();
        grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        grid.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });

        var glyph = new TextBlock { Text = p.Glyph, FontFamily = new FontFamily("Segoe MDL2 Assets"), FontSize = 22 };
        grid.Children.Add(glyph);

        if (active)
        {
            var badge = new Border
            {
                CornerRadius = new CornerRadius(6), Padding = new Thickness(7, 2, 7, 3),
                HorizontalAlignment = HorizontalAlignment.Right, VerticalAlignment = VerticalAlignment.Top,
                Child = new TextBlock { Text = "Активен", FontSize = 11, FontWeight = FontWeights.SemiBold }
            };
            badge.SetResourceReference(Border.BackgroundProperty, "GoodBrush");
            ((TextBlock)badge.Child).SetResourceReference(TextBlock.ForegroundProperty, "AccentTextBrush");
            grid.Children.Add(badge);
        }

        var name = new TextBlock
        {
            Text = p.Name, FontSize = 16, FontWeight = FontWeights.SemiBold, Margin = new Thickness(0, 10, 0, 0),
            TextTrimming = TextTrimming.CharacterEllipsis
        };
        Grid.SetRow(name, 1);
        grid.Children.Add(name);

        var summary = new TextBlock
        {
            Text = ProfileSummary(p), FontSize = 11.5, Margin = new Thickness(0, 4, 0, 0),
            TextWrapping = TextWrapping.Wrap, TextTrimming = TextTrimming.CharacterEllipsis, LineHeight = 16
        };
        summary.SetResourceReference(TextBlock.ForegroundProperty, "MutedBrush");
        Grid.SetRow(summary, 2);
        grid.Children.Add(summary);
        return grid;
    }

    /// <summary>«Волна · СЖО: радуга · вентиляторы: тихий».</summary>
    private string ProfileSummary(RigProfile p, bool full = false)
    {
        var parts = new List<string>();
        if (p.UseKeyboard)
        {
            var effect = Ak820Protocol.Effects[Math.Clamp(p.Effect, 0, Ak820Protocol.Effects.Length - 1)].Name;
            parts.Add(full ? $"Клавиатура: {effect}, яркость {p.Brightness}" : effect);
        }
        if (p.UseArgb)
        {
            var mode = ArgbModeCombo.Items.Count > p.ArgbMode && p.ArgbMode >= 0
                ? (ArgbModeCombo.Items[p.ArgbMode] as ComboBoxItem)?.Content as string ?? ""
                : "";
            var argb = p.ArgbMode == 0 ? "не трогать" : $"{mode.ToLowerInvariant()}" + (full ? $", {p.ArgbBrightness} %" : "");
            parts.Add((full ? "Подсветка СЖО: " : "СЖО: ") + argb);
        }
        if (p.UseFans) parts.Add((full ? "Вентиляторы: " : "вент.: ") + FanSummary(p.FanModes, p.FanModeAll).ToLowerInvariant());
        if (p.UsePump) parts.Add(full ? $"Экран помпы: {(p.CoolerTheme is { } t ? $"тема «{t}»" : "своя расстановка")}" : "помпа");
        if (p.UseMouse && p.MouseProfile is { } mp) parts.Add(full ? $"Мышь: профиль {mp + 1}" : $"мышь {mp + 1}");

        if (parts.Count == 0) return "Ничего не переключает";
        return full ? string.Join(" · ", parts) + "." : string.Join(" · ", parts);
    }

    private static string FanSummary(Dictionary<string, int>? modes, int all)
    {
        var values = modes is { Count: > 0 } ? modes.Values.Distinct().ToList() : new List<int> { all };
        return values.Count == 1 ? FanModeNames[Math.Clamp(values[0], 0, FanModeNames.Length - 1)] : "свои режимы";
    }

    private void ShowProfileNote(string text)
    {
        ProfileNote.Text = text;
        ProfileNote.Visibility = Visibility.Visible;
    }

    // ── Редактор профиля ─────────────────────────────────────────────────────

    private void OnProfileNew()
    {
        var n = Profiles.Count + 1;
        var name = $"Профиль {n}";
        while (Profiles.Any(p => p.Name == name)) name = $"Профиль {++n}";

        var profile = NewProfile(name, "");
        profile.UseMouse = profile.MouseProfile is not null;
        Profiles.Add(profile);
        _editProfile = profile;
        _settings.ActiveProfile = profile.Name;   // он и есть текущие настройки
        _settings.Save();
        RefreshProfileUi();
        ShowProfileNote($"Создан профиль «{name}» из текущих настроек. Переименуйте его и отметьте, что он переключает.");
        ProfileNameBox.Focus();
        ProfileNameBox.SelectAll();
    }

    private async void OnProfileApply(object sender, RoutedEventArgs e)
    {
        if (_editProfile is { } p) await ApplyProfileAsync(p);
    }

    private void OnProfileSave(object sender, RoutedEventArgs e)
    {
        if (_editProfile is not { } p) return;

        CaptureInto(p);
        _settings.ActiveProfile = p.Name;
        _settings.Save();
        RefreshProfileUi();
        ShowProfileNote($"В профиль «{p.Name}» записаны текущие настройки.");
    }

    private void OnProfileDelete(object sender, RoutedEventArgs e)
    {
        if (_editProfile is not { } p || Profiles.Count <= 1) return;
        if (!ConfirmTwice(ProfileDeleteButton, "Точно удалить?")) return;

        Profiles.Remove(p);
        if (_settings.ActiveProfile == p.Name) _settings.ActiveProfile = null;
        _editProfile = ActiveProfile ?? Profiles.FirstOrDefault();
        _settings.Save();
        RefreshProfileUi();
        ShowProfileNote($"Профиль «{p.Name}» удалён.");
    }

    private void OnProfilePartClick(object sender, RoutedEventArgs e)
    {
        if (_editProfile is not { } p || _profileUiLoading) return;

        p.UseKeyboard = ProfUseKeyboard.IsChecked == true;
        p.UseArgb = ProfUseArgb.IsChecked == true;
        p.UseFans = ProfUseFans.IsChecked == true;
        p.UsePump = ProfUsePump.IsChecked == true;
        p.UseMouse = ProfUseMouse.IsChecked == true;

        // Включили мышь, а её профиль ещё не записан — берём текущий.
        if (p.UseMouse && p.MouseProfile is null && _mouseConfig is { } cfg) p.MouseProfile = Math.Clamp(cfg.Profile, 0, 2);

        _settings.Save();
        RefreshProfileUi();
    }

    private void OnProfileNameKey(object sender, KeyEventArgs e)
    {
        if (e.Key != Key.Enter) return;
        CommitProfileName();
        e.Handled = true;
    }

    private void OnProfileNameLostFocus(object sender, RoutedEventArgs e) => CommitProfileName();

    private void CommitProfileName()
    {
        if (_editProfile is not { } p) return;

        var name = ProfileNameBox.Text.Trim();
        if (name.Length == 0 || name == p.Name || Profiles.Any(o => !ReferenceEquals(o, p) && o.Name == name))
        {
            ProfileNameBox.Text = p.Name;
            return;
        }

        if (_settings.ActiveProfile == p.Name) _settings.ActiveProfile = name;
        p.Name = name;
        _settings.Save();
        RefreshProfileUi();
    }

    /// <summary>Пункты профилей в меню значка в трее.</summary>
    private void AddTrayProfiles(ContextMenu menu, Style? itemStyle)
    {
        if (!_profilesReady || Profiles.Count == 0) return;

        var active = ActiveProfile;
        foreach (var p in Profiles)
        {
            var item = new MenuItem
            {
                Header = (ReferenceEquals(p, active) ? "●  " : "○  ") + p.Name,
                Style = itemStyle,
                ToolTip = ProfileSummary(p)
            };
            item.Click += async (_, _) => await ApplyProfileAsync(p, fromTray: true);
            menu.Items.Add(item);
        }
        menu.Items.Add(new Separator { Style = TryFindResource("TrayMenuSeparator") as Style });
    }
}
