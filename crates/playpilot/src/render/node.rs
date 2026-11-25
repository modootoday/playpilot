//! `playpilot render node`: the service files that keep a device's Remote
//! Control servers running, one per entry in the `Device`'s `spec.runtimes`.
//!
//! `wsl` gets the systemd user unit of `runtimes/wsl/systemd/`; `windows` gets
//! a Task Scheduler task that runs `rc-server.ps1` at logon.

use crate::validate::Resource;

pub const WSL_UNIT: &str = "claude-rc@.service";
pub const WINDOWS_TASK: &str = "claude-rc.xml";
const MAX_CAPACITY: i64 = 32;

/// One runtime of a device that hosts a Remote Control server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runtime {
    Wsl { capacity: u32 },
    Windows { capacity: u32 },
}

/// A rendered file: its name in the output directory and its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeFile {
    pub name: &'static str,
    pub text: String,
}

/// Reads the runtimes of the named device.
pub fn runtimes(resources: &[Resource], device: &str) -> Result<Vec<Runtime>, String> {
    let d = super::find(resources, "Device", device)?;
    let list = d
        .spec
        .get("runtimes")
        .and_then(toml::Value::as_array)
        .filter(|a| !a.is_empty())
        .ok_or_else(|| format!("{}: spec.runtimes must list at least one runtime", d.file))?;
    list.iter()
        .map(|r| runtime(r).map_err(|e| format!("{}: spec.runtimes: {e}", d.file)))
        .collect()
}

fn runtime(v: &toml::Value) -> Result<Runtime, String> {
    let kind = v
        .get("type")
        .and_then(toml::Value::as_str)
        .ok_or("each runtime needs a `type`")?;
    let capacity = v
        .get("capacity")
        .and_then(toml::Value::as_integer)
        .filter(|c| (1..=MAX_CAPACITY).contains(c))
        .ok_or_else(|| format!("runtime \"{kind}\" needs a capacity from 1 to {MAX_CAPACITY}"))?;
    let capacity = u32::try_from(capacity).map_err(|e| e.to_string())?;
    match kind {
        "wsl" => Ok(Runtime::Wsl { capacity }),
        "windows" => Ok(Runtime::Windows { capacity }),
        other => Err(format!("runtime \"{other}\" has no node service")),
    }
}

/// Renders every runtime's file. `workspace` is the directory under
/// `%USERPROFILE%\fleet` the Windows task serves; only `windows` needs it.
pub fn node_files(runtimes: &[Runtime], workspace: Option<&str>) -> Result<Vec<NodeFile>, String> {
    let mut files = Vec::new();
    for r in runtimes {
        let file = match *r {
            Runtime::Wsl { capacity } => NodeFile {
                name: WSL_UNIT,
                text: wsl_unit(capacity),
            },
            Runtime::Windows { capacity } => NodeFile {
                name: WINDOWS_TASK,
                text: windows_task(capacity, windows_workspace(workspace)?),
            },
        };
        if files.iter().any(|f: &NodeFile| f.name == file.name) {
            return Err(format!("two runtimes render {}", file.name));
        }
        files.push(file);
    }
    Ok(files)
}

fn windows_workspace(workspace: Option<&str>) -> Result<&str, String> {
    let w =
        workspace.ok_or("a windows runtime needs --workspace <dir under %USERPROFILE%\\fleet>")?;
    plain_workspace(w)
}

/// A workspace directory name: plain, no drive or parent reference.
pub fn plain_workspace(w: &str) -> Result<&str, String> {
    if super::is_plain(w) && !w.contains(':') && !w.starts_with('.') {
        Ok(w)
    } else {
        Err(format!(
            "--workspace \"{w}\" must be a plain directory name"
        ))
    }
}

