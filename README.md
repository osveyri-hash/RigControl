# RigControl (Rust)

Порт RigControl с C# (WPF) на Rust (egui). Управляет клавиатурой AJAZZ AK820 Pro, мышью MCHOSE,
экраном помпы Thermalright (0416:5302), подсветкой ARGB платы ASUS (AURA) и вентиляторами платы.
Только Windows. Настройки (`%AppData%\RigControl\settings.json`) общие с C#-версией.

Исходник C#-версии (2.17.3) лежит в истории репозитория: коммит `4ef70d0` и раньше.

## Что собирается

| Файл | Что это |
| --- | --- |
| `rigcontrol-gui.exe` | Окно программы. Пока оно открыто, устройства ведёт оно. |
| `rigcontrol-service.exe` | Фоновая служба со значком в трее: ведёт устройства, пока окно закрыто. |
| `rigcontrol.exe` | Консольная утилита (`rigcontrol help`). |
| `sensors\rigcontrol-sensors.exe` | Помощник датчиков на .NET 8 (LibreHardwareMonitor): температуры и вентиляторы. |

## Сборка

Нужны [Rust](https://rustup.rs) и .NET 8 SDK (для помощника датчиков).

```
build.bat
```

`build.bat` запускает `cargo test --features gui`, затем `cargo build --release --features gui` и
`dotnet publish` помощника датчиков. Журнал пишется в `build-log.txt`. Готовые файлы — в `target\release`.

Только консольная утилита:

```
cargo build --release
```

## Установка

```
install.bat
```

Запускать от имени администратора, после `build.bat`. Программа копируется в
`C:\Program Files\RigControl-rs`, ярлык появляется в меню «Пуск». Автозапуск включается из
установленной копии: «Настройки» → «Запускать вместе с Windows».

Права администратора нужны для датчиков и вентиляторов (LibreHardwareMonitor, драйвер PawnIO).

## Консольная утилита

Перед запуском освободите устройства — их держат окно RigControl, его фоновая служба и официальные
программы устройств. Закройте окно и службу: значок RigControl в трее → «Выход» или
`rigcontrol-service.exe --quit` (при включённой «работе в фоне» закрытие окна службу не останавливает).

```
rigcontrol list
rigcontrol info
rigcontrol effect 8 --rainbow
rigcontrol effect 7 --color #FF0050 --speed 2
rigcontrol paint Сакура
rigcontrol time
rigcontrol off
rigcontrol pump info
rigcontrol pump image C:\путь\картинка.gif --seconds 60
rigcontrol mouse info
rigcontrol daemon
```

## Проверка

```
cargo test --features gui
```

Тесты не требуют устройств. Обмен с железом ими не проверяется.

## Документы

- `GUI_PORT_STATUS.md` — история порта по версиям и что где лежит.
- `PLAN.md`, `ANALYZE.md` — план порта (rust-anything).
- `cleanup.ps1` — чистка рабочей папки от того, что пересобирается.
