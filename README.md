# rigcontrol (Rust)

Порт RigControl на Rust. Пока — консольная утилита для AJAZZ AK820 Pro (только по кабелю USB-C).

## Сборка
```
cargo build --release
```
Готовый файл: `target\release\rigcontrol.exe`.

## Запуск
Перед запуском закройте RigControl (C#) и официальную программу AJAZZ — они держат клавиатуру.

```
cargo run --release -- list
cargo run --release -- info
cargo run --release -- effect 8 --rainbow
cargo run --release -- effect 7 --color #FF0050 --speed 2
cargo run --release -- effect 106 --seconds 30
cargo run --release -- paint Сакура
cargo run --release -- time
cargo run --release -- off

# экран помпы Thermalright (закройте TRCC)
cargo run --release -- pump info
cargo run --release -- pump color #FF0050
cargo run --release -- pump image C:\путь\картинка.gif --seconds 60
```
