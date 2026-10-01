# İmleç konumunu FİZİKSEL piksel olarak yazar (DPI ölçeklendirmesinden bağımsız).
Add-Type -Namespace Couch -Name Native -MemberDefinition @'
[System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
public struct POINT { public int X; public int Y; }
[System.Runtime.InteropServices.DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(System.IntPtr value);
[System.Runtime.InteropServices.DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
'@
[void][Couch.Native]::SetProcessDpiAwarenessContext([System.IntPtr](-4))
$p = New-Object Couch.Native+POINT
[void][Couch.Native]::GetCursorPos([ref]$p)
Write-Output "$($p.X),$($p.Y)"