/// The systemd user unit; the instance name is the workspace directory.
pub fn wsl_unit(capacity: u32) -> String {
    format!(
        concat!(
            "# Remote Control server for one workspace directory under ~/fleet/<name>.\n",
            "[Unit]\n",
            "Description=Claude Code Remote Control server (%i)\n",
            "After=network-online.target\n",
            "Wants=network-online.target\n",
            "\n",
            "[Service]\n",
            "Type=simple\n",
            "WorkingDirectory=%h/fleet/%i\n",
            "Environment=PLAYPILOT_RUNTIME=wsl\n",
            "ExecStart=%h/.local/bin/claude remote-control --spawn worktree --capacity {capacity} ",
            "--permission-mode auto --settings %h/.config/fleet/node.settings.json\n",
            "# Server mode exits after ~10 minutes without network; come back on our own.\n",
            "Restart=always\n",
            "RestartSec=30\n",
            "# The server redraws a TUI about once a second; keep it out of the journal, keep errors.\n",
            "StandardOutput=null\n",
            "StandardError=journal\n",
            "\n",
            "[Install]\n",
            "WantedBy=default.target\n"
        ),
        capacity = capacity,
    )
}

/// Replaced with `DOMAIN\\user` by the register command `node install` prints,
/// so the logon trigger is the installing user's own and needs no elevation.
pub const TASK_USER: &str = "__PLAYPILOT_TASK_USER__";
/// Task Scheduler does not expand `%USERPROFILE%` in `<Arguments>`, so the
/// register command fills in the absolute path.
pub const TASK_PROFILE: &str = "__PLAYPILOT_USERPROFILE__";

/// The action runs through `conhost.exe --headless` because
/// `powershell -WindowStyle Hidden` alone still flashes a console at logon.
/// A per-user Task Scheduler task: starts `rc-server.ps1` (installed by
/// `bootstrap.ps1` into `%USERPROFILE%\fleet\.node`) at logon and restarts it
/// on failure.
pub fn windows_task(capacity: u32, workspace: &str) -> String {
    format!(
        concat!(
            "<?xml version=\"1.0\"?>\n",
            "<Task version=\"1.2\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">\n",
            "  <RegistrationInfo>\n",
            "    <Description>Claude Code Remote Control server ({workspace})</Description>\n",
            "  </RegistrationInfo>\n",
            "  <Triggers>\n",
            "    <LogonTrigger>\n",
            "      <Enabled>true</Enabled>\n",
            "      <UserId>{user}</UserId>\n",
            "    </LogonTrigger>\n",
            "  </Triggers>\n",
            "  <Principals>\n",
            "    <Principal id=\"Author\">\n",
            "      <UserId>{user}</UserId>\n",
            "      <LogonType>InteractiveToken</LogonType>\n",
            "      <RunLevel>LeastPrivilege</RunLevel>\n",
            "    </Principal>\n",
            "  </Principals>\n",
            "  <Settings>\n",
            "    <Hidden>true</Hidden>\n",
            "    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>\n",
            "    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>\n",
            "    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>\n",
            "    <StartWhenAvailable>true</StartWhenAvailable>\n",
            "    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>\n",
            "    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>\n",
            "    <RestartOnFailure>\n",
            "      <Interval>PT1M</Interval>\n",
            "      <Count>999</Count>\n",
            "    </RestartOnFailure>\n",
            "    <Enabled>true</Enabled>\n",
            "  </Settings>\n",
            "  <Actions Context=\"Author\">\n",
            "    <Exec>\n",
            "      <Command>conhost.exe</Command>\n",
            "      <Arguments>--headless powershell.exe -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden ",
            "-File \"{profile}\\fleet\\.node\\rc-server.ps1\" ",
            "-Workspace \"{profile}\\fleet\\{workspace}\" -Capacity {capacity}</Arguments>\n",
            "    </Exec>\n",
            "  </Actions>\n",
            "</Task>\n"
        ),
        capacity = capacity,
        workspace = workspace,
        user = TASK_USER,
        profile = TASK_PROFILE,
    )
}
