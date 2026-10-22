# Capture the real native app on the ephemeral Windows GitHub runner. This script is
# intentionally unavailable as a local workaround for a machine's application policy.
[CmdletBinding()]
param(
    [string]$RepositoryRoot = (Split-Path -Parent $PSScriptRoot),
    [string]$Executable,
    [Parameter(Mandatory)][string]$OutputDirectory,
    [ValidatePattern('^[0-9a-f]{40}$')][string]$SourceCommit
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_OS -ne 'Windows' -or $env:GITHUB_REPOSITORY -ne 'Gerbzz/PDFStrike') {
    throw 'Native UI capture is restricted to the authorized PDFStrike Windows GitHub runner.'
}
$RepositoryRoot = (Resolve-Path -LiteralPath $RepositoryRoot).Path
if (-not $Executable) { $Executable = Join-Path $RepositoryRoot 'native/pdfstrike/target/debug/pdfstrike-native.exe' }
$Executable = (Resolve-Path -LiteralPath $Executable).Path
$actualCommit = (& git -C $RepositoryRoot rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0 -or $actualCommit -notmatch '^[0-9a-f]{40}$') { throw 'Cannot identify the checked-out source commit.' }
if ($SourceCommit -and $actualCommit -ne $SourceCommit) { throw 'The capture checkout does not match SourceCommit.' }
$SourceCommit = $actualCommit
& git -C $RepositoryRoot diff --quiet HEAD -- native/pdfstrike apps/pdfstrike vendor/pdfcraft Cargo.toml Cargo.lock
if ($LASTEXITCODE -ne 0) { throw 'Application source has uncommitted changes; refusing ambiguous build provenance.' }
if (Test-Path -LiteralPath $OutputDirectory) {
    if (@(Get-ChildItem -LiteralPath $OutputDirectory -Force).Count -ne 0) { throw 'OutputDirectory must be empty.' }
} else {
    $null = New-Item -ItemType Directory -Path $OutputDirectory
}
$OutputDirectory = (Resolve-Path -LiteralPath $OutputDirectory).Path
$stage = Join-Path $env:RUNNER_TEMP ('pdfstrike-ui-' + [guid]::NewGuid().ToString('N'))
$null = New-Item -ItemType Directory -Path $stage
$stagedExe = Join-Path $stage 'pdfstrike-native.exe'
Copy-Item -LiteralPath $Executable -Destination $stagedExe
[IO.File]::WriteAllText((Join-Path $stage 'portable.txt'), 'Isolated hosted screenshot profile.')

