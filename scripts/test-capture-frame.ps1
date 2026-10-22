# Pure capture-timing controls: imports only the function AST and never launches an app.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$tokens = $null
$errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'capture-current-ui.ps1'), [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Capture script must parse.' }
$definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Wait-CaptureFrame' }, $true)
if (-not $definition) { throw 'Capture must wait for fresh frames to converge with stable native client dimensions.' }
. ([scriptblock]::Create($definition.Extent.Text))
function Assert([bool]$Condition, [string]$Message) { if (-not $Condition) { throw $Message } }
function Frame([int]$Width = 1008, [int]$Height = 681, [int[]]$Before = @(1008, 681), [int[]]$After = @(1008, 681)) {
    return @{ Shot = @{ width = $Width; height = $Height }; Before = $Before; After = $After }
}

# The failed hosted run returned 1024x768 while the successful fitted client was 1008x681.
$script:calls = 0
$frame = Wait-CaptureFrame -TimeoutMilliseconds 1000 -PollMilliseconds 1 -ReadFrame {
    $script:calls++
    if ($script:calls -eq 1) { return (Frame 1024 768) }
    return (Frame)
}
Assert ($script:calls -eq 3 -and $frame.Attempts -eq 3) 'A stale first frame must be discarded and followed by two matching fresh frames.'

$script:calls = 0
$null = Wait-CaptureFrame -TimeoutMilliseconds 1000 -PollMilliseconds 1 -ReadFrame {
    $script:calls++
    if ($script:calls -eq 2) { return (Frame -After @(1007, 681)) }
    return (Frame)
}
Assert ($script:calls -eq 4) 'A resize during a screenshot must reset the stable-frame count.'

$script:calls = 0
$frame = Wait-CaptureFrame -TimeoutMilliseconds 1000 -PollMilliseconds 1 -ReadFrame {
    $script:calls++
    return (Frame 1010 683)
}
Assert ($script:calls -eq 2) 'The existing two-pixel tolerance must still accept matching stable frames.'

$message = ''
try {
    $null = Wait-CaptureFrame -TimeoutMilliseconds 20 -PollMilliseconds 1 -ReadFrame { Frame 1011 684 }
} catch { $message = $_.Exception.Message }
Assert ($message.Contains('did not converge') -and $message.Contains('1011x684') -and $message.Contains('1008x681')) 'Persistent mismatch beyond two pixels must time out with observed screenshot and native dimensions.'

$message = ''
try {
    $null = Wait-CaptureFrame -TimeoutMilliseconds 100 -PollMilliseconds 1 -ReadFrame { throw 'foreground changed' }
} catch { $message = $_.Exception.Message }
Assert ($message -eq 'foreground changed') 'Visibility, foreground and native measurement failures must propagate immediately.'
Write-Output 'Passed five capture-frame convergence controls without launching an app.'
