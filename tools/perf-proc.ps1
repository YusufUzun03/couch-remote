# Boşta kaynak ölçümü: CouchRemote (veya couch-agent) işlemini <sn> saniye örnekler, JSON yazar.
# Kullanım: powershell -File perf-proc.ps1 -Seconds 30
param([int]$Seconds = 30)
Add-Type @'
using System; using System.Runtime.InteropServices; using System.Text;
public static class Thr {
  [DllImport("kernel32.dll")] public static extern IntPtr OpenThread(int access, bool inherit, int id);
  [DllImport("kernel32.dll")] public static extern bool CloseHandle(IntPtr h);
  [DllImport("kernel32.dll")] public static extern int GetThreadDescription(IntPtr h, out IntPtr desc);
  [DllImport("kernel32.dll")] public static extern IntPtr LocalFree(IntPtr p);
  public static string Name(int id) {
    IntPtr h = OpenThread(0x0800 /*QUERY_LIMITED_INFORMATION*/, false, id);
    if (h == IntPtr.Zero) return "";
    try { IntPtr d; if (GetThreadDescription(h, out d) >= 0 && d != IntPtr.Zero) { string s = Marshal.PtrToStringUni(d); LocalFree(d); return s; } return ""; }
    finally { CloseHandle(h); }
  }
}
'@
$p = Get-Process CouchRemote, couch-agent -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $p) { Write-Error 'işlem yok'; exit 1 }
$t0 = @{}; foreach ($t in $p.Threads) { $t0[$t.Id] = $t.TotalProcessorTime.TotalMilliseconds }
$cpu0 = $p.TotalProcessorTime.TotalMilliseconds
$sw = [Diagnostics.Stopwatch]::StartNew()
$ws = @(); $priv = @()
for ($i = 0; $i -lt $Seconds; $i++) { Start-Sleep 1; $p.Refresh(); $ws += $p.WorkingSet64; $priv += $p.PrivateMemorySize64 }
$p.Refresh()
$wall = $sw.Elapsed.TotalMilliseconds
$cpu = $p.TotalProcessorTime.TotalMilliseconds - $cpu0
$threads = @()
foreach ($t in $p.Threads) {
  $d = $t.TotalProcessorTime.TotalMilliseconds - $(if ($t0.ContainsKey($t.Id)) { $t0[$t.Id] } else { 0 })
  if ($d -gt 0) { $threads += [pscustomobject]@{ id = $t.Id; name = [Thr]::Name($t.Id); cpuMs = [math]::Round($d, 1) } }
}
[pscustomobject]@{
  seconds = $Seconds
  cpuPercentOfOneCore = [math]::Round(100 * $cpu / $wall, 2)
  workingSetMB = [math]::Round(($ws | Measure-Object -Average).Average / 1MB, 1)
  privateMB = [math]::Round(($priv | Measure-Object -Average).Average / 1MB, 1)
  threads = $p.Threads.Count
  handles = $p.HandleCount
  hotThreads = ($threads | Sort-Object cpuMs -Descending | Select-Object -First 8)
} | ConvertTo-Json -Depth 4
