# Test aracı: ön plana gelen küçük bir pencere, aldığı tuş basışlarını (tuş|shift|ctrl) dosyaya yazar. Kullanım: keylog.ps1 <dosya> [süre-ms]
param([string]$Log, [int]$Ms = 7000)
Add-Type -AssemblyName System.Windows.Forms
$f = New-Object Windows.Forms.Form
$f.Text = 'CouchRemote tuş testi'; $f.TopMost = $true; $f.KeyPreview = $true; $f.Width = 320; $f.Height = 110; $f.StartPosition = 'CenterScreen'
$f.Add_KeyDown({ param($s, $e) Add-Content -Path $Log -Value ('{0}|{1}|{2}' -f $e.KeyCode, $e.Shift, $e.Control) })
$f.Add_Shown({ $f.Activate(); Set-Content -Path ($Log + '.ready') -Value 'ok' })
$t = New-Object Windows.Forms.Timer; $t.Interval = $Ms; $t.Add_Tick({ $f.Close() }); $t.Start()
[Windows.Forms.Application]::Run($f)
