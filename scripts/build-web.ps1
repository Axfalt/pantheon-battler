# Construit le moteur WebAssembly de la page web/, l'intègre en base64 dans
# web/engine-wasm.js, puis vérifie que la page et la CLI jouent exactement les mêmes combats.
# Usage : pwsh scripts/build-web.ps1
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')

function Invoke-Step([string]$label, [scriptblock]$step) {
    Write-Host "== $label"
    & $step
    if ($LASTEXITCODE -ne 0) { throw "échec : $label" }
}

if (-not (rustup target list --installed | Where-Object { $_ -eq 'wasm32-unknown-unknown' })) {
    Write-Error "cible WebAssembly absente. Installez-la avec : rustup target add wasm32-unknown-unknown"
    exit 1
}

Invoke-Step 'Moteur WebAssembly' { cargo build -p pantheon-wasm --target wasm32-unknown-unknown --profile wasm }
Copy-Item target/wasm32-unknown-unknown/wasm/pantheon_wasm.wasm web/engine.wasm -Force

Invoke-Step 'CLI (pour la vérification)' { cargo build --release }

if (-not $env:PANTHEON_GIT) {
    $git = git describe --always --dirty 2>$null
    $env:PANTHEON_GIT = if ($LASTEXITCODE -eq 0 -and $git) { $git } else { 'non-versionné' }
}
Invoke-Step 'Intégration dans web/engine-wasm.js' { node scripts/embed-wasm.mjs }
Invoke-Step 'Vérification page = CLI' { node scripts/verify-web.mjs }
