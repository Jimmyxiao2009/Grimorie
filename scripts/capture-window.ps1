# Captures a window to a PNG, for visual verification during development.
#
# Caveat, measured rather than assumed: on a DPI-scaled display the WebView2
# surface returned by PrintWindow is positioned as though the viewport were
# scale-factor larger, so the content sits off-centre in the resulting image by
# a proportional amount. That is fine for "does this render, does this work"
# checks, which is what this script is for. It is NOT a reliable way to measure
# responsive layout — use a real browser viewport at exact CSS pixel sizes for
# that, since CSS pixels are what the breakpoints actually respond to.
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
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
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
# WebView2 surface even when another window is on top. The whole window is
# saved, frame included: cropping to the client rect was tried and misaligned,
# because the offsets GetWindowRect reports do not match where PrintWindow
# places the content.
$wr = New-Object Win+RECT; [void][Win]::GetWindowRect($h, [ref]$wr)
$bw = $wr.Right - $wr.Left
$bh = $wr.Bottom - $wr.Top
if ($bw -le 0 -or $bh -le 0) { Write-Error 'Window has no drawable area.'; exit 1 }

$bmp = New-Object System.Drawing.Bitmap $bw, $bh
$gfx = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $gfx.GetHdc()
$ok = [Win]::PrintWindow($h, $hdc, 2)
$gfx.ReleaseHdc($hdc)
if (-not $ok) {
  # Fall back to a screen grab; this requires the window to be unobscured.
  $gfx.CopyFromScreen($wr.Left, $wr.Top, 0, 0, (New-Object System.Drawing.Size $bw, $bh))
}
$gfx.Dispose()

$dir = Split-Path -Parent $Out
if ($dir -and -not (Test-Path -LiteralPath $dir)) { [void](New-Item -ItemType Directory -Force -Path $dir) }
$target = [System.IO.Path]::GetFullPath((Join-Path (Get-Location).Path $Out))
$bmp.Save($target, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()

$cr = New-Object Win+RECT; [void][Win]::GetClientRect($h, [ref]$cr)
Write-Output "captured window ${bw}x${bh} (client $($cr.Right)x$($cr.Bottom)) -> $Out"
