# Drives the real game window by posting mouse messages straight to its window handle.
# It never moves or uses the real mouse cursor, so you can keep working while it runs.
# Usage: powershell -File tools\click_test.ps1
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class W32 {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr wParam, IntPtr lParam);
}
"@
[void][W32]::SetProcessDPIAware()

$WM_MOUSEMOVE = 0x0200; $WM_LBUTTONDOWN = 0x0201; $WM_LBUTTONUP = 0x0202; $MK_LBUTTON = 1

$exe = Join-Path $PSScriptRoot "..\game\target\fast\abg-game.exe"
$p = Start-Process $exe -ArgumentList "--landing" -PassThru
Start-Sleep -Seconds 4
$p.Refresh()
$hwnd = $p.MainWindowHandle

function Send-Mouse([uint32]$msg, [int]$wparam, [int]$x, [int]$y) {
    $lparam = [IntPtr](($y -shl 16) -bor ($x -band 0xFFFF))
    [void][W32]::PostMessage($hwnd, $msg, [IntPtr]$wparam, $lparam)
}

function Click-Stage([double]$sx, [double]$sy, [string]$label) {
    $r = New-Object W32+RECT
    [void][W32]::GetClientRect($hwnd, [ref]$r)
    $scale = [math]::Min($r.Right / 1920.0, $r.Bottom / 1080.0)
    $ox = ($r.Right - 1920 * $scale) / 2; $oy = ($r.Bottom - 1080 * $scale) / 2
    $x = [int]($ox + $sx * $scale); $y = [int]($oy + $sy * $scale)
    Send-Mouse $WM_MOUSEMOVE 0 $x $y
    Start-Sleep -Milliseconds 200
    Send-Mouse $WM_LBUTTONDOWN $MK_LBUTTON $x $y
    Start-Sleep -Milliseconds 80
    Send-Mouse $WM_LBUTTONUP 0 $x $y
    Start-Sleep -Milliseconds 600
    $p.Refresh()
    "{0,-34} -> title: '{1}'" -f $label, $p.MainWindowTitle
}

"start                              -> title: '$($p.MainWindowTitle)'"
Click-Stage 960 500  "click GO! (landing)"
Click-Stage 1640 1035 "click settings gear (map)"
Click-Stage 960 300  "click outside settings (close)"
Click-Stage 115 591  "click the first campaign event"

if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force }
