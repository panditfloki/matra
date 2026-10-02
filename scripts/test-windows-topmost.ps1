param([Parameter(Mandatory)][int]$AppProcessId)
$ErrorActionPreference = 'Stop'
# Only window metadata and our own short-lived cover are used. No account data,
# screenshots, focus changes or saved settings are needed for this regression.
Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class MatraTopmostTest {
 public delegate bool Callback(IntPtr h, IntPtr p);
 public struct Rect { public int Left, Top, Right, Bottom; }
 [DllImport("user32.dll")] static extern bool EnumWindows(Callback c, IntPtr p);
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
 [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
 [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h, int n);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out Rect r);
 [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h, uint command);
 [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int w, int height, uint flags);
 [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern IntPtr CreateWindowEx(uint ex, string cls, string name, uint style, int x, int y, int w, int height, IntPtr parent, IntPtr menu, IntPtr instance, IntPtr data);
 [DllImport("user32.dll")] static extern bool ShowWindow(IntPtr h, int command);
 [DllImport("user32.dll")] public static extern bool DestroyWindow(IntPtr h);
 public static IntPtr Notch(uint pid) {
  var found = new List<IntPtr>();
  EnumWindows((h,p)=> { uint owner; GetWindowThreadProcessId(h,out owner);
   if(owner==pid && (GetWindowLong(h,-20)&0x08000000)!=0) {
    var cls=new StringBuilder(100);GetClassName(h,cls,100);
    if(cls.ToString().Equals("Tauri Window",StringComparison.OrdinalIgnoreCase)) found.Add(h);
   } return true; },IntPtr.Zero);
  if(found.Count!=1) throw new Exception("Expected one notch in the specified app process, found "+found.Count);
  return found[0];
 }
 public static Rect Bounds(IntPtr h) {Rect r;if(!GetWindowRect(h,out r))throw new Exception("Cannot read bounds");return r;}
 public static IntPtr Cover(Rect r) {
  var h=CreateWindowEx(0x08000088,"STATIC","Matra overlap regression",0x80000000,
   r.Left+20,r.Top+20,80,80,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero);
  if(h==IntPtr.Zero) throw new Exception("Cannot create isolated cover");
  ShowWindow(h,4);return h;
 }
 public static bool Above(IntPtr first,IntPtr second) {
  for(var h=GetWindow(second,3);h!=IntPtr.Zero;h=GetWindow(h,3))if(h==first)return true;
  return false;
 }
}
'@
function Assert-Condition([bool]$Value, [string]$Message) {
    if (!$Value) { throw $Message }
    Write-Output "PASS: $Message"
}
function Wait-Condition([scriptblock]$Check) {
    $deadline = [DateTime]::UtcNow.AddSeconds(4)
    do {
        if (& $Check) { return $true }
        Start-Sleep -Milliseconds 50
    } while ([DateTime]::UtcNow -lt $deadline)
    return $false
}
$notch = [MatraTopmostTest]::Notch($AppProcessId)
Assert-Condition ([MatraTopmostTest]::IsWindowVisible($notch)) 'Notch is visible before the regression'
$before = [MatraTopmostTest]::Bounds($notch)
$foreground = [MatraTopmostTest]::GetForegroundWindow()
$cover = [IntPtr]::Zero
try {
    Assert-Condition ([MatraTopmostTest]::SetWindowPos($notch,[IntPtr](-2),0,0,0,0,0x213)) 'Native demotion succeeded'
    Assert-Condition (Wait-Condition { ([MatraTopmostTest]::GetWindowLong($notch,-20) -band 8) -ne 0 }) 'App restores the actual topmost flag after native demotion'
    $cover = [MatraTopmostTest]::Cover($before)
    Assert-Condition ([MatraTopmostTest]::SetWindowPos($cover,[IntPtr](-1),0,0,0,0,0x213)) 'Overlapping topmost cover was raised'
    Assert-Condition ([MatraTopmostTest]::Above($cover,$notch)) 'Cover reproduces hiding the notch in native Z order'
    Assert-Condition (Wait-Condition { [MatraTopmostTest]::Above($notch,$cover) }) 'App restores the notch above an overlapping topmost window'
    $after = [MatraTopmostTest]::Bounds($notch)
    Assert-Condition (($before.Left -eq $after.Left) -and ($before.Top -eq $after.Top) -and ($before.Right -eq $after.Right) -and ($before.Bottom -eq $after.Bottom)) 'Repair preserves notch position and size'
    Assert-Condition ([MatraTopmostTest]::GetForegroundWindow() -eq $foreground) 'Repair preserves keyboard focus'
    Assert-Condition (([MatraTopmostTest]::GetWindowLong($notch,-20) -band 0x08000000) -ne 0) 'Notch remains non-activating'
} finally {
    if ($cover -ne [IntPtr]::Zero) { [MatraTopmostTest]::DestroyWindow($cover) | Out-Null }
    # Restore the original topmost behavior even when testing an unfixed build.
    [MatraTopmostTest]::SetWindowPos($notch,[IntPtr](-1),0,0,0,0,0x213) | Out-Null
}
