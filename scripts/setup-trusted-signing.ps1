$ErrorActionPreference = "Stop"

Write-Host "Microsoft Trusted Signing setup for WaveTrace"
Write-Host ""

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "Rust/cargo is required."
}

Write-Host "Installing trusted-signing-cli..."
cargo install trusted-signing-cli

Write-Host ""
Write-Host "Checking signtool (Windows SDK)..."
$signtool = Get-ChildItem "C:\Program Files (x86)\Windows Kits\10\bin" -Recurse -Filter signtool.exe -ErrorAction SilentlyContinue |
    Sort-Object FullName -Descending |
    Select-Object -First 1
if ($signtool) {
    Write-Host "  OK: $($signtool.FullName)"
} else {
    Write-Host "  MISSING: install Windows 11 SDK (10.0.26100+ recommended)"
    Write-Host "  winget install Microsoft.WindowsSDK.10.0.26100"
}

Write-Host ""
Write-Host "Checking Azure CLI..."
if (Get-Command az -ErrorAction SilentlyContinue) {
    Write-Host "  OK: az found"
} else {
    Write-Host "  OPTIONAL: winget install Microsoft.AzureCLI"
}

Write-Host ""
Write-Host "Next steps:"
Write-Host "  1. Create Azure Artifact Signing account + identity validation + certificate profile"
Write-Host "     https://learn.microsoft.com/en-us/azure/trusted-signing/quickstart"
Write-Host "  2. Create App Registration with client secret; grant it signing roles on the account"
Write-Host "  3. Copy .env.signing.example -> .env.signing and fill values"
Write-Host "  4. Build signed release: powershell -File scripts/build-signed.ps1"
