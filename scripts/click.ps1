# Dev helper: click at a point relative to the headroom window's top-left.
param([int]$X, [int]$Y, [int]$Wheel = 0)
Add-Type @"
using System; using System.Runtime.InteropServices;
public class C {
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int x, int y, uint d, UIntPtr e);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  public struct RECT { public int L, T, R, B; }
}
"@
[C]::SetProcessDPIAware() | Out-Null
$p = Get-Process headroom | Where-Object MainWindowHandle -ne 0 | Select-Object -First 1
[C]::SetForegroundWindow($p.MainWindowHandle) | Out-Null
$r = New-Object C+RECT; [C]::GetWindowRect($p.MainWindowHandle, [ref]$r) | Out-Null
[C]::SetCursorPos($r.L + $X, $r.T + $Y) | Out-Null
Start-Sleep -Milliseconds 100
if ($Wheel) { [C]::mouse_event(0x800, 0, 0, [uint32]([int64]$Wheel -band 0xFFFFFFFFL), [UIntPtr]::Zero) } else { [C]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero); [C]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero) }
