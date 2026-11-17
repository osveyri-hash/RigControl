using System.IO;
using System.Text.Json;

namespace RigControl;

/// <summary>Что запоминаем между запусками.</summary>
public sealed class Settings
{
    public string? LastImage { get; set; }
    public bool Crop { get; set; } = true;
    public int MaxFrames { get; set; } = 40;

    public int Effect { get; set; } = 1;
    public string Color { get; set; } = "#FFFFFF";
    public bool Rainbow { get; set; }
    public int Brightness { get; set; } = 5;
    public int Speed { get; set; } = 3;
    public int Direction { get; set; }

    /// <summary>Подсветка в такт гифке на экране клавиатуры.</summary>
    public bool SyncLight { get; set; }

    /// <summary>Как подсветка повторяет гифку: 0 — картинка на клавишах, 1 — волна от экрана, 2 — один цвет кадра.</summary>
    public int SyncStyle { get; set; }

    // Экран помпы
    public int CoolerMode { get; set; }          // 0 — мониторинг, 1 — картинка/гифка
    public string? CoolerImage { get; set; }
    public int CoolerRotation { get; set; }      // 0/90/180/270
    public int CoolerSize { get; set; }          // 0 — авто, иначе индекс в списке размеров
    // Новый редактор экрана помпы
    public List<LcdItem>? CoolerLayout { get; set; }
    public string? CoolerBackground { get; set; }
    public string? CoolerBgTop { get; set; }       // цвета градиента фона (тема); пусто — стандартные
    public string? CoolerBgBottom { get; set; }
    public int CoolerBrightness { get; set; } = 100;   // яркость фона, 0–100 %
    public int CoolerFps { get; set; } = 2;            // кадров в секунду, 1–10
    public string? CoolerTheme { get; set; }           // последняя выбранная тема — для смены ориентации

    // Своя раскраска клавиш: код подсветки → #RRGGBB
    public Dictionary<string, string>? KeyPaint { get; set; }
    public string? PaintColor { get; set; }
    public int PaintAnim { get; set; }          // 0 — без анимации (KeyPaint.Animations)
    public int PaintSpeed { get; set; } = 2;    // 0–5
    public bool PaintMirror { get; set; } = true;   // схема показывает подсветку, которая горит сейчас

    // Подсветка СЖО: ARGB-разъём платы ASUS (ArgbMode; 0 — не трогать)
    public int ArgbMode { get; set; }
    public string ArgbColor { get; set; } = "#00B4FF";
    public string ArgbColor2 { get; set; } = "#FF00A8";
    public string ArgbColor3 { get; set; } = "#8A2BFF";
    public int ArgbBrightness { get; set; } = 100;   // 0–100 %
    public int ArgbSpeed { get; set; } = 3;          // 1–5
    public int ArgbLeds { get; set; } = 30;
    public int ArgbFans { get; set; } = 3;           // вентиляторов в цепочке — для «как клавиатура»
    public int ArgbTarget { get; set; }              // 0 — все разъёмы, 1 — ARGB 1, …
    public bool ArgbBoardEffect { get; set; }        // эффект крутит сама плата (режим совместимости)
    public bool ArgbRainbow { get; set; } = true;    // радуга в режиме «Плавная волна», иначе выбранный цвет
    public bool ArmouryStopped { get; set; }         // службу подсветки Armoury Crate остановили мы
    public Dictionary<string, double>? EffectRates { get; set; }   // подгонка скорости эффектов клавиатуры («режим:скорость»)

    // Вентиляторы платы: идентификатор канала → режим (FanModes, 0 — авто/BIOS) и своя скорость, %
    public Dictionary<string, int>? FanModes { get; set; }
    public Dictionary<string, int>? FanPercent { get; set; }
    public Dictionary<string, int[]>? FanCurves { get; set; }   // своя кривая: [t0, p0, t1, p1, …]
    public int FanModeAll { get; set; }                          // режим для каналов, которых нет в FanModes (его задают профили)

    // Профили одной кнопкой: наборы настроек подсветки, вентиляторов, экрана помпы и мыши
    public List<RigProfile>? Profiles { get; set; }
    public string? ActiveProfile { get; set; }

    // Поведение программы
    public bool CloseToTray { get; set; } = true;
    public bool AutoOutput { get; set; } = true;

    private static readonly JsonSerializerOptions Options = new() { WriteIndented = true };

    private static string FilePath =>
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData),
            "RigControl", "settings.json");

    public static Settings Load()
    {
        try
        {
            if (File.Exists(FilePath))
            {
                var settings = JsonSerializer.Deserialize<Settings>(File.ReadAllText(FilePath)) ?? new Settings();

                // В ранней версии «Плавная волна» была отдельным пунктом. Теперь это
                // обычная волна с выбором радуги или палитры — переносим старое значение.
                if (settings.ArgbMode == (int)global::RigControl.ArgbMode.ColorWave)
                    settings.ArgbMode = (int)global::RigControl.ArgbMode.RainbowWave;
                if (settings.Profiles is not null)
                    foreach (var profile in settings.Profiles)
                        if (profile.ArgbMode == (int)global::RigControl.ArgbMode.ColorWave)
                            profile.ArgbMode = (int)global::RigControl.ArgbMode.RainbowWave;

                return settings;
            }
        }
        catch (Exception ex)
        {
            // Битый файл — начинаем с настроек по умолчанию, а сам файл сохраняем как settings.json.bad:
            // иначе первое же сохранение затрёт его, и настройки пропадут без следа.
            QuietLog.Write(ex);
            try { File.Copy(FilePath, FilePath + ".bad", overwrite: true); }
            catch { /* не получилось — запуску не мешаем */ }
        }

        return new Settings();
    }

    public void Save()
    {
        try
        {
            Directory.CreateDirectory(Path.GetDirectoryName(FilePath)!);

            // Пишем во временный файл и подменяем: если ПК выключится посреди записи,
            // старые настройки останутся целыми, а не превратятся в обрезанный JSON.
            var json = JsonSerializer.Serialize(this, Options);
            var temp = FilePath + ".tmp";
            File.WriteAllText(temp, json);
            File.Move(temp, FilePath, overwrite: true);
        }
        catch (Exception ex)
        {
            // Нет прав на запись — не мешаем работе.
            QuietLog.Write(ex);
        }
    }
}
