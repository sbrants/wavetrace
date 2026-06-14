$ErrorActionPreference = "Stop"

$keyPath = Join-Path $env:USERPROFILE ".tauri\wavetrace.key"

Write-Host "Generating Tauri updater signing keypair at:"
Write-Host "  $keyPath"
Write-Host ""

$env:CI = "true"
Push-Location (Split-Path $PSScriptRoot -Parent)
try {
    npm run tauri signer generate -- -w $keyPath --ci --force
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
finally {
    Pop-Location
}

Write-Host ""
Write-Host "Next steps:"
Write-Host "  1. Add GitHub repo secret TAURI_SIGNING_PRIVATE_KEY = contents of $keyPath"
Write-Host "  2. Optional local builds: set TAURI_SIGNING_PRIVATE_KEY_PATH in .env.signing"
Write-Host "  3. Public key is already in src-tauri/tauri.conf.json (safe to commit)"
Write-Host ""
Write-Host "Keep the .key file secret. If you lose it, existing installs cannot verify future updates."