Add-Type -AssemblyName System.Drawing, System.Windows.Forms
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class PdfStrikeCaptureWindow {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int w, int height, uint flags);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int command);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
    [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int attribute, out RECT r, int size);
}
'@
$null = [PdfStrikeCaptureWindow]::SetProcessDPIAware()
$workArea = [Windows.Forms.Screen]::PrimaryScreen.WorkingArea
if ($workArea.Width -lt 840 -or $workArea.Height -lt 560) { throw 'Hosted desktop is too small for the native app.' }
$utf8 = [Text.UTF8Encoding]::new($false)
$process = $null
$endpoint = $null
$launches = [Collections.Generic.List[object]]::new()
$captures = [Collections.Generic.List[object]]::new()
$manifest = [ordered]@{
    schema = 1
    status = 'incomplete'
    source_commit = $SourceCommit
    source_tree = (& git -C $RepositoryRoot rev-parse ($SourceCommit + '^{tree}')).Trim()
    native_source_tree = (& git -C $RepositoryRoot rev-parse ($SourceCommit + ':native/pdfstrike')).Trim()
    fixtures_tree = (& git -C $RepositoryRoot rev-parse ($SourceCommit + ':fixtures/pdfstrike')).Trim()
    cli_tree = (& git -C $RepositoryRoot rev-parse ($SourceCommit + ':apps/pdfstrike')).Trim()
    helper_sha256 = (Get-FileHash -LiteralPath $PSCommandPath -Algorithm SHA256).Hash.ToLowerInvariant()
    executable_sha256 = (Get-FileHash -LiteralPath $Executable -Algorithm SHA256).Hash.ToLowerInvariant()
    github = [ordered]@{ repository = $env:GITHUB_REPOSITORY; run_id = $env:GITHUB_RUN_ID; run_attempt = $env:GITHUB_RUN_ATTEMPT; workflow = $env:GITHUB_WORKFLOW }
    os = [Runtime.InteropServices.RuntimeInformation]::OSDescription
    desktop_work_area = @($workArea.X, $workArea.Y, $workArea.Width, $workArea.Height)
    capture_methods = @('Native window: System.Drawing.CopyFromScreen of the visible DWM frame bounds, including OS titlebar.', 'Client: ui.screenshot from the running PDFStrike wrapper, without OS decorations.')
    fixtures = @()
    logo_assets = @()
    launches = $launches
    shots = $captures
}
foreach ($fixture in @('native-compat.pdfstrike', 'packet-real.pdf', 'form-real.pdf')) {
    $path = Join-Path $RepositoryRoot ('fixtures/pdfstrike/' + $fixture)
    $manifest.fixtures += [ordered]@{ path = 'fixtures/pdfstrike/' + $fixture; sha256 = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() }
}
foreach ($relative in @('native/pdfstrike/assets/app-icon/pdfstrike-geometric-256.png', 'native/pdfstrike/assets/app-icon/pdfstrike-geometric.ico')) {
    $path = Join-Path $RepositoryRoot $relative
    $manifest.logo_assets += [ordered]@{ path = $relative; sha256 = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() }
}

function Invoke-Ui([string]$Method, [hashtable]$Parameters = @{}) {
    if (-not $script:endpoint) { throw 'The native control endpoint is not ready.' }
    $tcp = [Net.Sockets.TcpClient]::new()
    try {
        $tcp.ReceiveTimeout = 15000
        $tcp.SendTimeout = 15000
        if (-not $tcp.ConnectAsync('127.0.0.1', [int]$script:endpoint.port).Wait(5000)) { throw 'Native control connection timed out.' }
        $stream = $tcp.GetStream()
        $reader = [IO.StreamReader]::new($stream, $script:utf8, $false, 4096, $true)
        $writer = [IO.StreamWriter]::new($stream, $script:utf8, 4096, $true)
        try {
            $writer.AutoFlush = $true
            $writer.WriteLine((@{ jsonrpc = '2.0'; id = 1; method = 'auth'; params = @{ token = $script:endpoint.token } } | ConvertTo-Json -Compress))
            $auth = $reader.ReadLine() | ConvertFrom-Json -AsHashtable
            if (-not $auth.ContainsKey('result') -or -not $auth.result.ok) { throw 'Native control authentication failed.' }
            $writer.WriteLine((@{ jsonrpc = '2.0'; id = 2; method = $Method; params = $Parameters } | ConvertTo-Json -Depth 8 -Compress))
            $line = $reader.ReadLine()
            if (-not $line) { throw "$Method returned no reply." }
            $reply = $line | ConvertFrom-Json -AsHashtable
            if ($reply.ContainsKey('error')) { throw ($Method + ': ' + $reply.error.message) }
            if (-not $reply.ContainsKey('result')) { throw "$Method returned an invalid reply." }
            return $reply.result
        } finally {
            $reader.Dispose()
            $writer.Dispose()
        }
    } finally { $tcp.Dispose() }
}

function Stop-Native {
    $script:endpoint = $null
    if ($script:process) {
        if (-not $script:process.HasExited) {
            $null = $script:process.CloseMainWindow()
            if (-not $script:process.WaitForExit(5000)) {
                # Only this script's newly created process is terminated, never an existing app.
                $script:process.Kill()
                if (-not $script:process.WaitForExit(5000)) { throw 'The capture process did not terminate within five seconds.' }
            }
        }
        $script:process.Dispose()
        $script:process = $null
    }
}

