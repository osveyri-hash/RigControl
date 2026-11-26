# Чистка папки rust123: удаляет то, что cargo и dotnet пересоберут сами, и ужимает большие бэкапы.
# Ничего нужного не трогает: исходники, assets, settings.json и установленная программа
# (C:\Program Files\RigControl-rs) остаются.
# rigcontrol-rs\target по умолчанию ОСТАВЛЯЕТ — с ней новые версии собираются быстро.
#
# Запуск (PowerShell):
#   powershell -ExecutionPolicy Bypass -File .\cleanup.ps1
#   powershell -ExecutionPolicy Bypass -File .\cleanup.ps1 -Downloads   # ещё и старые архивы rigcontrol-*.zip в «Загрузках»
#   powershell -ExecutionPolicy Bypass -File .\cleanup.ps1 -All         # и rigcontrol-rs\target (следующая сборка — с нуля)
param(
    [string]$Root = "$env:USERPROFILE\Desktop\rust123",
    [switch]$Downloads,
    [switch]$All
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem

function Size([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path)) { return 0 }
    $sum = (Get-ChildItem -LiteralPath $Path -Recurse -Force -File -ErrorAction SilentlyContinue | Measure-Object Length -Sum).Sum
    if ($sum) { return [int64]$sum } else { return 0 }
}
function Gb([int64]$Bytes) { '{0:N2} ГБ' -f ($Bytes / 1GB) }
function Mb([int64]$Bytes) { '{0:N0} МБ' -f ($Bytes / 1MB) }

if (-not (Test-Path -LiteralPath $Root)) { Write-Host "Нет папки $Root" -ForegroundColor Red; exit 1 }
if (Get-Process cargo, rustc -ErrorAction SilentlyContinue) {
    Write-Host 'Сейчас идёт сборка (cargo/rustc) — дождитесь конца build.bat и запустите снова.' -ForegroundColor Red
    exit 1
}

$before = Size $Root
Write-Host "Папка $Root : $(Gb $before)"
Write-Host ''

# 1. Сборка Rust: папки target рядом с Cargo.toml (rigcontrol-rs, rigcontrol-verify\…).
# 2. Сборка .NET: bin, obj и .vs рядом с *.csproj (помощник датчиков, проверка, исходник C#).
$junk = @()
foreach ($d in Get-ChildItem -LiteralPath $Root -Directory -Recurse -Force -ErrorAction SilentlyContinue) {
    $parent = $d.Parent.FullName
    if ($d.Name -eq 'target' -and (Test-Path -LiteralPath (Join-Path $parent 'Cargo.toml'))) { $junk += $d.FullName }
    elseif (($d.Name -eq 'bin' -or $d.Name -eq 'obj') -and (Get-ChildItem -LiteralPath $parent -Filter *.csproj -File -ErrorAction SilentlyContinue)) { $junk += $d.FullName }
    elseif ($d.Name -eq '.vs') { $junk += $d.FullName }
}
# Сборку основного проекта оставляем, если не просили -All: без неё каждая версия собирается заново.
$keep = Join-Path $Root 'rigcontrol-rs\target'
if (-not $All) {
    $junk = @($junk | Where-Object { $_ -ne $keep -and -not $_.StartsWith($keep + '\') })
    if (Test-Path -LiteralPath $keep) { Write-Host ("Оставляю rigcontrol-rs\target ({0}) — с ней сборка быстрая. Удалить: -All" -f (Mb (Size $keep))) }
}
# Вложенные (target внутри уже найденного target) не считаем дважды.
$junk = $junk | Sort-Object Length | Where-Object { $p = $_; -not ($junk | Where-Object { $_ -ne $p -and $p.StartsWith($_ + '\') }) }

foreach ($j in $junk) {
    $s = Size $j
    Write-Host ("Удаляю {0}  ({1})" -f $j.Substring($Root.Length + 1), (Mb $s))
    Remove-Item -LiteralPath $j -Recurse -Force -ErrorAction SilentlyContinue
    if (Test-Path -LiteralPath $j) { Write-Host '   не всё удалилось — какой-то файл занят (закройте RigControl и Проводник в этой папке)' -ForegroundColor Yellow }
}

# 3. Бэкапы больше 20 МБ — это бэкапы вместе с target. Пересобираем их без target/bin/obj: код остаётся.
$skip = '(^|[\\/])(target|bin|obj|\.vs)[\\/]'
foreach ($zip in Get-ChildItem -LiteralPath $Root -Filter 'rigcontrol-rs-backup-*.zip' -File) {
    if ($zip.Length -lt 20MB) { continue }
    $tmp = $zip.FullName + '.tmp'
    Write-Host ("Ужимаю бэкап {0}  ({1})…" -f $zip.Name, (Mb $zip.Length))
    $src = [System.IO.Compression.ZipFile]::OpenRead($zip.FullName)
    try {
        $dst = [System.IO.Compression.ZipFile]::Open($tmp, [System.IO.Compression.ZipArchiveMode]::Create)
        try {
            foreach ($e in $src.Entries) {
                if ($e.FullName -match $skip) { continue }
                $n = $dst.CreateEntry($e.FullName, [System.IO.Compression.CompressionLevel]::Optimal)
                $n.LastWriteTime = $e.LastWriteTime
                if ($e.Length -gt 0 -or -not $e.FullName.EndsWith('/')) {
                    $in = $e.Open(); $o = $n.Open()
                    try { $in.CopyTo($o) } finally { $o.Dispose(); $in.Dispose() }
                }
            }
        } finally { $dst.Dispose() }
    } finally { $src.Dispose() }
    Move-Item -LiteralPath $tmp -Destination $zip.FullName -Force
    Write-Host ("   стало {0}" -f (Mb (Get-Item -LiteralPath $zip.FullName).Length))
}

# 4. По желанию: старые архивы из «Загрузок» (оставляем самый свежий).
if ($Downloads) {
    $dl = Join-Path $env:USERPROFILE 'Downloads'
    $old = Get-ChildItem -LiteralPath $dl -Filter 'rigcontrol-*.zip' -File | Sort-Object LastWriteTime -Descending | Select-Object -Skip 1
    $s = ($old | Measure-Object Length -Sum).Sum
    if ($old) {
        Write-Host ("Удаляю из «Загрузок» старые архивы: {0} шт. ({1})" -f @($old).Count, (Mb $s))
        $old | Remove-Item -Force
    }
}

$after = Size $Root
Write-Host ''
Write-Host ("Готово: было {0}, стало {1}, освобождено {2}." -f (Gb $before), (Gb $after), (Gb ($before - $after))) -ForegroundColor Green
Write-Host 'Программа в C:\Program Files\RigControl-rs работает как раньше.'
