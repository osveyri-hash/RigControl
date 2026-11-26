@echo off
chcp 65001 >nul
rem Ставит собранную программу в C:\Program Files\RigControl-rs (оттуда её безопасно запускать при входе в Windows):
rem rigcontrol-gui.exe (окно), rigcontrol-service.exe (фоновая служба со значком в трее), rigcontrol.exe (консоль)
rem и помощник датчиков sensors\. Настройки остаются в %AppData%\RigControl.
rem Запуск: правой кнопкой → «Запуск от имени администратора» (или из консоли администратора).
cd /d "%~dp0"
net session >nul 2>nul || (echo Нужны права администратора: запустите install.bat от имени администратора. & pause & exit /b 1)
if not exist target\release\rigcontrol-gui.exe (echo Сначала соберите: build.bat & pause & exit /b 1)
if not exist target\release\rigcontrol-service.exe (echo Сначала соберите: build.bat & pause & exit /b 1)
if not exist target\release\sensors\rigcontrol-sensors.exe (echo Нет помощника датчиков: соберите build.bat с .NET 8 SDK & pause & exit /b 1)

set "DEST=%ProgramFiles%\RigControl-rs"
tasklist /FI "IMAGENAME eq rigcontrol-gui.exe" 2>nul | find /I "rigcontrol-gui.exe" >nul && (
  echo Закройте окно RigControl и запустите install.bat снова.
  pause & exit /b 1
)

rem Фоновую службу останавливаем сами и после установки запускаем снова.
set "RESTART="
tasklist /FI "IMAGENAME eq rigcontrol-service.exe" 2>nul | find /I "rigcontrol-service.exe" >nul && (
  set "RESTART=1"
  echo Останавливаю фоновую службу…
  target\release\rigcontrol-service.exe --quit
)
for /l %%i in (1,1,20) do (
  tasklist /FI "IMAGENAME eq rigcontrol-sensors.exe" 2>nul | find /I "rigcontrol-sensors.exe" >nul || goto :copy
  timeout /t 1 /nobreak >nul
)
echo Ещё работает помощник датчиков: закройте RigControl и подождите пару секунд.
pause & exit /b 1

:copy
if not exist "%DEST%" mkdir "%DEST%"
copy /y target\release\rigcontrol-gui.exe "%DEST%\" >nul || (echo Не скопирован rigcontrol-gui.exe & pause & exit /b 1)
copy /y target\release\rigcontrol-service.exe "%DEST%\" >nul || (echo Не скопирован rigcontrol-service.exe & pause & exit /b 1)
copy /y target\release\rigcontrol.exe "%DEST%\" >nul || (echo Не скопирован rigcontrol.exe & pause & exit /b 1)
robocopy target\release\sensors "%DEST%\sensors" /MIR /NFL /NDL /NJH /NJS /NP >nul
if errorlevel 8 (echo Не скопирована папка sensors & pause & exit /b 1)

powershell -NoProfile -Command "$s=(New-Object -ComObject WScript.Shell).CreateShortcut([Environment]::GetFolderPath('CommonPrograms')+'\RigControl (Rust).lnk'); $s.TargetPath='%DEST%\rigcontrol-gui.exe'; $s.WorkingDirectory='%DEST%'; $s.IconLocation='%DEST%\rigcontrol-gui.exe,0'; $s.Save()"

rem Windows кэширует значки ярлыков: просим обновить кэш, чтобы новый значок появился сразу.
ie4uinit.exe -show >nul 2>nul
if defined RESTART start "" "%DEST%\rigcontrol-service.exe"
echo Готово: %DEST%
echo Ярлык «RigControl (Rust)» — в меню «Пуск».
echo Дальше: запустите оттуда RigControl, «Настройки» → «Запускать вместе с Windows».
pause
