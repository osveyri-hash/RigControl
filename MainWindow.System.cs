using System.Windows;
using System.Windows.Controls;

namespace RigControl;

/// <summary>Страница «О системе»: характеристики ПК и показатели андервольтинга процессора и видеокарты.</summary>
public partial class MainWindow
{
    /// <summary>С какой загрузки CPU/GPU считаются «под нагрузкой» — по этим секундам считаются средние.</summary>
    private const double SysLoadThreshold = 90;

    /// <summary>Строка таблицы: текущее значение, пик за запись и паспортное значение модели.</summary>
    private sealed class SysMetric
    {
        public string Key = "";
        public required string Label;
        public required Func<Snapshot, double?> Get;
        public required Func<double, string> Format;
        public Func<SystemSpecs, string?>? Spec;
        public bool Peak = true;
        public double? Max;
        public TextBlock? Now, Top;
    }

    /// <summary>Средние под нагрузкой: частота, напряжение, мощность и самая высокая температура.</summary>
    private sealed class LoadStats
    {
        private double _clock, _volt, _power;
        private int _clockN, _voltN, _powerN;

        public int Seconds { get; private set; }
        public double? TempMax { get; private set; }
        public double? Clock => _clockN > 0 ? _clock / _clockN : null;
        public double? Volt => _voltN > 0 ? _volt / _voltN : null;
        public double? Power => _powerN > 0 ? _power / _powerN : null;

        public void Add(double? clock, double? volt, double? power, double? temp)
        {
            Seconds++;
            Sum(clock, ref _clock, ref _clockN);
            Sum(volt, ref _volt, ref _voltN);
            Sum(power, ref _power, ref _powerN);
            if (temp is { } t && double.IsFinite(t)) TempMax = Math.Max(TempMax ?? t, t);
        }

        private static void Sum(double? value, ref double sum, ref int count)
        {
            if (value is not { } v || !double.IsFinite(v)) return;
            sum += v;
            count++;
        }
    }

    private readonly SysMetric[] _cpuMetrics =
    {
        new() { Label = "Загрузка", Get = s => s.CpuLoad, Format = Pct, Peak = false },
        new() { Label = "Температура", Get = s => s.CpuTemp, Format = Deg, Spec = s => s.Cpu is { } c ? $"предел {c.TjMaxC:0} °C" : null },
        new() { Label = "Напряжение ядер (Vcore)", Get = s => s.Uv?.CpuVcore, Format = Volt },
        new() { Label = "Частота, средняя", Get = s => s.CpuClockMhz, Format = Ghz, Spec = s => s.Cpu is { } c ? "база " + Ghz(c.BaseMhz) : null },
        new() { Label = "Частота, лучшее ядро", Get = s => s.Uv?.CpuClockMaxMhz, Format = Ghz, Spec = s => s.Cpu is { } c ? "буст до " + Ghz(c.BoostMhz) : null },
        new() { Label = "Мощность (Package)", Get = s => s.CpuPowerW, Format = Watt, Spec = s => s.Cpu is { } c ? $"PPT {c.PptW:0} Вт" : null }
    };

    private readonly SysMetric[] _gpuMetrics =
    {
        new() { Label = "Загрузка", Get = s => s.GpuLoad, Format = Pct, Peak = false },
        new() { Label = "Температура ядра", Get = s => s.GpuTemp, Format = Deg },
        new() { Label = "Хотспот", Get = s => s.Uv?.GpuHotSpot, Format = Deg, Spec = s => s.Gpu is { } g ? $"предел {g.HotSpotLimitC:0} °C" : null },
        new() { Label = "Температура памяти", Get = s => s.Uv?.GpuMemTemp, Format = Deg },
        new() { Key = "gpu-volt", Label = "Напряжение ядра", Get = s => s.Uv?.GpuCoreVolt, Format = MilliVolt, Spec = s => s.Gpu is { } g ? $"сток до {g.MaxVoltMv:0} мВ" : null },
        new() { Label = "Частота ядра", Get = s => s.Uv?.GpuCoreClockMhz, Format = Mhz, Spec = s => s.Gpu is { } g ? $"буст {g.BoostMhz:0} МГц" : null },
        new() { Label = "Частота памяти", Get = s => s.Uv?.GpuMemClockMhz, Format = Mhz, Spec = s => s.Gpu is { } g ? $"{g.MemClockMhz:0} МГц" : null },
        new() { Label = "Мощность", Get = s => s.GpuPowerW, Format = Watt, Spec = s => s.Gpu is { } g ? $"TBP {g.BoardPowerW:0} Вт" : null },
        new() { Label = "Вентилятор", Get = s => s.Uv?.GpuFanRpm, Format = Rpm }
    };

