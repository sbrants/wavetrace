$ErrorActionPreference = "Stop"

$gh = "C:\Program Files\GitHub CLI\gh.exe"
if (-not (Test-Path $gh)) {
    $gh = (Get-Command gh -ErrorAction SilentlyContinue)?.Source
}
if (-not $gh) {
    throw "GitHub CLI (gh) not found. Install from https://cli.github.com/"
}

$keyPath = Join-Path $env:USERPROFILE ".tauri\wavetrace.key"
if (-not (Test-Path $keyPath)) {
    throw "Missing $keyPath. Run: powershell -File scripts/setup-updater-signing.ps1"
}

& $gh auth status *> $null
if ($LASTEXITCODE -ne 0) {
    throw "Not logged into GitHub. Run: gh auth login"
}

$key = Get-Content $keyPath -Raw
$key | & $gh secret set TAURI_SIGNING_PRIVATE_KEY --repo sbrants/thetower-perftracker

Write-Host "Set GitHub secret TAURI_SIGNING_PRIVATE_KEY for sbrants/thetower-perftracker"
Write-Host "Re-run the Release workflow or push a new v* tag."