function Start-Native([string]$Fixture, [string]$Panel) {
    Stop-Native
    $control = Join-Path $script:stage ('control-' + [guid]::NewGuid().ToString('N') + '.json')
    $info = [Diagnostics.ProcessStartInfo]::new($script:stagedExe)
    $info.WorkingDirectory = $script:stage
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $false
    $info.WindowStyle = [Diagnostics.ProcessWindowStyle]::Normal
    $info.Environment['RUST_LOG'] = 'warn,pdfcraft*=info,pdfstrike_native=info'
    foreach ($argument in @('--new-window', '--control', $control, '--theme', 'light', '--language', 'en', '--panel', $Panel, '--left', 'closed', $Fixture)) {
        $info.ArgumentList.Add($argument)
    }
    $script:process = [Diagnostics.Process]::Start($info)
    $deadline = [DateTime]::UtcNow.AddSeconds(60)
    do {
        if ($script:process.HasExited) { throw "Native app exited before its window was ready (exit $($script:process.ExitCode))." }
        $script:process.Refresh()
        if ((Test-Path -LiteralPath $control) -and $script:process.MainWindowHandle -ne [IntPtr]::Zero) { break }
        Start-Sleep -Milliseconds 200
    } while ([DateTime]::UtcNow -lt $deadline)
    if (-not (Test-Path -LiteralPath $control) -or $script:process.MainWindowHandle -eq [IntPtr]::Zero) { throw 'Native window/control startup exceeded 60 seconds.' }
    $script:endpoint = Get-Content -LiteralPath $control -Raw | ConvertFrom-Json -AsHashtable
    if ([int]$script:endpoint.pid -ne $script:process.Id) { throw 'Native control endpoint belongs to another process.' }
    $handle = $script:process.MainWindowHandle
    $null = [PdfStrikeCaptureWindow]::ShowWindow($handle, 9)
    $width = [Math]::Min(1460, $script:workArea.Width)
    $height = [Math]::Min(960, $script:workArea.Height)
    # TOPMOST applies only to this hosted capture window and ends with its process.
    if (-not [PdfStrikeCaptureWindow]::SetWindowPos($handle, [IntPtr](-1), $script:workArea.X, $script:workArea.Y, $width, $height, 0x0040)) { throw 'Cannot position the native capture window.' }
    $null = [PdfStrikeCaptureWindow]::SetForegroundWindow($handle)
    $script:launches.Add([ordered]@{ fixture = [IO.Path]::GetFileName($Fixture); panel = $Panel; process_id = $script:process.Id; requested_window_size = @($width, $height) })
    $null = Invoke-Ui 'ui.set' @{ key = 'layout'; value = 'single' }
    $null = Invoke-Ui 'ui.set' @{ key = 'page'; value = '1' }
    $null = Invoke-Ui 'ui.set' @{ key = 'zoom'; value = '80' }
}

function Wait-Rendered {
    $deadline = [DateTime]::UtcNow.AddSeconds(40)
    do {
        if ($script:process.HasExited) { throw 'Native app exited during capture.' }
        $state = Invoke-Ui 'ui.state'
        if ($state.active -and @($state.active.page_errors).Count -gt 0) { throw 'The native app reported a PDF page-rendering error.' }
        if ($state.active -and @($state.active.pages_on_screen).Count -gt 0 -and $state.active.raster_memory.page_count -gt 0 -and $state.active.raster_memory.page_bytes -gt 0 -and $state.active.raster_memory.pending_requests -eq 0 -and -not $state.progress -and -not $state.notice) {
            Start-Sleep -Milliseconds 1000
            return $state
        }
        Start-Sleep -Milliseconds 250
    } while ([DateTime]::UtcNow -lt $deadline)
    throw 'The document did not reach a rendered, unobstructed state within 40 seconds.'
}