    private SystemSpecs? _sysSpecs;
    private bool _sysSpecsLoading;
    private bool _sysTablesBuilt;
    private bool _sysRecord;
    private bool _sysSensors = true;
    private Snapshot? _sysLast;
    private LoadStats _cpuStats = new();
    private LoadStats _gpuStats = new();
    private ZenReport? _zen;
    private bool _zenLoading;
    private long _zenReadAt;

    /// <summary>Датчики нужны: открыта страница или включена запись в фоне.</summary>
    private bool SystemWanted => (NavSystem?.IsChecked == true && WindowShown) || _sysRecord;

    private static string Pct(double v) => $"{v:0} %";
    private static string Deg(double v) => $"{v:0} °C";
    private static string Volt(double v) => $"{v:0.000} В";
    private static string MilliVolt(double v) => $"{v * 1000:0} мВ";
    private static string Ghz(double mhz) => $"{mhz / 1000:0.00} ГГц";
    private static string Mhz(double v) => $"{v:0} МГц";
    private static string Watt(double v) => $"{v:0} Вт";
    private static string Rpm(double v) => $"{v:0} об/мин";

    private static string Duration(int seconds) => seconds < 60 ? $"{seconds} с" : $"{seconds / 60} мин {seconds % 60} с";

    private async void ShowSystemPage()
    {
        if (!_sysTablesBuilt) BuildSysTables();
        DrawSystem();
        _ = ReadZenAsync();

        if (_sysSpecs is not null || _sysSpecsLoading) return;
        _sysSpecsLoading = true;
        try
        {
            _sysSpecs = await Task.Run(SystemInfo.Read);
        }
        catch (Exception ex)
        {
            QuietLog.Write(ex);
            SysSpecsNote.Text = "Не удалось прочитать характеристики.";
            return;
        }
        finally
        {
            _sysSpecsLoading = false;
        }

        DrawSpecs(_sysSpecs);
        BuildSysTables();
        DrawSystem();
    }

    /// <summary>Показания очередного опроса датчиков (раз в секунду): копим пики и средние, перерисовываем страницу.</summary>
    private void UpdateSystemPage(Snapshot s, bool sensorsAvailable)
    {
        _sysLast = s;
        _sysSensors = sensorsAvailable;

        foreach (var m in _cpuMetrics.Concat(_gpuMetrics))
            if (m.Peak && m.Get(s) is { } v && double.IsFinite(v))
                m.Max = Math.Max(m.Max ?? v, v);

        if (s.CpuLoad >= SysLoadThreshold) _cpuStats.Add(s.CpuClockMhz, s.Uv?.CpuVcore, s.CpuPowerW, s.CpuTemp);
        if (s.GpuLoad >= SysLoadThreshold)
            _gpuStats.Add(s.Uv?.GpuCoreClockMhz, s.Uv?.GpuCoreVolt, s.GpuPowerW, s.Uv?.GpuHotSpot ?? s.GpuTemp);

        if (PageSystem.IsVisible && WindowShown) DrawSystem();
    }

    /// <summary>Curve Optimizer и тайминги — при открытии страницы, не чаще раза в 30 секунд (меняются только из BIOS).</summary>
    private async Task ReadZenAsync()
    {
        if (_zenLoading || (_zen is not null && Environment.TickCount64 - _zenReadAt < 30_000)) return;
        _zenLoading = true;
        try
        {
            _zen = await Task.Run(ZenTuning.Read);
            _zenReadAt = Environment.TickCount64;
        }
        catch (Exception ex)
        {
            QuietLog.Write(ex);
            _zen = new ZenReport(null, "Не удалось прочитать.", null, "Не удалось прочитать.");
        }
        finally
        {
            _zenLoading = false;
        }

        DrawZen(_zen);
        DrawSystem();
    }

    /// <summary>«−10», «+5», «0».</summary>
    private static string Signed(int value) => value switch { > 0 => $"+{value}", < 0 => $"−{-value}", _ => "0" };

