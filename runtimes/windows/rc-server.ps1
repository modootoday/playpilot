# Runs the Remote Control server for one workspace and restarts it if it exits.
param([Parameter(Mandatory)][string]$Workspace, [int]$Capacity = 2)
$settings = Join-Path $env:USERPROFILE 'fleet\.node\node.settings.json'
$env:PLAYPILOT_RUNTIME = 'windows'
Set-Location $Workspace
while ($true) {
    claude remote-control --spawn worktree --capacity $Capacity --permission-mode auto --settings $settings
    Start-Sleep -Seconds 30
}
