# Runs the Remote Control server for one workspace and restarts it if it exits.
param([Parameter(Mandatory)][string]$Workspace)
$settings = Join-Path $env:APPDATA 'fleet\node.settings.json'
$env:PLAYPILOT_RUNTIME = 'windows'
Set-Location $Workspace
while ($true) {
    claude remote-control --spawn worktree --capacity 2 --permission-mode auto --settings $settings
    Start-Sleep -Seconds 30
}