function Wait-CaptureFrame {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][scriptblock]$ReadFrame,
        [ValidateRange(1, 60000)][int]$TimeoutMilliseconds = 40000,
        [ValidateRange(1, 1000)][int]$PollMilliseconds = 250
    )
    $timer = [Diagnostics.Stopwatch]::StartNew()
    $stableCount = 0
    $stableSize = ''
    $attempts = 0
    $observed = 'no frame received'
    do {
        $frame = & $ReadFrame
        $attempts++
        $before = @($frame.Before)
        $after = @($frame.After)
        if ($before.Count -ne 2 -or $after.Count -ne 2 -or $before[0] -le 0 -or $before[1] -le 0 -or $after[0] -le 0 -or $after[1] -le 0) { throw 'Cannot measure valid native client dimensions.' }
        $observed = 'native before {0}x{1}, after {2}x{3}, screenshot {4}x{5}' -f $before[0], $before[1], $after[0], $after[1], $frame.Shot.width, $frame.Shot.height
        $size = '{0}x{1}' -f $after[0], $after[1]
        if ($before[0] -eq $after[0] -and $before[1] -eq $after[1] -and [Math]::Abs($after[0] - $frame.Shot.width) -le 2 -and [Math]::Abs($after[1] - $frame.Shot.height) -le 2) {
            $stableCount = $(if ($size -eq $stableSize) { $stableCount + 1 } else { 1 })
            $stableSize = $size
        } else {
            $stableCount = 0
            $stableSize = ''
        }
        if ($timer.ElapsedMilliseconds -ge $TimeoutMilliseconds) { break }
        if ($stableCount -ge 2) {
            $frame.Attempts = $attempts
            return $frame
        }
        $remaining = $TimeoutMilliseconds - [int]$timer.ElapsedMilliseconds
        if ($remaining -le 0) { break }
        Start-Sleep -Milliseconds ([Math]::Min($PollMilliseconds, $remaining))
    } while ($timer.ElapsedMilliseconds -lt $TimeoutMilliseconds)
    throw "Native window and screenshot did not converge before the capture deadline ($observed); refusing an ambiguous capture."
}

