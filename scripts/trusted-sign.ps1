param(
    [Parameter(Mandatory = $true, Position = 0)]
    [string]$File
)

$ErrorActionPreference = "Stop"

function Require-Env([string]$Name) {
    $value = [Environment]::GetEnvironmentVariable($Name)
    if ([string]::IsNullOrWhiteSpace($value)) {
        throw "Missing environment variable: $Name (see .env.signing.example)"
    }
    return $value
}

$cli = Get-Command trusted-signing-cli -ErrorAction SilentlyContinue
if (-not $cli) {
    throw "trusted-signing-cli not found. Install: cargo install trusted-signing-cli"
}

$endpoint = Require-Env "AZURE_TRUSTED_SIGNING_ENDPOINT"
$account = Require-Env "AZURE_TRUSTED_SIGNING_ACCOUNT_NAME"
$profile = Require-Env "AZURE_CERTIFICATE_PROFILE_NAME"
Require-Env "AZURE_CLIENT_ID" | Out-Null
Require-Env "AZURE_CLIENT_SECRET" | Out-Null
Require-Env "AZURE_TENANT_ID" | Out-Null

Write-Host "Signing $File via Microsoft Trusted Signing ($account / $profile)..."

& trusted-signing-cli `
    -e $endpoint `
    -a $account `
    -c $profile `
    -d "TowerRun Performance Tracker" `
    $File

if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}
