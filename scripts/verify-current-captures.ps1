# Read-only verification of checked-in hosted screenshots; never launches the editor.
[CmdletBinding()]
param([string]$RepositoryRoot = (Split-Path -Parent $PSScriptRoot))
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$RepositoryRoot = (Resolve-Path -LiteralPath $RepositoryRoot).Path
$directory = Join-Path $RepositoryRoot 'assets/screenshots/current'
$manifest = Get-Content -LiteralPath (Join-Path $directory 'capture-manifest.json') -Raw | ConvertFrom-Json -AsHashtable
if ($manifest.schema -ne 1 -or $manifest.status -ne 'success') { throw 'A successful schema-1 capture manifest is required.' }
if ($manifest.source_commit -notmatch '^[0-9a-f]{40}$' -or $manifest.executable_sha256 -notmatch '^[0-9a-f]{64}$' -or $manifest.helper_sha256 -notmatch '^[0-9a-f]{64}$') { throw 'Invalid capture source or file hash.' }
if ($manifest.github.repository -ne 'Gerbzz/PDFStrike' -or $manifest.github.run_id -notmatch '^\d+$' -or $manifest.github.run_attempt -notmatch '^[1-9]\d*$') { throw 'Invalid hosted-run provenance.' }
foreach ($tree in @(@('native_source_tree', 'native/pdfstrike'), @('fixtures_tree', 'fixtures/pdfstrike'), @('cli_tree', 'apps/pdfstrike'))) {
    $actual = (& git -C $RepositoryRoot rev-parse ('HEAD:' + $tree[1])).Trim()
    if ($LASTEXITCODE -ne 0 -or $manifest[$tree[0]] -ne $actual) { throw "Captured $($tree[1]) does not match this checkout." }
}
$helperRelative = 'scripts/capture-current-ui.ps1'
# The capture commit is already merged into main. Verify its immutable helper,
# rather than requiring today's helper to equal the one used for historical PNGs.
# The provenance workflow fetches full history; deleting a merged branch is safe.
$captureTree = (& git -C $RepositoryRoot rev-parse ($manifest.source_commit + '^{tree}')).Trim()
if ($LASTEXITCODE -ne 0 -or $captureTree -ne $manifest.source_tree) { throw 'Captured source tree differs from its recorded commit.' }
& git -C $RepositoryRoot merge-base --is-ancestor $manifest.source_commit HEAD
if ($LASTEXITCODE -ne 0) { throw 'Captured source must remain in the published history.' }
$helperLines = @(& git -C $RepositoryRoot show ($manifest.source_commit + ':' + $helperRelative))
if ($LASTEXITCODE -ne 0) { throw 'Cannot read the executed capture helper from its recorded source.' }
$helperText = ($helperLines -join "`n") + "`n"
# Windows Git checkouts can use CRLF; compare both exact UTF-8 variants.
$helperHashes = @($helperText, $helperText.Replace("`n", "`r`n")) | ForEach-Object {
    [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($_))).ToLowerInvariant()
}
if ($manifest.helper_sha256 -notin $helperHashes) { throw 'Executed helper hash differs from the captured source.' }
$expectedAssets = @(
    'fixtures/pdfstrike/native-compat.pdfstrike',
    'fixtures/pdfstrike/packet-real.pdf',
    'fixtures/pdfstrike/form-real.pdf',
    'native/pdfstrike/assets/app-icon/pdfstrike-geometric-256.png',
    'native/pdfstrike/assets/app-icon/pdfstrike-geometric.ico'
)
$assets = @($manifest.fixtures) + @($manifest.logo_assets)
if ($assets.Count -ne $expectedAssets.Count -or @($manifest.fixtures).Count -ne 3 -or @($manifest.logo_assets).Count -ne 2) { throw 'All three fixtures and both logo assets must be listed exactly once.' }
foreach ($relative in $expectedAssets) {
    $assetMatches = @($assets | Where-Object { $_.path -ceq $relative })
    if ($assetMatches.Count -ne 1 -or $assetMatches[0].sha256 -notmatch '^[0-9a-f]{64}$') { throw "Missing, repeated or invalid captured asset: $relative" }
    $hash = (Get-FileHash -LiteralPath (Join-Path $RepositoryRoot $relative) -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($hash -ne $assetMatches[0].sha256) { throw "Captured asset hash differs: $relative" }
}
if (-not $manifest.note_evidence.saved_note_verified) { throw 'The saved fictional note was not verified.' }
$names = @('pdfstrike-project-overview-current', 'pdfstrike-review-note-current', 'pdfstrike-form-fields-current')
if (@($manifest.shots).Count -ne $names.Count) { throw 'Exactly three verified views are required.' }
$readme = Get-Content -LiteralPath (Join-Path $RepositoryRoot 'README.md') -Raw
$provenance = Get-Content -LiteralPath (Join-Path $RepositoryRoot 'docs/SCREENSHOT_PROVENANCE.md') -Raw
$runUrl = 'https://github.com/Gerbzz/PDFStrike/actions/runs/' + $manifest.github.run_id
if (-not $provenance.Contains($manifest.source_commit) -or -not $provenance.Contains($runUrl)) { throw 'Screenshot provenance must link the actual captured source and hosted run.' }
for ($i = 0; $i -lt $names.Count; $i++) {
    $shot = $manifest.shots[$i]
    if ($shot.name -ne $names[$i]) { throw 'Unexpected capture ordering or view name.' }
    $null = [DateTimeOffset]::Parse($shot.capture_utc, [Globalization.CultureInfo]::InvariantCulture)
    foreach ($kind in @('window', 'client')) {
        $file = $shot[($kind + '_file')]
        if ($file -ne ($names[$i] + '-' + $kind + '.png') -or $shot[$kind].file -ne $file) { throw 'Unexpected screenshot filename.' }
        $path = Join-Path $directory $file
        $hash = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($hash -ne $shot[($kind + '_sha256')] -or $hash -ne $shot[$kind].sha256) { throw "Screenshot hash differs: $file" }
        $bytes = [IO.File]::ReadAllBytes($path)
        if ($bytes.Length -lt 33 -or [Convert]::ToHexString($bytes[0..7]) -ne '89504E470D0A1A0A' -or [Text.Encoding]::ASCII.GetString($bytes[12..15]) -ne 'IHDR') { throw "Invalid PNG header: $file" }
        $width = [int64]$bytes[16] * 16777216 + [int64]$bytes[17] * 65536 + [int64]$bytes[18] * 256 + $bytes[19]
        $height = [int64]$bytes[20] * 16777216 + [int64]$bytes[21] * 65536 + [int64]$bytes[22] * 256 + $bytes[23]
        if ($width -ne $shot[$kind].width -or $height -ne $shot[$kind].height -or $width -lt 800 -or $height -lt 500 -or $width -gt 8192 -or $height -gt 8192) { throw "Screenshot dimensions differ: $file" }
    }
    if (-not $readme.Contains(('assets/screenshots/current/' + $shot.window_file))) { throw "README does not reference the verified native window: $($shot.name)" }
}
Write-Output ('Verified six unchanged capture PNGs and three README views from source {0}, hosted run {1}; native source and fixtures match this checkout.' -f $manifest.source_commit, $manifest.github.run_id)
