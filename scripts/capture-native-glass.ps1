param([int]$Left,[int]$Top,[int]$Width,[int]$Height,[string]$OutputPath,[switch]$Foreground,[long]$NotchWindow=0)
# A controlled test background, not a capture of unrelated desktop content.
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System.Runtime.InteropServices;
public class GlassTestDpi {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetWindowPos(System.IntPtr h, System.IntPtr after, int x,int y,int w,int height,uint flags);
}
'@
[GlassTestDpi]::SetProcessDPIAware() | Out-Null
$form = New-Object Windows.Forms.Form
$form.FormBorderStyle = 'None'
$form.StartPosition = 'Manual'
$form.AutoScaleMode = 'None'
$form.Bounds = New-Object Drawing.Rectangle($Left,$Top,$Width,$Height)
$form.ShowInTaskbar = $false
$form.TopMost = $Foreground.IsPresent
$form.BackColor = [Drawing.Color]::White
$form.Add_Paint({param($sender,$event)
    for($stripe=0;$stripe -lt $sender.Width;$stripe+=12) {
        $brush = if (($stripe / 12) % 2 -eq 0) { [Drawing.Brushes]::DarkSlateBlue } else { [Drawing.Brushes]::White }
        $event.Graphics.FillRectangle($brush,$stripe,0,12,$sender.Height)
    }
})
$timer = New-Object Windows.Forms.Timer
$timer.Interval = 1800
$timer.Add_Tick({
    $timer.Stop()
    $bitmap = New-Object Drawing.Bitmap($Width,$Height)
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.CopyFromScreen($Left,$Top,0,0,$bitmap.Size)
        $bitmap.Save($OutputPath,[Drawing.Imaging.ImageFormat]::Png)
    } finally { $graphics.Dispose(); $bitmap.Dispose(); $form.Close() }
})
$form.Add_Shown({
    if ($NotchWindow -ne 0) { [GlassTestDpi]::SetWindowPos([IntPtr]$NotchWindow,[IntPtr](-1),0,0,0,0,0x13) | Out-Null }
    $timer.Start()
})
[Windows.Forms.Application]::Run($form)
$timer.Dispose(); $form.Dispose()
