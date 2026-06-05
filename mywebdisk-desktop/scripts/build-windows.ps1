# Windows 本机打包：单文件 MyWebDisk.exe（静态链接 webrpc SDK）
$ErrorActionPreference = "Stop"
$Desktop = Split-Path -Parent $PSScriptRoot
$Dist = Join-Path $Desktop "dist\windows"

New-Item -ItemType Directory -Force -Path $Dist | Out-Null

Write-Host "==> build mywebdisk-desktop (release)"
Push-Location $Desktop
cargo build --release
Pop-Location

$App = Join-Path $Desktop "target\release\mywebdisk-desktop.exe"
if (-not (Test-Path $App)) { throw "binary not found: $App" }

Copy-Item -Force $App (Join-Path $Dist "MyWebDisk.exe")

Write-Host "==> done"
Write-Host "  $Dist\MyWebDisk.exe"