function Capture-Current([string]$Name, [string]$Description) {
    # Raster readiness alone does not establish that WM_SIZE and the render surface
    # have converged. Require two fresh matching frames before recording pixels.
    $frame = Wait-CaptureFrame -ReadFrame {
        $state = Wait-Rendered
        $handle = $script:process.MainWindowHandle
        if (-not [PdfStrikeCaptureWindow]::IsWindowVisible($handle) -or [PdfStrikeCaptureWindow]::GetForegroundWindow() -ne $handle) { throw 'The native window is not visible and in front; refusing a capture.' }
        $before = [PdfStrikeCaptureWindow+RECT]::new()
        if (-not [PdfStrikeCaptureWindow]::GetClientRect($handle, [ref]$before)) { throw 'Cannot measure the native client before its screenshot.' }
        $shot = Invoke-Ui 'ui.screenshot'
        $after = [PdfStrikeCaptureWindow+RECT]::new()
        if (-not [PdfStrikeCaptureWindow]::GetClientRect($handle, [ref]$after)) { throw 'Cannot measure the native client after its screenshot.' }
        return @{ Shot = $shot; State = $state; Before = @(($before.Right - $before.Left), ($before.Bottom - $before.Top)); After = @(($after.Right - $after.Left), ($after.Bottom - $after.Top)) }
    }
    $state = $frame.State
    $shot = $frame.Shot
    $clientName = $Name + '-client.png'
    $clientPath = Join-Path $script:OutputDirectory $clientName
    [IO.File]::WriteAllBytes($clientPath, [Convert]::FromBase64String($shot.png_base64))
    $capture = [ordered]@{
        name = $Name
        description = $Description
        capture_utc = [DateTime]::UtcNow.ToString('o')
        active = $state.active
        mode = $state.mode
        right_panel = $state.right_panel
        geometry = [ordered]@{ fresh_frame_attempts = $frame.Attempts; native_client_before = $frame.Before; native_client_after = $frame.After }
        client = [ordered]@{ file = $clientName; width = $shot.width; height = $shot.height; pixels_per_point = $shot.pixels_per_point; sha256 = (Get-FileHash -LiteralPath $clientPath -Algorithm SHA256).Hash.ToLowerInvariant() }
        client_file = $clientName
        client_sha256 = (Get-FileHash -LiteralPath $clientPath -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    $script:captures.Add($capture)
    $handle = $script:process.MainWindowHandle
    if (-not [PdfStrikeCaptureWindow]::IsWindowVisible($handle) -or [PdfStrikeCaptureWindow]::GetForegroundWindow() -ne $handle) {
        throw 'The native window is not visible and in front; client PNG retained, native titlebar capture refused.'
    }
    $clientBounds = [PdfStrikeCaptureWindow+RECT]::new()
    if (-not [PdfStrikeCaptureWindow]::GetClientRect($handle, [ref]$clientBounds) -or [Math]::Abs(($clientBounds.Right - $clientBounds.Left) - $shot.width) -gt 2 -or [Math]::Abs(($clientBounds.Bottom - $clientBounds.Top) - $shot.height) -gt 2) {
        throw 'The native window client dimensions do not match the app screenshot; refusing a potentially unrelated window.'
    }
    $bounds = [PdfStrikeCaptureWindow+RECT]::new()
    $result = [PdfStrikeCaptureWindow]::DwmGetWindowAttribute($handle, 9, [ref]$bounds, [Runtime.InteropServices.Marshal]::SizeOf($bounds))
    $boundsMethod = 'DWM extended frame bounds'
    if ($result -ne 0) {
        if (-not [PdfStrikeCaptureWindow]::GetWindowRect($handle, [ref]$bounds)) { throw 'Cannot obtain native window bounds.' }
        $boundsMethod = 'GetWindowRect'
    }
    $rectangle = [Drawing.Rectangle]::FromLTRB($bounds.Left, $bounds.Top, $bounds.Right, $bounds.Bottom)
    if ($rectangle.Width -le 0 -or $rectangle.Height -le 0 -or -not $script:workArea.Contains($rectangle)) { throw 'Native window is outside the visible hosted work area.' }
    $bitmap = [Drawing.Bitmap]::new($rectangle.Width, $rectangle.Height)
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    $windowName = $Name + '-window.png'
    $windowPath = Join-Path $script:OutputDirectory $windowName
    try {
        $graphics.CopyFromScreen($rectangle.Location, [Drawing.Point]::Empty, $rectangle.Size, [Drawing.CopyPixelOperation]::SourceCopy)
        if ([PdfStrikeCaptureWindow]::GetForegroundWindow() -ne $handle) { throw 'Foreground window changed during capture; refusing an ambiguous desktop image.' }
        $bitmap.Save($windowPath, [Drawing.Imaging.ImageFormat]::Png)
    } finally { $graphics.Dispose(); $bitmap.Dispose() }
    $capture.window = [ordered]@{ file = $windowName; width = $rectangle.Width; height = $rectangle.Height; bounds = @($rectangle.Left, $rectangle.Top, $rectangle.Right, $rectangle.Bottom); bounds_method = $boundsMethod; title = $script:process.MainWindowTitle; dpi = [PdfStrikeCaptureWindow]::GetDpiForWindow($handle); sha256 = (Get-FileHash -LiteralPath $windowPath -Algorithm SHA256).Hash.ToLowerInvariant() }
    $capture.window_file = $windowName
    $capture.window_sha256 = $capture.window.sha256
    Write-Output ("Captured {0}: native window {1}x{2}, client {3}x{4}" -f $Name, $rectangle.Width, $rectangle.Height, $shot.width, $shot.height)
}

try {
    $project = Join-Path $stage 'native-compat.pdfstrike'
    Copy-Item -LiteralPath (Join-Path $RepositoryRoot 'fixtures/pdfstrike/native-compat.pdfstrike') -Destination $project
    Start-Native $project 'none'
    Capture-Current 'pdfstrike-project-overview-current' 'Current native PDFStrike project wrapper with the checked-in four-page synthetic packet fixture.'
    $widgets = Invoke-Ui 'ui.inspect' @{ query = 'Comments'; limit = 100 }
    $comments = @($widgets.widgets | Where-Object { $_.ContainsKey('label') -and $_.label -eq 'Comments' -and $_.clickable -and $_.enabled -and $_.rect[0] -lt 300 })
    if ($comments.Count -ne 1) { throw 'Cannot uniquely locate the project Comments selector.' }
    $null = Invoke-Ui 'ui.click' @{ id = $comments[0].id }
    $widgets = Invoke-Ui 'ui.inspect' @{ limit = 1000 }
    $inputs = @($widgets.widgets | Where-Object { $_.role -match '(?i)multiline' -and $_.rect[0] -lt 300 })
    if ($inputs.Count -ne 1) { throw 'Cannot uniquely locate the project note editor.' }
    $rect = $inputs[0].rect
    $null = Invoke-Ui 'ui.click' @{ x = ($rect[0] + $rect[2]) / 2; y = ($rect[1] + $rect[3]) / 2 }
    $note = 'Synthetic review note: confirm the packet order before export.'
    $null = Invoke-Ui 'ui.type' @{ text = $note }
    $before = (Get-FileHash -LiteralPath $project -Algorithm SHA256).Hash
    $null = Invoke-Ui 'ui.click' @{ label = 'Add note' }
    $null = Wait-Rendered
    if ((Get-FileHash -LiteralPath $project -Algorithm SHA256).Hash -eq $before) { throw 'The project note did not produce a saved archive change.' }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [IO.Compression.ZipFile]::OpenRead($project)
    try {
        $entry = $archive.GetEntry('manifest.json')
        $reader = [IO.StreamReader]::new($entry.Open())
        try { $journal = $reader.ReadToEnd() | ConvertFrom-Json -AsHashtable } finally { $reader.Dispose() }
        if ($journal.cursor -lt 1) { throw 'The saved project journal is empty.' }
        $lastEdit = $journal.edits[$journal.cursor - 1]
        if (-not $lastEdit.ContainsKey('Note') -or $lastEdit.Note.text -ne $note) { throw 'The saved project journal does not contain the requested fictional note.' }
        $manifest.note_evidence = [ordered]@{ text = $note; project_after_sha256 = (Get-FileHash -LiteralPath $project -Algorithm SHA256).Hash.ToLowerInvariant(); journal_cursor = $journal.cursor; saved_note_verified = $true }
    } finally { $archive.Dispose() }
    Capture-Current 'pdfstrike-review-note-current' 'A fictional note was entered through the native project Comments panel and verified in the saved project journal.'
    Start-Native (Join-Path $RepositoryRoot 'fixtures/pdfstrike/form-real.pdf') 'fields'
    Capture-Current 'pdfstrike-form-fields-current' 'Current native editor showing the checked-in synthetic AcroForm PDF and its Fields panel; this is a standalone PDF, not a project Forms edit.'
    $manifest.status = 'success'
} catch {
    $manifest.status = 'failed'
    $manifest.failure = $_.Exception.Message
    throw
} finally {
    # Extract only renderer identity, never the control endpoint, full logs, profiles or tokens.
    $log = Join-Path $stage 'PDFStrikeData/logs/pdfstrike.log'
    $manifest.renderer = if (Test-Path -LiteralPath $log) { @(Get-Content -LiteralPath $log | Where-Object { $_ -match 'renderer:|drawing on ' } | Select-Object -Last 4) } else { @() }
    $manifest.finished_utc = [DateTime]::UtcNow.ToString('o')
    Stop-Native
    [IO.File]::WriteAllText((Join-Path $OutputDirectory 'capture-manifest.json'), ($manifest | ConvertTo-Json -Depth 20), $utf8)
}
