# Pure window-selection controls: imports only the selector AST; no app or WinAPI calls.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$tokens = $null
$errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'capture-current-ui.ps1'), [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Capture script must parse.' }
$definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq 'Select-CaptureWindow' }, $true)
if (-not $definition) { throw 'Capture window selection is missing: Select-CaptureWindow must reject the internal winit event window.' }
. ([scriptblock]::Create($definition.Extent.Text))

function Assert([bool]$Condition, [string]$Message) { if (-not $Condition) { throw $Message } }
function Window(
    [long]$Handle,
    [int]$WindowProcessId = 4242,
    [bool]$Visible = $true,
    [bool]$HasOwner = $false,
    [string]$ClassName = 'Window Class',
    [string]$Title = 'packet.pdf - PDFStrike',
    [int]$Width = 1008,
    [int]$Height = 681
) {
    return [pscustomobject]@{
        Handle = $Handle; ProcessId = $WindowProcessId; Visible = $Visible; HasOwner = $HasOwner
        ClassName = $ClassName; Title = $Title; Width = $Width; Height = $Height
    }
}
function Assert-Handle($Selected, [long]$ExpectedHandle, [string]$Message) {
    Assert ($null -ne $Selected -and [long]$Selected.Handle -eq $ExpectedHandle) $Message
}

# The internal event target satisfies .NET's visible/unowned/PID heuristic and can
# occur before the GUI. Select by the actual GUI identity instead of enumeration order.
$eventTarget = Window -Handle 101 -ClassName 'Winit Thread Event Target' -Title '' -Width 1024 -Height 720
$gui = Window -Handle 202
$selected = Select-CaptureWindow -Windows @($eventTarget, $gui) -ExpectedProcessId 4242
Assert-Handle $selected 202 'The first visible internal winit event target must be skipped in favor of the actual GUI.'

$foreign = Window -Handle 303 -WindowProcessId 9000
Assert ($null -eq (Select-CaptureWindow -Windows @($foreign) -ExpectedProcessId 4242)) 'A matching window from another PID must be rejected.'
Assert-Handle (Select-CaptureWindow -Windows @($foreign, $gui) -ExpectedProcessId 4242) 202 'A foreign matching window must not compete with the launched process GUI.'

$ineligible = @(
    (Window -Handle 404 -Visible $false),
    (Window -Handle 505 -HasOwner $true),
    (Window -Handle 606 -Width 0),
    (Window -Handle 707 -Height 0),
    (Window -Handle 708 -Width -1)
)
Assert ($null -eq (Select-CaptureWindow -Windows $ineligible -ExpectedProcessId 4242)) 'Hidden, owned and non-positive client dimensions must all be rejected.'
Assert-Handle (Select-CaptureWindow -Windows ($ineligible + @($gui)) -ExpectedProcessId 4242) 202 'Ineligible own-process windows must not mask the actual GUI.'

Assert-Handle (Select-CaptureWindow -Windows @((Window -Handle 808 -Title 'PDFStrike')) -ExpectedProcessId 4242) 808 'The exact initial PDFStrike title must be accepted.'
Assert-Handle (Select-CaptureWindow -Windows @((Window -Handle 909 -Title 'native-compat.pdfstrike - PDFStrike')) -ExpectedProcessId 4242) 909 'A document title ending in the exact PDFStrike suffix must be accepted.'

$wrongIdentity = @(
    (Window -Handle 1001 -Title 'PDFStrike - unrelated'),
    (Window -Handle 1002 -Title 'Some PDFStrike'),
    (Window -Handle 1003 -Title 'packet.pdf - pdfstrike'),
    (Window -Handle 1004 -ClassName 'Window Class helper'),
    (Window -Handle 1005 -ClassName 'window class')
)
Assert ($null -eq (Select-CaptureWindow -Windows $wrongIdentity -ExpectedProcessId 4242)) 'Only the exact GUI class and exact app title or document suffix may be accepted.'

$threw = $false
try {
    $null = Select-CaptureWindow -Windows @($gui, (Window -Handle 1101 -Title 'other.pdf - PDFStrike')) -ExpectedProcessId 4242
} catch { $threw = $true }
Assert $threw 'Two eligible GUI windows must throw instead of choosing one by enumeration order.'

Assert ($null -eq (Select-CaptureWindow -Windows @($eventTarget) -ExpectedProcessId 4242)) 'Zero eligible GUI windows must return null.'
Assert ($null -eq (Select-CaptureWindow -Windows @() -ExpectedProcessId 4242)) 'An empty candidate set must return null.'
Write-Output 'Passed eleven capture-window selection controls without launching an app or calling WinAPI.'
