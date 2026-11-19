# Prepares a Windows user account as a fleet node (native Windows lane:
# MSVC builds, WebView2/GUI checks). Idempotent. Run in a normal (non-admin) PowerShell.
$ErrorActionPreference = 'Stop'
$repo = Resolve-Path (Join-Path $PSScriptRoot '..\..')
foreach ($tool in 'git', 'gh', 'cargo') {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) { throw "install $tool first (winget)" }
}
if (-not (Get-Command claude -ErrorAction SilentlyContinue)) {
    Invoke-RestMethod https://claude.ai/install.ps1 | Invoke-Expression
}
cargo install --locked --path (Join-Path $repo 'crates\playpilot')
$cfg = Join-Path $env:APPDATA 'fleet'
New-Item -ItemType Directory -Force $cfg | Out-Null
Copy-Item (Join-Path $repo 'profiles\node.settings.json') (Join-Path $cfg 'node.settings.json') -Force
Copy-Item (Join-Path $PSScriptRoot 'rc-server.ps1') (Join-Path $cfg 'rc-server.ps1') -Force
Write-Host "Next (owner, once): claude auth login; then register the task from playpilot render node: Register-ScheduledTask -TaskName claude-rc -Xml (Get-Content -Raw claude-rc.xml)."
