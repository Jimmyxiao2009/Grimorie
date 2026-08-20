# Captures a window to a PNG, for visual verification during development.
#
# -Width/-Height set the *client* size, which is what the app lays out in, so
# the responsive states can be checked at their real dimensions.
#
# Note that on this machine one client pixel is one CSS pixel: WebView2 reports
# window.innerWidth in the same units as the client rect. The window's backing
# store is still DPI-scaled, which is handled below.
#
#   powershell -NoProfile -File scripts/capture-window.ps1 -Out shot.png
#   powershell -NoProfile -File scripts/capture-window.ps1 -Out shot.png -Width 1024 -Height 768

param(
  [string]$Title = 'Grimoire',
  [string]$Out = 'shot.png',
  [int]$Width = 0,
  [int]$Height = 0,
  [int]$SettleMs = 700
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

Add-Type @'
using System;
using System.Runtime.InteropServices;
public class Win {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
  [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int hh, bool repaint);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
}
'@

$proc = Get-Process | Where-Object { $_.MainWindowTitle -eq $Title } | Select-Object -First 1
if (-not $proc) { Write-Error "No window titled '$Title' is open."; exit 1 }
$h = $proc.MainWindowHandle

[void][Win]::ShowWindow($h, 9)   # SW_RESTORE — undo any maximised state
try { [void][Win]::SetForegroundWindow($h) } catch { }
Start-Sleep -Milliseconds 250

if ($Width -gt 0 -and $Height -gt 0) {
  # Grow the requested client size by the frame, so the *client* area — which is
  # what the app actually lays out in — ends up at the requested dimensions.
  $wr = New-Object Win+RECT; [void][Win]::GetWindowRect($h, [ref]$wr)
  $cr = New-Object Win+RECT; [void][Win]::GetClientRect($h, [ref]$cr)
  $chromeW = ($wr.Right - $wr.Left) - $cr.Right
  $chromeH = ($wr.Bottom - $wr.Top) - $cr.Bottom
  [void][Win]::MoveWindow($h, $wr.Left, $wr.Top, ($Width + $chromeW), ($Height + $chromeH), $true)
  Start-Sleep -Milliseconds $SettleMs
}
Start-Sleep -Milliseconds $SettleMs

# PW_RENDERFULLCONTENT (2) asks the window to draw itself, which captures the
# WebView2 surface even when another window is on top.
$wr = New-Object Win+RECT; [void][Win]::GetWindowRect($h, [ref]$wr)
$bw = $wr.Right - $wr.Left
$bh = $wr.Bottom - $wr.Top
if ($bw -le 0 -or $bh -le 0) { Write-Error 'Window has no drawable area.'; exit 1 }

# PrintWindow draws into the window's *backing store*, which on a scaled display
# is devicePixelRatio times larger than the logical window rect. Sizing the
# bitmap to the logical rect therefore captures only the top-left fraction of
# the content — measured, not guessed. The bitmap is allocated at physical size
# and the result is scaled back down.
$dpi = [Win]::GetDpiForWindow($h)
if ($dpi -le 0) { $dpi = 96 }
$scale = $dpi / 96.0
$pw = [int][Math]::Round($bw * $scale)
$ph = [int][Math]::Round($bh * $scale)

$raw = New-Object System.Drawing.Bitmap $pw, $ph
$gfx = [System.Drawing.Graphics]::FromImage($raw)
$hdc = $gfx.GetHdc()
$ok = [Win]::PrintWindow($h, $hdc, 2)
$gfx.ReleaseHdc($hdc)
if (-not $ok) {
  # Fall back to a screen grab; this requires the window to be unobscured.
  $gfx.CopyFromScreen($wr.Left, $wr.Top, 0, 0, (New-Object System.Drawing.Size $pw, $ph))
}
$gfx.Dispose()

# Scale back to logical pixels, then crop to the client area so that image
# coordinates are client coordinates. That equivalence is what lets a position
# read off a screenshot be clicked directly by drive-window.ps1.
$cr = New-Object Win+RECT; [void][Win]::GetClientRect($h, [ref]$cr)
$origin = New-Object Win+POINT; [void][Win]::ClientToScreen($h, [ref]$origin)
$offsetX = $origin.X - $wr.Left
$offsetY = $origin.Y - $wr.Top

$bmp = New-Object System.Drawing.Bitmap $cr.Right, $cr.Bottom
$sg = [System.Drawing.Graphics]::FromImage($bmp)
$sg.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
$sg.DrawImage(
  $raw,
  (New-Object System.Drawing.Rectangle 0, 0, $cr.Right, $cr.Bottom),
  (New-Object System.Drawing.Rectangle ([int]($offsetX * $scale)), ([int]($offsetY * $scale)),
    ([int]($cr.Right * $scale)), ([int]($cr.Bottom * $scale))),
  [System.Drawing.GraphicsUnit]::Pixel)
$sg.Dispose()
$raw.Dispose()

$dir = Split-Path -Parent $Out
if ($dir -and -not (Test-Path -LiteralPath $dir)) { [void](New-Item -ItemType Directory -Force -Path $dir) }
$target = [System.IO.Path]::GetFullPath((Join-Path (Get-Location).Path $Out))
$bmp.Save($target, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()

Write-Output "captured client $($cr.Right)x$($cr.Bottom) at dpi $dpi -> $Out"
