# Development asset generator. The shipped icon is original PDFStrike artwork.
Add-Type -AssemblyName System.Drawing

$assetDir = Join-Path $PSScriptRoot '../assets/app-icon'
New-Item -ItemType Directory -Path $assetDir -Force | Out-Null
$size = 1024
$bmp = [System.Drawing.Bitmap]::new($size, $size)
$graphics = [System.Drawing.Graphics]::FromImage($bmp)
$graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
$graphics.Clear([System.Drawing.Color]::Transparent)

function RoundedRect([float]$x, [float]$y, [float]$w, [float]$h, [float]$r) {
    $path = [System.Drawing.Drawing2D.GraphicsPath]::new()
    $d = 2 * $r
    $path.AddArc($x, $y, $d, $d, 180, 90)
    $path.AddArc($x + $w - $d, $y, $d, $d, 270, 90)
    $path.AddArc($x + $w - $d, $y + $h - $d, $d, $d, 0, 90)
    $path.AddArc($x, $y + $h - $d, $d, $d, 90, 90)
    $path.CloseFigure()
    return $path
}

$tile = RoundedRect 32 32 960 960 176
$tileBrush = [System.Drawing.Drawing2D.LinearGradientBrush]::new(
    [System.Drawing.Point]::new(90, 50), [System.Drawing.Point]::new(910, 950),
    [System.Drawing.Color]::FromArgb(18, 29, 49),
    [System.Drawing.Color]::FromArgb(8, 17, 34))
$graphics.FillPath($tileBrush, $tile)

$shadow = RoundedRect 237 180 550 676 54
$shadowBrush = [System.Drawing.SolidBrush]::new([System.Drawing.Color]::FromArgb(85, 0, 0, 0))
$graphics.FillPath($shadowBrush, $shadow)
$page = RoundedRect 222 164 550 676 54
$pageBrush = [System.Drawing.SolidBrush]::new([System.Drawing.Color]::FromArgb(245, 249, 254))
$graphics.FillPath($pageBrush, $page)

$fold = [System.Drawing.PointF[]]@(
    [System.Drawing.PointF]::new(615,164),
    [System.Drawing.PointF]::new(772,321),
    [System.Drawing.PointF]::new(615,321))
$foldBrush = [System.Drawing.SolidBrush]::new([System.Drawing.Color]::FromArgb(200, 217, 234))
$graphics.FillPolygon($foldBrush, $fold)
$linePen = [System.Drawing.Pen]::new([System.Drawing.Color]::FromArgb(151, 171, 193), 24)
$linePen.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
$linePen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
$graphics.DrawLine($linePen, 300, 385, 550, 385)
$graphics.DrawLine($linePen, 300, 457, 672, 457)
$graphics.DrawLine($linePen, 300, 529, 622, 529)

$strikePen = [System.Drawing.Pen]::new([System.Drawing.Color]::FromArgb(16, 175, 204), 82)
$strikePen.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
$strikePen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
$graphics.DrawLine($strikePen, 327, 730, 699, 357)
$corePen = [System.Drawing.Pen]::new([System.Drawing.Color]::FromArgb(105, 230, 234), 17)
$corePen.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
$corePen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
$graphics.DrawLine($corePen, 327, 730, 699, 357)

$largePath = Join-Path $assetDir 'pdfstrike-1024.png'
$smallPath = Join-Path $assetDir 'pdfstrike-256.png'
$icoPath = Join-Path $assetDir 'pdfstrike.ico'
$bmp.Save($largePath, [System.Drawing.Imaging.ImageFormat]::Png)
$small = [System.Drawing.Bitmap]::new($bmp, [System.Drawing.Size]::new(256, 256))
$small.Save($smallPath, [System.Drawing.Imaging.ImageFormat]::Png)
$graphics.Dispose()
$bmp.Dispose()
$small.Dispose()
$tileBrush.Dispose()
$shadowBrush.Dispose()
$pageBrush.Dispose()
$foldBrush.Dispose()
$linePen.Dispose()
$strikePen.Dispose()
$corePen.Dispose()
$tile.Dispose()
$shadow.Dispose()
$page.Dispose()

$png = [System.IO.File]::ReadAllBytes($smallPath)
$stream = [System.IO.File]::Create($icoPath)
$writer = [System.IO.BinaryWriter]::new($stream)
$writer.Write([uint16]0)
$writer.Write([uint16]1)
$writer.Write([uint16]1)
$writer.Write([byte]0)
$writer.Write([byte]0)
$writer.Write([byte]0)
$writer.Write([byte]0)
$writer.Write([uint16]1)
$writer.Write([uint16]32)
$writer.Write([uint32]$png.Length)
$writer.Write([uint32]22)
$writer.Write($png)
$writer.Dispose()
