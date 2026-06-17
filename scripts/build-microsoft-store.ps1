param(
    [switch]$LocalTest
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path $PSScriptRoot -Parent
$storeRoot = Join-Path $repoRoot "microsoft-store"
$staging = Join-Path $storeRoot "staging"
$outDir = Join-Path $storeRoot "out"
$manifestSource = Join-Path $storeRoot "Package.appxmanifest"
$iconSource = Join-Path $repoRoot "src-tauri\icons\128x128.png"
$exeSource = Join-Path $repoRoot "src-tauri\target\release\wavetrace.exe"
$resourcesSource = Join-Path $repoRoot "src-tauri\target\release\resources"

function Find-MakeAppx {
    $kits = "C:\Program Files (x86)\Windows Kits\10\bin"
    if (-not (Test-Path $kits)) {
        return $null
    }
    Get-ChildItem $kits -Recurse -Filter "makeappx.exe" -ErrorAction SilentlyContinue |
        Sort-Object FullName -Descending |
        Select-Object -First 1
}

function Get-TauriVersion {
    $conf = Get-Content (Join-Path $repoRoot "src-tauri\tauri.conf.json") -Raw | ConvertFrom-Json
    $parts = $conf.version.Split(".")
    while ($parts.Count -lt 4) { $parts += "0" }
    return ($parts[0..3] -join ".")
}

function Sync-StoreAssets([string]$assetsDir) {
    if (-not (Test-Path $iconSource)) {
        throw "Missing icon: $iconSource (run: npm run tauri icon <source.png>)"
    }
    New-Item -ItemType Directory -Force -Path $assetsDir | Out-Null
    $names = @(
        "StoreLogo.png",
        "Square44x44Logo.png",
        "Square71x71Logo.png",
        "Square150x150Logo.png",
        "Square310x310Logo.png",
        "Wide310x150Logo.png"
    )
    foreach ($name in $names) {
        Copy-Item $iconSource (Join-Path $assetsDir $name) -Force
    }
}

function Get-FrontendFingerprint {
    $distIndex = Join-Path $repoRoot "dist\index.html"
    if (-not (Test-Path $distIndex)) {
        throw "dist/index.html missing. Run: npm run build"
    }
    $indexHtml = Get-Content $distIndex -Raw
    if ($indexHtml -match 'src="\./assets/([^"]+\.js)"') {
        return $matches[1]
    }
    throw "Could not read frontend bundle name from dist/index.html"
}

function Verify-ReleaseExe([string]$exePath) {
    if (-not (Test-Path $exePath)) {
        throw "Release binary not found: $exePath"
    }
    $fingerprint = Get-FrontendFingerprint
    $bytes = [System.IO.File]::ReadAllBytes($exePath)
    $text = [System.Text.Encoding]::ASCII.GetString($bytes)
    if (-not $text.Contains("tauri.localhost")) {
        throw @"
Store build verification failed: wavetrace.exe does not embed production assets (tauri.localhost missing).
Ensure dist/ exists and run: npm run build && npm run tauri build -- --no-bundle --config src-tauri/tauri.microsoftstore.conf.json
"@
    }
    if (-not $text.Contains($fingerprint)) {
        throw @"
Store build verification failed: wavetrace.exe does not embed the current dist/ build ($fingerprint missing).
Run a clean release build: npm run tauri:store:build
Do not package a debug build or an exe built before npm run build.
"@
    }
    Write-Host "Verified release exe embeds production frontend ($fingerprint, tauri.localhost)."
}

function Copy-StoreRuntimeFiles([string]$stagingDir, [string]$exePath, [string]$resourcesDir) {
    Copy-Item $exePath (Join-Path $stagingDir "wavetrace.exe") -Force
    if (Test-Path $resourcesDir) {
        Copy-Item $resourcesDir (Join-Path $stagingDir "resources") -Recurse -Force
        Write-Host "Copied resources/ alongside wavetrace.exe"
    }
}
function Update-ManifestVersion([string]$manifestPath, [string]$msixVersion) {
    $xml = [System.IO.File]::ReadAllText($manifestPath)
    if ($xml -match '<Identity\b[\s\S]*?\bVersion="([^"]+)"' -and $matches[1] -eq $msixVersion) {
        return
    }
    $updated = $xml -replace '(<Identity\b[\s\S]*?\bVersion=")[^"]+(")', "`${1}$msixVersion`$2"
    if ($updated -eq $xml) {
        throw "Failed to update Identity Version in $manifestPath"
    }
    $utf8NoBom = New-Object System.Text.UTF8Encoding $false
    [System.IO.File]::WriteAllText($manifestPath, $updated, $utf8NoBom)
}

if (-not (Get-Command npm -ErrorAction SilentlyContinue)) {
    throw "npm is required."
}

$msixVersion = Get-TauriVersion
Write-Host "Building WaveTrace for Microsoft Store (MSIX $msixVersion)..."

$env:VITE_STORE_DISTRIBUTION = "true"
$env:CARGO_TARGET_DIR = Join-Path $repoRoot "src-tauri\target"
Push-Location $repoRoot
try {
    npm run tauri build -- --no-bundle --config src-tauri/tauri.microsoftstore.conf.json
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
finally {
    Pop-Location
    Remove-Item Env:VITE_STORE_DISTRIBUTION -ErrorAction SilentlyContinue
    Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue
}

if (-not (Test-Path $exeSource)) {
    throw "Release binary not found: $exeSource"
}
Verify-ReleaseExe $exeSource

if (Test-Path $staging) {
    Remove-Item $staging -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $staging | Out-Null
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

Copy-StoreRuntimeFiles $staging $exeSource $resourcesSource
Sync-StoreAssets (Join-Path $staging "Assets")

$manifestStaging = Join-Path $staging "AppxManifest.xml"
Copy-Item $manifestSource $manifestStaging -Force
Update-ManifestVersion $manifestStaging $msixVersion

$msixName = "Meringue.WaveTrace_${msixVersion}_x64.msix"
$msixPath = Join-Path $outDir $msixName

if (Get-Command winapp -ErrorAction SilentlyContinue) {
    Write-Host "Packaging with winapp..."
    if ($LocalTest) {
        $cert = Join-Path $storeRoot "devcert.pfx"
        if (-not (Test-Path $cert)) {
            Push-Location $storeRoot
            try {
                winapp cert generate --if-exists skip
            }
            finally {
                Pop-Location
            }
        }
        winapp pack $staging --cert $cert --output $msixPath
    }
    else {
        winapp pack $staging --output $msixPath
    }
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
else {
    $makeappx = Find-MakeAppx
    if (-not $makeappx) {
        throw "Install Windows SDK (makeappx.exe) or winapp CLI: winget install Microsoft.winappcli"
    }
    Write-Host "Packaging with $($makeappx.FullName)..."
    if (Test-Path $msixPath) {
        Remove-Item $msixPath -Force
    }
    & $makeappx.FullName pack /d $staging /p $msixPath /o
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

Write-Host ""
Write-Host "MSIX ready: $msixPath"
Write-Host "Partner Center -> Packages -> upload this file (unsigned is OK; Microsoft re-signs after certification)."
if ($LocalTest) {
    Write-Host "Local test: winapp cert install $storeRoot\devcert.pfx  (admin), then double-click the MSIX."
    Write-Host "Confirm the app shows Dashboard/Settings - not 'localhost refused to connect'."
}
