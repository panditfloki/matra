$ErrorActionPreference='Stop'
Add-Type @'
using System.Runtime.InteropServices;
public class MatraHoverCursor {
 [StructLayout(LayoutKind.Sequential)] public struct Point { public int X; public int Y; }
 [DllImport("user32.dll")] public static extern bool GetCursorPos(out Point p);
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
}
'@
[MatraHoverCursor]::SetProcessDPIAware() | Out-Null
function Query([string]$code) {
    $json=& node (Join-Path $PSScriptRoot 'native-inspect.js') notch $code
    if($LASTEXITCODE -ne 0){throw 'Native inspection failed'}
    ($json | ConvertFrom-Json).value
}
function Move-To([string]$point) {
    $target=Query "window.__TAURI__.window.getCurrentWindow().outerPosition().then(pos=>{const p=pill.getBoundingClientRect(),r=document.getElementById('rest').getBoundingClientRect(),v=edgeIsVertical(),q=$point;return {x:pos.x+q[0]*devicePixelRatio,y:pos.y+q[1]*devicePixelRatio}})"
    [MatraHoverCursor]::SetCursorPos([int]$target.x,[int]$target.y) | Out-Null
    Start-Sleep -Milliseconds 650
}
function Check([string]$stage,[string]$move,[string]$settings) {
    $actual=Query "({move:getComputedStyle(moveHandle).opacity,settings:getComputedStyle(orb).opacity,folded,pointerIn,hovered})"
    if($actual.move -ne $move -or $actual.settings -ne $settings){throw "$stage failed: $($actual | ConvertTo-Json -Compress)"}
    Write-Output "$stage PASS: drag=$move settings=$settings"
}
$saved=New-Object MatraHoverCursor+Point
[MatraHoverCursor]::GetCursorPos([ref]$saved) | Out-Null
try {
    Move-To '[r.left+r.width/2,r.top+r.height/2]'
    Move-To '[p.left+p.width/2,p.top+p.height/2]'
    Check 'Provider area' '0' '0'
    Move-To 'v?[p.left+p.width/2,p.top+2]:[p.left+2,p.top+p.height/2]'
    Check 'Drag border' '1' '0'
    Move-To '[moveAt.x,moveAt.y]'
    Check 'Drag circle' '1' '0'
    Move-To '[p.left+p.width/2,p.top+p.height/2]'
    Check 'Return to provider' '0' '0'
    Move-To 'v?[p.left+p.width/2,p.bottom-2]:[p.right-2,p.top+p.height/2]'
    Check 'Settings border' '0' '1'
    Move-To '[orbAt.x,orbAt.y]'
    Check 'Settings circle' '0' '1'
    Move-To '[innerWidth+100,innerHeight+100]'
    Check 'Leave window' '0' '0'
} finally {[MatraHoverCursor]::SetCursorPos($saved.X,$saved.Y) | Out-Null}
