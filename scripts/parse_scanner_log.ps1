$log = Join-Path $env:APPDATA "towerrun\logs\scanner.log"
if (-not (Test-Path $log)) { Write-Error "Missing $log"; exit 1 }

$old = [System.Collections.Generic.List[int]]::new()
$new = [System.Collections.Generic.List[object]]::new()

Get-Content $log | ForEach-Object {
    if ($_ -match 'ocr_ms=(\d+) tier=' -and $_ -notmatch 'capture_ms=') {
        [void]$old.Add([int]$matches[1])
    } elseif ($_ -match 'capture_ms=(\d+) match_ms=(\d+) ocr_ms=(\d+) regions=(\w+)') {
        [void]$new.Add([pscustomobject]@{
            capture = [int]$matches[1]
            match   = [int]$matches[2]
            ocr     = [int]$matches[3]
            regions = $matches[4]
            total   = [int]$matches[1] + [int]$matches[2] + [int]$matches[3]
        })
    }
}

function Stats($values, $label) {
    if ($values.Count -eq 0) { Write-Host "$label : no data"; return }
    $sorted = $values | Sort-Object
    $p50 = $sorted[[int]($sorted.Count * 0.5)]
    $p95 = $sorted[[int]([math]::Min($sorted.Count - 1, [math]::Floor($sorted.Count * 0.95)))]
    $avg = [math]::Round(($values | Measure-Object -Average).Average, 1)
    $min = ($values | Measure-Object -Minimum).Minimum
    $max = ($values | Measure-Object -Maximum).Maximum
    Write-Host "$label count=$($values.Count) min=$min max=$max avg=$avg p50=$p50 p95=$p95"
}

Write-Host "=== OLD full-frame format ==="
Stats $old "ocr_ms"

Write-Host ""
Write-Host "=== NEW region format ==="
Stats ($new | ForEach-Object { $_.capture }) "capture_ms"
Stats ($new | ForEach-Object { $_.match }) "match_ms"
Stats ($new | ForEach-Object { $_.ocr }) "ocr_ms"
Stats ($new | ForEach-Object { $_.total }) "total_ms"

$under500 = ($new | Where-Object { $_.total -lt 500 }).Count
$under1000 = ($new | Where-Object { $_.total -lt 1000 }).Count
$cached = ($new | Where-Object { $_.match -eq 0 }).Count
Write-Host "under500=$under500/$($new.Count) under1000=$under1000/$($new.Count) match_cached=$cached/$($new.Count)"
Write-Host "regions_true=$(($new | Where-Object { $_.regions -eq 'true' }).Count) regions_false=$(($new | Where-Object { $_.regions -eq 'false' }).Count)"

# Inter-poll intervals for new format (last 50)
$tsLines = Get-Content $log | Select-String 'capture_ms=' | Select-Object -Last 51
if ($tsLines.Count -ge 2) {
    $intervals = [System.Collections.Generic.List[int]]::new()
    for ($i = 1; $i -lt $tsLines.Count; $i++) {
        $t1 = [datetimeoffset]::Parse(($tsLines[$i-1].Line -split ' ')[0])
        $t2 = [datetimeoffset]::Parse(($tsLines[$i].Line -split ' ')[0])
        [void]$intervals.Add([int]($t2 - $t1).TotalMilliseconds)
    }
    Stats $intervals "poll_interval_actual_ms (last $($intervals.Count))"
}
