param(
    [Parameter(Mandatory)][int]$AppProcessId,
    [Parameter(Mandatory)][string]$ConfigPath
)
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_OS -ne 'Windows') {
    throw 'Physical pointer fixtures run only on a disposable Windows CI desktop.'
}
# CDP observes the real WebView2 and supplies synthetic presentation data only.
# Mouse/key input, capture ownership, cancellation and window rectangles below
# are actual User32 operations. No carry command, frame or terminal event is mocked.
Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class MatraCarryNative {
 public delegate bool WindowCallback(IntPtr h, IntPtr p);
 [StructLayout(LayoutKind.Sequential)] public struct Point { public int X, Y; }
 [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left, Top, Right, Bottom; }
 [StructLayout(LayoutKind.Sequential)] struct MonitorInfo { public uint Size; public Rect Monitor, Work; public uint Flags; }
 [StructLayout(LayoutKind.Sequential)] struct GuiInfo {
  public uint Size, Flags; public IntPtr Active, Focus, Capture, MenuOwner, MoveSize, Caret; public Rect CaretRect;
 }
 [StructLayout(LayoutKind.Sequential)] struct MouseInput { public int X,Y; public uint Data,Flags,Time; public UIntPtr Extra; }
 [StructLayout(LayoutKind.Sequential)] struct KeyInput { public ushort Key,Scan; public uint Flags,Time; public UIntPtr Extra; }
 [StructLayout(LayoutKind.Explicit)] struct InputUnion { [FieldOffset(0)] public MouseInput Mouse; [FieldOffset(0)] public KeyInput Key; }
 [StructLayout(LayoutKind.Sequential)] struct Input { public uint Type; public InputUnion Data; }
 [DllImport("user32.dll")] static extern bool EnumWindows(WindowCallback cb, IntPtr data);
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint owner);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder text, int count);
 [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h,int index);
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h,out Rect rect);
 [DllImport("user32.dll")] static extern bool GetGUIThreadInfo(uint thread,ref GuiInfo info);
 [DllImport("user32.dll")] static extern IntPtr MonitorFromWindow(IntPtr h,uint flags);
 [DllImport("user32.dll")] static extern bool GetMonitorInfo(IntPtr monitor,ref MonitorInfo info);
 [DllImport("user32.dll")] public static extern bool GetCursorPos(out Point p);
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
 [DllImport("user32.dll")] public static extern short GetAsyncKeyState(int key);
 [DllImport("user32.dll",SetLastError=true)] static extern uint SendInput(uint count,Input[] input,int size);
 [DllImport("user32.dll",SetLastError=true)] static extern IntPtr SendMessageTimeout(IntPtr h,uint msg,UIntPtr w,IntPtr l,uint flags,uint timeout,out UIntPtr result);
 public static IntPtr Window(uint owner,bool overlay) {
  var found=new List<IntPtr>();
  EnumWindows((h,p)=> { uint id;GetWindowThreadProcessId(h,out id);if(id!=owner)return true;
   var text=new StringBuilder(256);GetWindowText(h,text,text.Capacity);
   bool isOverlay=text.ToString()=="Matra carry";
   if(overlay ? isOverlay : (!isOverlay && IsWindowVisible(h) && (GetWindowLong(h,-20)&0x08000000)!=0))found.Add(h);
   return true;
  },IntPtr.Zero);
  if(found.Count>1 || (!overlay && found.Count!=1))throw new Exception("Expected one native notch/overlay; found "+found.Count);
  return found.Count==0?IntPtr.Zero:found[0];
 }
 public static Rect Bounds(IntPtr h){Rect r;if(!GetWindowRect(h,out r))throw new Exception("GetWindowRect failed");return r;}
 public static Rect Work(IntPtr h){var info=new MonitorInfo();info.Size=(uint)Marshal.SizeOf(typeof(MonitorInfo));
  if(!GetMonitorInfo(MonitorFromWindow(h,2),ref info))throw new Exception("GetMonitorInfo failed");return info.Work;}
 public static IntPtr Capture(IntPtr h){uint owner;uint thread=GetWindowThreadProcessId(h,out owner);var info=new GuiInfo();info.Size=(uint)Marshal.SizeOf(typeof(GuiInfo));
  if(!GetGUIThreadInfo(thread,ref info))throw new Exception("GetGUIThreadInfo failed");return info.Capture;}
 public static void Mouse(bool down){var i=new Input();i.Type=0;i.Data.Mouse.Flags=down?2u:4u;Send(i);}
 public static void Key(ushort key,bool down){var i=new Input();i.Type=1;i.Data.Key.Key=key;i.Data.Key.Flags=down?0u:2u;Send(i);}
 static void Send(Input i){if(SendInput(1,new[]{i},Marshal.SizeOf(typeof(Input)))!=1)throw new Exception("SendInput failed: "+Marshal.GetLastWin32Error());}
 public static void CancelCapture(IntPtr h){UIntPtr result;
  if(SendMessageTimeout(h,0x001f,UIntPtr.Zero,IntPtr.Zero,2,1000,out result)==IntPtr.Zero)throw new Exception("WM_CANCELMODE failed/timed out");}
}
'@
function Query([string]$Code, [string]$Page = 'notch', [string]$Screenshot = '') {
    $arguments = @((Join-Path $PSScriptRoot 'native-inspect.js'), $Page, $Code)
    if ($Screenshot) { $arguments += $Screenshot }
    $json = & node @arguments
    if ($LASTEXITCODE -ne 0) { throw "WebView2 observation failed: $Page" }
    return ($json | ConvertFrom-Json).value
}
function Assert-Native([bool]$Condition, [string]$Message) {
    if (!$Condition) { throw $Message }
    Write-Host "PASS: $Message"
}
function Wait-Native([scriptblock]$Probe, [string]$Message, [int]$Seconds = 8) {
    $deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
    do {
        if (& $Probe) { return }
        Start-Sleep -Milliseconds 40
    } while ([DateTime]::UtcNow -lt $deadline)
    $state = Query "({carrying,dragging,hidden:document.body.classList.contains('carry-hidden'),events:window.__nativeCarryEvents})"
    throw "$Message : $($state | ConvertTo-Json -Depth 8 -Compress)"
}
function Move-Native([double]$X, [double]$Y) {
    Assert-Native ([MatraCarryNative]::SetCursorPos([int][Math]::Round($X),[int][Math]::Round($Y))) 'OS accepted physical cursor position'
    Start-Sleep -Milliseconds 180
}
function Placement {
    $config = Get-Content -LiteralPath $ConfigPath -Raw | ConvertFrom-Json
    # Ignore independent quota/activity writes; compare every saved placement field.
    return ([ordered]@{edge=$config.notch_edge;monitor=$config.notch_monitor;along=$config.notch_along} | ConvertTo-Json -Depth 6 -Compress)
}
function Is-SameRect($A, $B) {
    return [Math]::Abs($A.Left-$B.Left) -le 1 -and [Math]::Abs($A.Top-$B.Top) -le 1 -and [Math]::Abs($A.Right-$B.Right) -le 1 -and [Math]::Abs($A.Bottom-$B.Bottom) -le 1
}
$repo = Split-Path $PSScriptRoot -Parent
$output = Join-Path $repo '.build\development-lifecycle\native-carry.json'
$results = [System.Collections.Generic.List[object]]::new()
$oldDpi = [MatraCarryNative]::SetThreadDpiAwarenessContext([IntPtr](-4))
$savedCursor = New-Object MatraCarryNative+Point
[MatraCarryNative]::GetCursorPos([ref]$savedCursor) | Out-Null
$notch = [MatraCarryNative]::Window($AppProcessId,$false)
$work = [MatraCarryNative]::Work($notch)
function Begin-PhysicalCarry([bool]$Grip = $true) {
    Query "hideCard();onHover=false;unfold();setDockActive(true);reportHot();true" | Out-Null
    $selector = if ($Grip) { 'moveHandle' } else { 'pill' }
    $point = Query "window.__TAURI__.window.getCurrentWindow().outerPosition().then(pos=>{const r=$selector.getBoundingClientRect();const x=r.left+r.width/2,y=r.top+r.height/2;return {x:pos.x+x*devicePixelRatio,y:pos.y+y*devicePixelRatio,hit:onHandle(x,y),dpr:devicePixelRatio}})"
    if ($Grip) { Assert-Native ($point.hit -eq 'move') 'Physical press starts at the actual grip hit target' }
    Move-Native $point.x $point.y
    if (!$Grip) { [MatraCarryNative]::Key(0x12,$true) }
    [MatraCarryNative]::Mouse($true)
    Assert-Native ([MatraCarryNative]::GetAsyncKeyState(1) -lt 0) 'Windows reports the held physical left button'
    if (!$Grip) { Move-Native ($point.x+12*$point.dpr) ($point.y+12*$point.dpr) }
    Wait-Native { Query "carrying && document.body.classList.contains('carry-hidden')" } 'Actual pointer press did not present the native carry overlay'
    Assert-Native ([MatraCarryNative]::Capture($notch) -eq $notch) 'The owning UI thread captured the ordinary notch HWND'
    $overlay = [MatraCarryNative]::Window($AppProcessId,$true)
    Assert-Native ($overlay -ne [IntPtr]::Zero -and [MatraCarryNative]::IsWindowVisible($overlay)) 'A native carry overlay is visible'
    Assert-Native (Is-SameRect ([MatraCarryNative]::Bounds($overlay)) $work) 'Carry overlay occupies the actual monitor work area'
    Assert-Native (([MatraCarryNative]::GetWindowLong($overlay,-20) -band 0x20) -ne 0) 'Overlay is native click-through'
    Assert-Native (([MatraCarryNative]::GetWindowLong($overlay,-20) -band 0x08000000) -ne 0) 'Overlay is nonactivating'
    $scene = Query "invoke('get_zones')"
    Assert-Native ($scene.presentation.grip -eq $Grip) 'Physical grip/Alt-body path reaches the correct native presentation'
    $drawn = Query "({cells:document.querySelectorAll('#cells image').length,body:document.querySelectorAll('#body path').length})" 'dropzones'
    Assert-Native ($drawn.cells -gt 0 -and $drawn.cells -eq $scene.presentation.cells.Count -and $drawn.body -gt 0) 'Real overlay painted the carried provider cells and body'
    return $scene
}
function End-PhysicalCarry([long]$Id, [string]$Status) {
    [MatraCarryNative]::Mouse($false)
    [MatraCarryNative]::Key(0x12,$false)
    Wait-Native { Query "!carrying && !dragging && !document.body.classList.contains('carry-hidden') && window.__nativeCarryEvents.some(e=>e.id===$Id)" } 'Carry did not restore the normal body' 12
    Wait-Native { [MatraCarryNative]::Window($AppProcessId,$true) -eq [IntPtr]::Zero } 'Carry overlay survived terminal cleanup'
    Assert-Native ([MatraCarryNative]::Capture($notch) -eq [IntPtr]::Zero) 'Native pointer capture is released after terminal cleanup'
    $terminal = Query "window.__nativeCarryEvents.filter(e=>e.id===$Id)"
    Assert-Native (@($terminal).Count -eq 1 -and $terminal[0].status -eq $Status) "One native carry terminal event reports $Status"
    return $terminal[0]
}
try {
    Query "(async()=>{window.__nativeCarryEvents=[];window.__nativeCarryStop=await window.__TAURI__.event.listen('carry_end',e=>window.__nativeCarryEvents.push(e.payload));await invoke('set_move_handle',{on:true});await invoke('set_notch_edge',{edge:'left'});return true})()" | Out-Null
    Start-Sleep -Milliseconds 650
    $notch = [MatraCarryNative]::Window($AppProcessId,$false)
    $work = [MatraCarryNative]::Work($notch)
    $before = Placement
    $scene = Begin-PhysicalCarry
    $firstPlace = $scene.frame.place
    Move-Native ($work.Left+3) ($work.Top+8)
    $corner = Query "invoke('get_zones')"
    Move-Native (($work.Left+$work.Right)/2) ($work.Top+3)
    Start-Sleep -Milliseconds 350
    $moved = Query "invoke('get_zones')"
    Assert-Native ([Math]::Abs($moved.frame.place-$firstPlace) -gt 5) 'Physical cursor movement changes the actual native carried body frame'
    Query "document.title" 'dropzones' 'windows-development-physical-carry.png' | Out-Null
    $terminal = End-PhysicalCarry $scene.frame.id 'finished'
    $after = Placement
    $edge = Query "invoke('get_notch_edge')"
    $bounds = [MatraCarryNative]::Bounds($notch)
    Assert-Native ($edge -eq 'top' -and [Math]::Abs($bounds.Top-$work.Top) -le 1) 'Release commits the top edge and places the actual notch window there'
    Assert-Native ($after -ne $before) 'Successful carry persists a changed native placement'
    $results.Add(@{case='physical-grip-release';terminal=$terminal;before=$before;after=$after;work=$work;bounds=$bounds;cornerObserved=($null -ne $corner.frame.corner)})

    foreach ($cancel in @('escape','capture-loss')) {
        $before = Placement
        $originalBounds = [MatraCarryNative]::Bounds($notch)
        $scene = Begin-PhysicalCarry
        Move-Native (($work.Left+$work.Right)/2) ($work.Bottom-4)
        if ($cancel -eq 'escape') {
            [MatraCarryNative]::Key(0x1b,$true)
            Start-Sleep -Milliseconds 160
            [MatraCarryNative]::Key(0x1b,$false)
        } else {
            # DefWindowProc's WM_CANCELMODE handling releases capture on the
            # owning UI thread. GetGUIThreadInfo proves actual native loss.
            [MatraCarryNative]::CancelCapture($notch)
            Assert-Native ([MatraCarryNative]::Capture($notch) -ne $notch) 'External WM_CANCELMODE removed actual native capture'
        }
        $terminal = End-PhysicalCarry $scene.frame.id 'cancelled'
        Assert-Native ((Placement) -ceq $before) "$cancel leaves all saved placement fields unchanged"
        Assert-Native (Is-SameRect ([MatraCarryNative]::Bounds($notch)) $originalBounds) "$cancel restores the original native window rectangle"
        $results.Add(@{case=$cancel;terminal=$terminal;placement=$before;bounds=$originalBounds})
    }

    $scene = Begin-PhysicalCarry $false
    Assert-Native (Query 'dragging') 'Physical Alt-body gesture entered the native drag path'
    Move-Native (($work.Left+$work.Right)*0.5+100) ($work.Top+3)
    $terminal = End-PhysicalCarry $scene.frame.id 'finished'
    $results.Add(@{case='physical-alt-body';terminal=$terminal;placement=(Placement)})
    Assert-Native ($results.Count -eq 4) 'All four physical native carry cases completed'
    $results | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $output -Encoding utf8NoBOM
    Write-Output 'PASS: actual Windows physical grip/release, Escape cancellation, external capture loss and Alt-body carry; native HWND capture, overlay paint, placement persistence and cleanup.'
} finally {
    [MatraCarryNative]::Mouse($false)
    [MatraCarryNative]::Key(0x12,$false)
    [MatraCarryNative]::Key(0x1b,$false)
    [MatraCarryNative]::SetCursorPos($savedCursor.X,$savedCursor.Y) | Out-Null
    if ($oldDpi -ne [IntPtr]::Zero) { [MatraCarryNative]::SetThreadDpiAwarenessContext($oldDpi) | Out-Null }
    try { Query "if(window.__nativeCarryStop)window.__nativeCarryStop();true" | Out-Null } catch { Write-Warning $_ }
    # Partial receipts remain diagnostic, never a successful terminal result.
    if (!(Test-Path $output)) { $results | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath ($output+'.partial') -Encoding utf8NoBOM }
}
