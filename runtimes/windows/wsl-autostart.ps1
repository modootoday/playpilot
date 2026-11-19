# Starts a WSL distribution at Windows boot so an always-on node comes back after a reboot.
# instanceIdleTimeout=-1 (.wslconfig) keeps a running instance alive but does not start one;
# with systemd and linger on, starting the distribution starts the user's claude-rc@ services.
# Run once in a normal PowerShell as the node's Windows user. Idempotent.
param([Parameter(Mandatory)][string]$Distro)
$ErrorActionPreference = 'Stop'
if (-not ((wsl.exe -l -q) -replace "`0", '' | Where-Object { $_.Trim() -eq $Distro })) {
    throw "no WSL distribution named $Distro (wsl -l -q)"
}
$action = New-ScheduledTaskAction -Execute 'wsl.exe' -Argument "-d $Distro --exec /bin/true"
$trigger = New-ScheduledTaskTrigger -AtStartup
$principal = New-ScheduledTaskPrincipal -UserId "$env:USERDOMAIN\$env:USERNAME" -LogonType S4U
$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries `
    -ExecutionTimeLimit (New-TimeSpan -Minutes 5) -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1)
Register-ScheduledTask -TaskName "wsl-autostart-$Distro" -Action $action -Trigger $trigger `
    -Principal $principal -Settings $settings -Force | Out-Null
Write-Host "Registered wsl-autostart-$Distro. Check after a reboot: wsl -l -v shows $Distro Running."
