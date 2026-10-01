# Dağıtım dosyasını hazırlar: sürüm derlemesi → dist\CouchRemote.exe
# Kullanım: powershell -ExecutionPolicy Bypass -File tools\release.ps1
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"

Push-Location (Join-Path $root 'agent')
try {
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw 'Derleme başarısız' }
} finally {
    Pop-Location
}

$dist = Join-Path $root 'dist'
New-Item -ItemType Directory -Force $dist | Out-Null
$out = Join-Path $dist 'CouchRemote.exe'
Copy-Item (Join-Path $root 'agent\target\release\couch-agent.exe') $out -Force
$hash = (Get-FileHash $out -Algorithm SHA256).Hash
"{0}  ({1:N1} MB)`nSHA-256: {2}" -f $out, ((Get-Item $out).Length / 1MB), $hash
