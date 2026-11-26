@echo off
chcp 65001 >nul
rem Сборка и проверка rigcontrol-rs. Запускать из папки проекта.
rem Собирает окно (rigcontrol-gui.exe), фоновую службу (rigcontrol-service.exe), консоль (rigcontrol.exe)
rem и помощника датчиков. Установленную программу (install.bat) сборка не трогает.
cd /d "%~dp0"
where cargo >nul 2>nul || (echo Нет Rust. Установите: https://rustup.rs & pause & exit /b 1)
echo === cargo test --features gui ===
cargo test --features gui > build-log.txt 2>&1
if errorlevel 1 (echo Тесты не прошли — пришлите build-log.txt & pause & exit /b 1)
echo === cargo build --release --features gui ===
cargo build --release --features gui >> build-log.txt 2>&1
if errorlevel 1 (echo Ошибки сборки — пришлите build-log.txt & pause & exit /b 1)
where dotnet >nul 2>nul
if errorlevel 1 (
  echo Нет .NET 8 SDK: помощник датчиков не собран, температуры CPU и вентиляторы работать не будут.
) else (
  echo === dotnet publish sensors ===
  dotnet publish sensors\RigControl.Sensors.csproj -c Release -o target\release\sensors --nologo >> build-log.txt 2>&1
  if errorlevel 1 (echo Ошибка сборки помощника датчиков — пришлите build-log.txt & pause & exit /b 1)
)
echo Готово: target\release\rigcontrol-gui.exe, rigcontrol-service.exe, rigcontrol.exe и sensors\rigcontrol-sensors.exe
target\release\rigcontrol.exe list
pause