    private void DrawZen(ZenReport zen)
    {
        SysCoCores.Children.Clear();
        if (zen.CurveOptimizer is { Count: > 0 } co)
        {
            var same = co.All(v => v == co[0]);
            SysCoValue.Text = same ? Signed(co[0]) : $"{Signed(co.Min())}…{Signed(co.Max())}";
            SysCoNote.Text = co.All(v => v == 0)
                ? "Андервольт не настроен: на всех ядрах 0."
                : same
                    ? $"На всех {co.Count} ядрах. Отрицательное значение — андервольт: на той же частоте ниже напряжение."
                    : $"По ядрам, в среднем {Signed((int)Math.Round(co.Average()))}. Отрицательное значение — андервольт.";

            if (!same)
            {
                for (var i = 0; i < co.Count; i++)
                {
                    var chip = new Border
                    {
                        CornerRadius = new CornerRadius(6), Padding = new Thickness(8, 3, 8, 3), Margin = new Thickness(0, 0, 6, 6),
                        Child = new TextBlock { Text = $"Ядро {i}: {Signed(co[i])}", FontSize = 12.5 }
                    };
                    chip.SetResourceReference(Border.BackgroundProperty, "Surface2Brush");
                    SysCoCores.Children.Add(chip);
                }
            }
        }
        else
        {
            SysCoValue.Text = "—";
            SysCoNote.Text = zen.CoNote ?? "";
        }

        SysRamGrid.Children.Clear();
        if (zen.Timings is { } t)
        {
            SysRamTitle.Text = $"DDR5-{t.DataRate} · CL{t.Cl}-{t.RcdRd}-{t.Rp}-{t.Ras}";
            SysRamNote.Text = $"MEMCLK {t.MemClkMhz:0} МГц · Command Rate {(t.Cmd2T ? "2T" : "1T")} · Gear Down {(t.Gdm ? "вкл" : "выкл")} · " +
                              $"Power Down {(t.PowerDown ? "вкл" : "выкл")}. Тайминги в тактах, как в BIOS и ZenTimings.";

            var cells = new (string Label, string Value)[]
            {
                ("tCL", $"{t.Cl}"), ("tRCD RD", $"{t.RcdRd}"), ("tRCD WR", $"{t.RcdWr}"), ("tRP", $"{t.Rp}"), ("tRAS", $"{t.Ras}"),
                ("tRC", $"{t.Rc}"), ("tRRD_S", $"{t.Rrds}"), ("tRRD_L", $"{t.Rrdl}"), ("tFAW", $"{t.Faw}"), ("tWR", $"{t.Wr}"),
                ("tWTR_S", $"{t.Wtrs}"), ("tWTR_L", $"{t.Wtrl}"), ("tRTP", $"{t.Rtp}"), ("tCWL", $"{t.Cwl}"),
                ("tRFC", t.Rfc > 0 ? $"{t.Rfc} ({t.Ns(t.Rfc):0} нс)" : "—"),
                ("tRFC2", t.Rfc2 > 0 ? $"{t.Rfc2}" : "—"), ("tRFCsb", t.RfcSb > 0 ? $"{t.RfcSb}" : "—"),
                ("tREFI", t.Refi > 0 ? $"{t.Refi} ({t.Ns(t.Refi) / 1000:0.0} мкс)" : "—")
            };
            foreach (var (label, value) in cells)
            {
                var cell = new StackPanel { Margin = new Thickness(0, 8, 12, 0) };
                cell.Children.Add(Cell(label, muted: true));
                cell.Children.Add(Cell(value, bold: true));
                SysRamGrid.Children.Add(cell);
            }
        }
        else
        {
            SysRamTitle.Text = "—";
            SysRamNote.Text = zen.TimingsNote ?? "";
        }
    }

    private void OnSysRecord(object sender, RoutedEventArgs e)
    {
        _sysRecord = SysRecordSwitch.IsChecked == true;
        SysRecordText.Text = _sysRecord
            ? "Запись идёт — окно можно свернуть или спрятать в трей. После теста выключите: датчики опрашиваются, пока запись включена."
            : "Датчики опрашиваются раз в секунду, пока запись включена.";
    }

    private void OnSysReset(object sender, RoutedEventArgs e)
    {
        foreach (var m in _cpuMetrics.Concat(_gpuMetrics)) m.Max = null;
        _cpuStats = new LoadStats();
        _gpuStats = new LoadStats();
        DrawSystem();
    }

