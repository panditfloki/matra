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
 // Every top-level HWND owned by the fixture process, with the metadata needed
 // to identify it. Tao's thread event target is also visible and nonactivating,
 // so style alone never identifies the notch; selection happens in the script.
 public sealed class TopLevel { public IntPtr Handle; public string Class, Title; public bool Visible; public int Style, ExStyle; public Rect Bounds; }
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder text, int count);
 public static List<TopLevel> Windows(uint owner) {
  var found=new List<TopLevel>();
  EnumWindows((h,p)=> { uint id;GetWindowThreadProcessId(h,out id);if(id!=owner)return true;
   var text=new StringBuilder(256);GetWindowText(h,text,text.Capacity);
   var cls=new StringBuilder(256);GetClassName(h,cls,cls.Capacity);
   Rect r;if(!GetWindowRect(h,out r))r=new Rect();
   found.Add(new TopLevel{Handle=h,Class=cls.ToString(),Title=text.ToString(),Visible=IsWindowVisible(h),Style=GetWindowLong(h,-16),ExStyle=GetWindowLong(h,-20),Bounds=r});
   return true;
  },IntPtr.Zero);
  return found;
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
# Tauri registers every app window under this class (tauri-runtime-wry). The
# carry overlay is the only one titled 'Matra carry' (dropzones.rs).
$tauriClass = 'Tauri Window'
$overlayTitle = 'Matra carry'
$windowDiagnostics = $null
function Describe-Windows {
    return @([MatraCarryNative]::Windows($AppProcessId) | ForEach-Object {
        [ordered]@{handle=('0x{0:X}' -f $_.Handle.ToInt64());'class'=$_.Class;title=$_.Title;visible=$_.Visible
            style=('0x{0:X8}' -f $_.Style);exStyle=('0x{0:X8}' -f $_.ExStyle)
            bounds=[ordered]@{left=$_.Bounds.Left;top=$_.Bounds.Top;right=$_.Bounds.Right;bottom=$_.Bounds.Bottom}}
    })
}
function Fail-Lookup([string]$Message, $Reported) {
    # Lookup failures carry every candidate so CI shows which HWNDs competed.
    $script:windowDiagnostics = [ordered]@{reason=$Message;reported=$Reported;windows=@(Describe-Windows)}
    throw "$Message; candidates: $($script:windowDiagnostics | ConvertTo-Json -Depth 6 -Compress)"
}
function Find-Notch {
    # Positive identity: the notch webview reports its own label and physical
    # outer rectangle; exactly one visible nonactivating Tauri HWND of this PID
    # must occupy that rectangle. Retry briefly while an edge change settles.
    $deadline = [DateTime]::UtcNow.AddSeconds(4)
    do {
        $reported = Query "(async()=>{const w=window.__TAURI__.window.getCurrentWindow();const p=await w.outerPosition(),s=await w.outerSize();return {label:w.label,left:p.x,top:p.y,right:p.x+s.width,bottom:p.y+s.height}})()"
        $candidates = @([MatraCarryNative]::Windows($AppProcessId) | Where-Object {
            $_.Class -eq $tauriClass -and $_.Title -ne $overlayTitle -and $_.Visible -and ($_.ExStyle -band 0x08000000) -ne 0 -and (Is-SameRect $_.Bounds $reported)
        })
        if ($reported.label -eq 'notch' -and $candidates.Count -eq 1) { return $candidates[0].Handle }
        Start-Sleep -Milliseconds 100
    } while ([DateTime]::UtcNow -lt $deadline)
    Fail-Lookup "Expected exactly one visible nonactivating '$tauriClass' notch HWND at the notch webview's reported rectangle; found $($candidates.Count)" $reported
}
function Find-Overlay {
    # Zero while no carry is presented; more than one is always a defect.
    $found = @([MatraCarryNative]::Windows($AppProcessId) | Where-Object { $_.Class -eq $tauriClass -and $_.Title -eq $overlayTitle })
    if ($found.Count -gt 1) { Fail-Lookup "Expected at most one '$overlayTitle' overlay HWND; found $($found.Count)" $null }
    if ($found.Count -eq 0) { return [IntPtr]::Zero }
    return $found[0].Handle
}
$repo = Split-Path $PSScriptRoot -Parent
$output = Join-Path $repo '.build\development-lifecycle\native-carry.json'
$results = [System.Collections.Generic.List[object]]::new()
# The installer harness invokes this script in-process. Thread DPI, held input,
# cursor and listener changes are therefore made only inside the guarded block
# and each is undone independently in finally.
$oldDpi = [IntPtr]::Zero
$savedCursor = New-Object MatraCarryNative+Point
$cursorSaved = $false
$listenerInstalled = $false
$primaryError = $null
$cleanupErrors = [System.Collections.Generic.List[string]]::new()
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
    $overlay = Find-Overlay
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
    Wait-Native { (Find-Overlay) -eq [IntPtr]::Zero } 'Carry overlay survived terminal cleanup'
    Assert-Native ([MatraCarryNative]::Capture($notch) -eq [IntPtr]::Zero) 'Native pointer capture is released after terminal cleanup'
    # @() keeps a single-event result an array; the failure names every observed payload.
    $terminal = @(Query "window.__nativeCarryEvents.filter(e=>e.id===$Id)")
    if (!($terminal.Count -eq 1 -and $terminal[0].status -eq $Status)) {
        throw "Expected one native carry terminal event reporting $Status for id $Id; observed $($terminal.Count): $(ConvertTo-Json -InputObject $terminal -Depth 6 -Compress)"
    }
    Write-Host "PASS: One native carry terminal event reports $Status"
    return $terminal[0]
}
function Invoke-Cleanup([string]$Name, [scriptblock]$Step) {
    # One failed restoration step must neither hide the primary failure nor
    # prevent the remaining restorations.
    try { & $Step } catch { $cleanupErrors.Add("${Name}: $($_.Exception.Message)") }
}
try {
    $oldDpi = [MatraCarryNative]::SetThreadDpiAwarenessContext([IntPtr](-4))
    if ($oldDpi -eq [IntPtr]::Zero) { throw 'Per-monitor-v2 DPI awareness could not be applied to the physical input thread' }
    $cursorSaved = [MatraCarryNative]::GetCursorPos([ref]$savedCursor)
    $listenerInstalled = $true
    Query "(async()=>{window.__nativeCarryEvents=[];window.__nativeCarryStop=await window.__TAURI__.event.listen('carry_end',e=>window.__nativeCarryEvents.push(e.payload));await invoke('set_move_handle',{on:true});await invoke('set_notch_edge',{edge:'left'});return true})()" | Out-Null
    Start-Sleep -Milliseconds 650
    $notch = Find-Notch
    $work = [MatraCarryNative]::Work($notch)
    $before = Placement
    $scene = Begin-PhysicalCarry
    $firstPlace = $scene.frame.place
    # Walk the real cursor up the left edge and around the top-left corner in
    # small steps. The body sits ahead of the grip along the edge, so it must
    # straddle the corner before the cursor arrives; every published native
    # frame reports that split, which is asserted below rather than recorded.
    $start = New-Object MatraCarryNative+Point
    [MatraCarryNative]::GetCursorPos([ref]$start) | Out-Null
    $step = [Math]::Max(4, [Math]::Min(24, $scene.frame.length / 8))
    $cornerFrames = [System.Collections.Generic.List[object]]::new()
    $sweep = [System.Collections.Generic.List[object]]::new()
    for ($y = [double]$start.Y; $y -gt ($work.Top + 3); $y -= $step) { $sweep.Add(@(($work.Left+3), $y)) }
    $sweep.Add(@(($work.Left+3), ($work.Top+3)))
    for ($x = [double]($work.Left + 3); $x -lt (($work.Left + $work.Right) / 2); $x += 4 * $step) { $sweep.Add(@($x, ($work.Top+3))) }
    foreach ($at in $sweep) {
        Move-Native $at[0] $at[1]
        $frame = (Query "invoke('get_zones')").frame
        if ($null -ne $frame.corner) {
            $cornerFrames.Add([ordered]@{pointer=$at;corner=$frame.corner;from=$frame.from;to=$frame.to;turned=$frame.turned;before=$frame.before;after=$frame.after;place=$frame.place;length=$frame.length})
        }
    }
    Move-Native (($work.Left+$work.Right)/2) ($work.Top+3)
    Start-Sleep -Milliseconds 350
    $moved = Query "invoke('get_zones')"
    Assert-Native ([Math]::Abs($moved.frame.place-$firstPlace) -gt 5) 'Physical cursor movement changes the actual native carried body frame'
    $topLeft = @($cornerFrames | Where-Object { $_.corner -eq 'topLeft' -and $_.from -eq 'left' -and $_.to -eq 'top' })
    Assert-Native ($topLeft.Count -ge 2) "Native carried body split across the top-left corner during physical traversal ($($cornerFrames.Count) corner frames)"
    Assert-Native ((@($topLeft | Where-Object { $_.turned -lt 0.5 }).Count -gt 0) -and (@($topLeft | Where-Object { $_.turned -gt 0.5 }).Count -gt 0)) 'Corner split turned from mostly-left to mostly-top during traversal'
    Query "document.title" 'dropzones' 'windows-development-physical-carry.png' | Out-Null
    $terminal = End-PhysicalCarry $scene.frame.id 'finished'
    $after = Placement
    $edge = Query "invoke('get_notch_edge')"
    $bounds = [MatraCarryNative]::Bounds($notch)
    Assert-Native ($edge -eq 'top' -and [Math]::Abs($bounds.Top-$work.Top) -le 1) 'Release commits the top edge and places the actual notch window there'
    Assert-Native ($after -ne $before) 'Successful carry persists a changed native placement'
    $results.Add(@{case='physical-grip-corner-release';terminal=$terminal;before=$before;after=$after;work=$work;bounds=$bounds;cornerFrames=$cornerFrames})

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
    Write-Output 'PASS: actual Windows physical grip traversal across the top-left corner with release, Escape cancellation, external capture loss and Alt-body carry; native HWND capture, overlay paint, corner split frames, placement persistence and cleanup.'
} catch {
    $primaryError = $_
    throw
} finally {
    Invoke-Cleanup 'left button release' { [MatraCarryNative]::Mouse($false) }
    Invoke-Cleanup 'Alt release' { [MatraCarryNative]::Key(0x12,$false) }
    Invoke-Cleanup 'Escape release' { [MatraCarryNative]::Key(0x1b,$false) }
    if ($cursorSaved) { Invoke-Cleanup 'cursor restore' { if (![MatraCarryNative]::SetCursorPos($savedCursor.X,$savedCursor.Y)) { throw 'SetCursorPos failed' } } }
    if ($oldDpi -ne [IntPtr]::Zero) { Invoke-Cleanup 'DPI restore' { if ([MatraCarryNative]::SetThreadDpiAwarenessContext($oldDpi) -eq [IntPtr]::Zero) { throw 'SetThreadDpiAwarenessContext failed' } } }
    if ($listenerInstalled) { Invoke-Cleanup 'carry listener removal' { Query "if(window.__nativeCarryStop)window.__nativeCarryStop();true" | Out-Null } }
    # Partial receipts remain diagnostic, never a successful terminal result.
    if (!(Test-Path $output)) {
        $failureText = if ($primaryError) { "$primaryError" } else { $null }
        Invoke-Cleanup 'partial receipt' {
            [ordered]@{passed=$false;error=$failureText;cleanupErrors=$cleanupErrors;windows=$windowDiagnostics;results=$results} |
                ConvertTo-Json -Depth 10 | Set-Content -LiteralPath ($output+'.partial') -Encoding utf8NoBOM
        }
    }
    foreach ($message in $cleanupErrors) { Write-Warning "Carry cleanup: $message" }
    # The primary exception, if any, continues to propagate unchanged after this
    # block. Without one, an incomplete restoration is itself a failure.
    if (!$primaryError -and $cleanupErrors.Count -gt 0) { throw "Physical carry cleanup failed: $($cleanupErrors -join '; ')" }
}
