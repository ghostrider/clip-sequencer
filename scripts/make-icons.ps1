# Renders the app icons from the logo mark (web/public/logo-mark.png): white mark on a dark rounded square.
# Writes web/public/icon.png (128), web/public/favicon.png (32), crates/seq-app/icons/icon.png (1024) and icon.ico.
# Usage:  powershell -ExecutionPolicy Bypass -File scripts\make-icons.ps1 -Root .
param([string]$Root)
Add-Type -AssemblyName System.Drawing
$mark = [System.Drawing.Bitmap]::new((Join-Path $Root 'web\public\logo-mark.png'))
function New-Icon([int]$size) {
  $bmp = [System.Drawing.Bitmap]::new($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.SmoothingMode = 'AntiAlias'; $g.InterpolationMode = 'HighQualityBicubic'; $g.PixelOffsetMode = 'HighQuality'; $g.CompositingQuality = 'HighQuality'
  $g.Clear([System.Drawing.Color]::Transparent)
  $m = [Math]::Max(1.0, $size * 0.035)            # transparent edge
  $s = $size - 2 * $m; $r = $s * 0.22               # rounded square
  $p = [System.Drawing.Drawing2D.GraphicsPath]::new()
  $p.AddArc($m, $m, 2*$r, 2*$r, 180, 90); $p.AddArc($m+$s-2*$r, $m, 2*$r, 2*$r, 270, 90)
  $p.AddArc($m+$s-2*$r, $m+$s-2*$r, 2*$r, 2*$r, 0, 90); $p.AddArc($m, $m+$s-2*$r, 2*$r, 2*$r, 90, 90); $p.CloseFigure()
  $g.FillPath([System.Drawing.SolidBrush]::new([System.Drawing.Color]::FromArgb(255, 14, 15, 17)), $p)
  # The mark: 72 % of the square's width, centred (small sizes a little larger so it stays readable).
  $fw = if ($size -le 24) { 0.84 } elseif ($size -le 48) { 0.78 } else { 0.72 }
  $w = $s * $fw; $h = $w * $mark.Height / $mark.Width
  $g.DrawImage($mark, [System.Drawing.RectangleF]::new(($size - $w) / 2, ($size - $h) / 2, $w, $h))
  $g.Dispose(); return $bmp
}
function Save-Png($bmp, $path) { $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png) }
Save-Png (New-Icon 128) (Join-Path $Root 'web\public\icon.png')
Save-Png (New-Icon 32) (Join-Path $Root 'web\public\favicon.png')
Save-Png (New-Icon 1024) (Join-Path $Root 'crates\seq-app\icons\icon.png')
# ICO with PNG-compressed entries (Windows Vista and later), the sizes the old one had.
$sizes = 16, 24, 32, 48, 64, 128, 256
$pngs = foreach ($z in $sizes) { $ms = [IO.MemoryStream]::new(); (New-Icon $z).Save($ms, [System.Drawing.Imaging.ImageFormat]::Png); ,$ms.ToArray() }
$out = [IO.MemoryStream]::new(); $wr = [IO.BinaryWriter]::new($out)
$wr.Write([UInt16]0); $wr.Write([UInt16]1); $wr.Write([UInt16]$sizes.Count)
$offset = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
  $z = $sizes[$i]; $b = if ($z -ge 256) { 0 } else { $z }
  $wr.Write([byte]$b); $wr.Write([byte]$b); $wr.Write([byte]0); $wr.Write([byte]0)
  $wr.Write([UInt16]1); $wr.Write([UInt16]32); $wr.Write([UInt32]$pngs[$i].Length); $wr.Write([UInt32]$offset)
  $offset += $pngs[$i].Length
}
foreach ($d in $pngs) { $wr.Write($d) }
[IO.File]::WriteAllBytes((Join-Path $Root 'crates\seq-app\icons\icon.ico'), $out.ToArray())
$mark.Dispose()
'icons written'