    private void DrawSpecs(SystemSpecs specs)
    {
        SysSpecsNote.Visibility = Visibility.Collapsed;
        SysSpecsGrid.Children.Clear();
        SysSpecsGrid.RowDefinitions.Clear();
        SysSpecsGrid.ColumnDefinitions.Clear();
        SysSpecsGrid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(170) });
        SysSpecsGrid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });

        for (var i = 0; i < specs.Rows.Count; i++)
        {
            var row = specs.Rows[i];
            SysSpecsGrid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });

            var label = Cell(row.Label, muted: true);
            label.Margin = new Thickness(0, 10, 12, 0);
            Grid.SetRow(label, i);
            SysSpecsGrid.Children.Add(label);

            var value = new StackPanel { Margin = new Thickness(0, 10, 0, 0) };
            value.Children.Add(Cell(row.Value, bold: true));
            if (!string.IsNullOrWhiteSpace(row.Detail))
                value.Children.Add(new TextBlock { Text = row.Detail, Style = (Style)FindResource("Cap"), Margin = new Thickness(0, 2, 0, 0) });
            Grid.SetRow(value, i);
            Grid.SetColumn(value, 1);
            SysSpecsGrid.Children.Add(value);
        }

        SysCpuTitle.Text = specs.CpuName ?? "Процессор";
        SysGpuTitle.Text = specs.GpuName ?? "Видеокарта";

        SysTipsCpu.Text = specs.Cpu?.Tips ?? "";
        SysTipsCpu.Visibility = specs.Cpu is null ? Visibility.Collapsed : Visibility.Visible;
        SysTipsGpu.Text = specs.Gpu?.Tips ?? "";
        SysTipsGpu.Visibility = specs.Gpu is null ? Visibility.Collapsed : Visibility.Visible;
        SysTipsCard.Visibility = specs.Cpu is null && specs.Gpu is null ? Visibility.Collapsed : Visibility.Visible;
    }

    private void BuildSysTables()
    {
        BuildSysTable(SysCpuGrid, _cpuMetrics);
        BuildSysTable(SysGpuGrid, _gpuMetrics);
        _sysTablesBuilt = true;
    }

    private void BuildSysTable(Grid grid, SysMetric[] metrics)
    {
        grid.Children.Clear();
        grid.RowDefinitions.Clear();
        grid.ColumnDefinitions.Clear();
        foreach (var width in new[] { 1.4, 1.0, 1.0, 1.2 })
            grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(width, GridUnitType.Star) });

        var specs = _sysSpecs;
        AddSysRow(grid, 0, false, Cell("", muted: true), Cell("Сейчас", muted: true), Cell("Пик за запись", muted: true),
            Cell(specs is null ? "" : "Паспорт", muted: true));

        for (var i = 0; i < metrics.Length; i++)
        {
            var m = metrics[i];
            m.Now = Cell("—", bold: true);
            m.Top = Cell(m.Peak ? "—" : "");
            var spec = specs is not null ? m.Spec?.Invoke(specs) : null;
            AddSysRow(grid, i + 1, true, Cell(m.Label), m.Now, m.Top, Cell(spec ?? "", muted: true));
        }
    }

    private static void AddSysRow(Grid grid, int row, bool line, params TextBlock[] cells)
    {
        grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });

        if (line)
        {
            var separator = new Border { BorderThickness = new Thickness(0, 1, 0, 0) };
            separator.SetResourceReference(Border.BorderBrushProperty, "LineBrush");
            Grid.SetRow(separator, row);
            Grid.SetColumnSpan(separator, cells.Length);
            grid.Children.Add(separator);
        }

        for (var col = 0; col < cells.Length; col++)
        {
            cells[col].Margin = new Thickness(0, 7, 8, 7);
            Grid.SetRow(cells[col], row);
            Grid.SetColumn(cells[col], col);
            grid.Children.Add(cells[col]);
        }
    }

    private static TextBlock Cell(string text, bool muted = false, bool bold = false)
    {
        var cell = new TextBlock
        {
            Text = text,
            TextTrimming = TextTrimming.CharacterEllipsis,
            FontWeight = bold ? FontWeights.SemiBold : FontWeights.Normal,
            VerticalAlignment = VerticalAlignment.Center
        };
        if (muted)
        {
            cell.FontSize = 12.5;
            cell.SetResourceReference(TextBlock.ForegroundProperty, "MutedBrush");
        }
        return cell;
    }

    private void DrawSystem()
    {
        if (!_sysTablesBuilt) return;

        var s = _sysLast;
        foreach (var m in _cpuMetrics.Concat(_gpuMetrics))
        {
            var value = s is { } snap ? m.Get(snap) : null;
            m.Now!.Text = value is { } v && double.IsFinite(v) ? m.Format(v) : "—";
            if (m.Peak) m.Top!.Text = m.Max is { } top ? m.Format(top) : "—";
        }

        var uv = s?.Uv;
        if (_sysSpecs?.CpuName is null && uv?.CpuName is { } cpu) SysCpuTitle.Text = cpu;
        if (_sysSpecs?.GpuName is null && uv?.GpuName is { } gpu) SysGpuTitle.Text = gpu;

        SysVcoreNote.Text = uv?.CpuVcoreSource switch
        {
            "SVI" => "Vcore — телеметрия регулятора напряжения (SVI).",
            "плата" => "Vcore — с чипа мониторинга материнской платы.",
            "VID" => "Показан VID — напряжение, которое запрашивает процессор; реальное обычно немного ниже.",
            _ => ""
        };
        SysVcoreNote.Visibility = SysVcoreNote.Text.Length > 0 ? Visibility.Visible : Visibility.Collapsed;

        SysCpuSummary.Text = CpuSummary();
        SysGpuSummary.Text = GpuSummary();

        SysSensorsNote.Text = !_sysSensors
            ? "Датчики недоступны: нужен запуск от администратора и драйвер PawnIO (pawnio.eu) — его использует LibreHardwareMonitor."
            : s is { CpuTemp: null }
                ? "Температура и напряжение процессора не читаются — установите драйвер PawnIO (pawnio.eu), его использует LibreHardwareMonitor."
                : "";
        SysSensorsNote.Visibility = SysSensorsNote.Text.Length > 0 ? Visibility.Visible : Visibility.Collapsed;
    }

    private string CpuSummary()
    {
        var co = _zen?.CurveOptimizer is { Count: > 0 } values
            ? values.All(v => v == values[0])
                ? $"Curve Optimizer {Signed(values[0])} на всех ядрах. "
                : $"Curve Optimizer от {Signed(values.Min())} до {Signed(values.Max())} по ядрам. "
            : "";

        var st = _cpuStats;
        if (st.Seconds < 5) return co + "Процессор: нагрузки от 90 % ещё не было — запустите Cinebench R23 или OCCT.";

        var parts = new List<string>();
        if (st.Clock is { } clock) parts.Add(Ghz(clock));
        if (st.Volt is { } volt) parts.Add(Volt(volt));
        if (st.Power is { } power) parts.Add(Watt(power));
        if (st.TempMax is { } temp) parts.Add("до " + Deg(temp));

        var text = co + $"Процессор под нагрузкой ({Duration(st.Seconds)}): {string.Join(" · ", parts)}.";
        if (st.Clock is { } c && st.Power is > 1 and var p) text += $" Эффективность {c / p:0} МГц/Вт.";
        return text;
    }

    private string GpuSummary()
    {
        var st = _gpuStats;
        if (st.Seconds < 5) return "Видеокарта: нагрузки от 90 % ещё не было — запустите игру или 3DMark.";

        var parts = new List<string>();
        if (st.Clock is { } clock) parts.Add(Mhz(clock));
        if (st.Volt is { } volt) parts.Add(MilliVolt(volt));
        if (st.Power is { } power) parts.Add(Watt(power));
        if (st.TempMax is { } temp) parts.Add("до " + Deg(temp));

        var text = $"Видеокарта под нагрузкой ({Duration(st.Seconds)}): {string.Join(" · ", parts)}.";
        if (st.Clock is { } c && st.Power is > 1 and var p) text += $" Эффективность {c / p:0.0} МГц/Вт.";

        // Андервольт виден по пику напряжения: стоковая кривая хоть раз доходит до своей верхней точки.
        var peak = _gpuMetrics.First(m => m.Key == "gpu-volt").Max;
        if (peak is { } pv && _sysSpecs?.Gpu is { } spec && st.Seconds >= 30)
        {
            var mv = pv * 1000;
            var below = spec.MaxVoltMv - mv;
            text += below >= 15
                ? $" Пик напряжения {mv:0} мВ — на {below:0} мВ ниже стоковых {spec.MaxVoltMv:0} мВ: андервольт работает."
                : $" Пик напряжения {mv:0} мВ — как в стоке ({spec.MaxVoltMv:0} мВ): андервольт не применён.";
        }

        return text;
    }
}
