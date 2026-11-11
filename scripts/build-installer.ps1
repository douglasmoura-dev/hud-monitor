# Gera target\installer\hud-monitor-setup-<versão>.exe (precisa do Inno Setup 6).
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$version = (Select-String -Path "$root\crates\hud-app\Cargo.toml" -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$iscc = @("$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe", "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe", "$env:ProgramFiles\Inno Setup 6\ISCC.exe") |
    Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) { throw 'Inno Setup 6 não encontrado: winget install JRSoftware.InnoSetup' }
$cargo = (Get-Command cargo -ErrorAction SilentlyContinue).Source
if (-not $cargo) { $cargo = "$env:USERPROFILE\.cargo\bin\cargo.exe" }
& $cargo build --release -p hud-app -j 2
if ($LASTEXITCODE) { throw 'cargo build falhou' }
& $iscc "/DAppVersion=$version" "$root\installer\hud-monitor.iss"
if ($LASTEXITCODE) { throw 'ISCC falhou' }
