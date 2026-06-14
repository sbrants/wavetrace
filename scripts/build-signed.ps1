$ErrorActionPreference = "Stop"

$repoRoot = Split-Path $PSScriptRoot -Parent
$envFile = Join-Path $repoRoot ".env.signing"

if (Test-Path $envFile) {
    Get-Content $envFile | ForEach-Object {
        if ($_ -match '^\s*([^#][^=]+)=(.*)$') {
            $name = $matches[1].Trim()
            $value = $matches[2].Trim().Trim('"')
            [Environment]::SetEnvironmentVariable($name, $value, "Process")
        }
    }
}

$required = @(
    "AZURE_CLIENT_ID",
    "AZURE_CLIENT_SECRET",
    "AZURE_TENANT_ID",
    "AZURE_TRUSTED_SIGNING_ENDPOINT",
    "AZURE_TRUSTED_SIGNING_ACCOUNT_NAME",
    "AZURE_CERTIFICATE_PROFILE_NAME"
)

foreach ($name in $required) {
    if ([string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($name))) {
        throw "Missing $name. Copy .env.signing.example to .env.signing and fill in Azure values."
    }
}

$updaterKey = [Environment]::GetEnvironmentVariable("TAURI_SIGNING_PRIVATE_KEY")
$keyPath = [Environment]::GetEnvironmentVariable("TAURI_SIGNING_PRIVATE_KEY_PATH")
if ([string]::IsNullOrWhiteSpace($updaterKey) -and -not [string]::IsNullOrWhiteSpace($keyPath)) {
    if (-not (Test-Path $keyPath)) {
        throw "TAURI_SIGNING_PRIVATE_KEY_PATH not found: $keyPath"
    }
    [Environment]::SetEnvironmentVariable(
        "TAURI_SIGNING_PRIVATE_KEY",
        (Get-Content $keyPath -Raw),
        "Process"
    )
}

if (-not (Get-Command trusted-signing-cli -ErrorAction SilentlyContinue)) {
    throw "trusted-signing-cli not found. Run: powershell -File scripts/setup-trusted-signing.ps1"
}

Push-Location $repoRoot
try {
    npm run tauri build -- --config tauri.signed.windows.conf.json
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
finally {
    Pop-Location
}
