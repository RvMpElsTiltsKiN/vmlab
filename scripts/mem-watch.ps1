# Registra la RAM en uso y los 5 procesos que mas consumen cada N segundos.
# Uso:  powershell -ExecutionPolicy Bypass -File scripts\mem-watch.ps1
#       powershell -ExecutionPolicy Bypass -File scripts\mem-watch.ps1 -Log mem-watch.log
# Dejalo abierto, abre y cierra VMLab, y para con Ctrl+C. Con -Log queda el historial.
param(
    [int]$Seconds = 5,
    [string]$Log = ""
)

$ErrorActionPreference = 'SilentlyContinue'

Write-Host "Midiendo cada $Seconds s. Ctrl+C para parar." -ForegroundColor Cyan
if ($Log) { Write-Host "Registrando en $Log" -ForegroundColor Cyan }

while ($true) {
    $os = Get-CimInstance Win32_OperatingSystem
    $used = ($os.TotalVisibleMemorySize - $os.FreePhysicalMemory) / 1MB
    $top = Get-Process |
        Group-Object ProcessName |
        ForEach-Object {
            [pscustomobject]@{
                Name = $_.Name
                MB   = [math]::Round((($_.Group | Measure-Object WorkingSet64 -Sum).Sum) / 1MB)
            }
        } |
        Sort-Object MB -Descending |
        Select-Object -First 5

    $line = "{0:HH:mm:ss}  En uso {1,5:N2} GB  |  {2}" -f (Get-Date), $used, (($top | ForEach-Object { "$($_.Name)=$($_.MB)MB" }) -join '  ')

    if ($Log) { Add-Content -Path $Log -Value $line }
    else { Write-Host $line }

    Start-Sleep -Seconds $Seconds
}
