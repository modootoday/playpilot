# Devices and lanes

Two physical machines serve the fleet. They are different devices with
different jobs; never assume one can see the other's files.

| Device (Remote Control title) | Primary lane | Fallback | Notes |
| --- | --- | --- | --- |
| OSS Desktop | `needs-windows-run`, `needs-gui-check` | `needs-heavy-build` | Owner's PC. Native Windows (MSVC, WebView2, GUI). GUI quality approval happens here, by the owner. |
| OSS Desktop #2 | `needs-heavy-build` | `needs-windows-run` | Build node. WSL ext4 workspace, sccache, long test suites. |

Cloud lanes (no device):

| Runtime | Use | Limits |
| --- | --- | --- |
| Claude cloud session / scheduled task | Roadmap slices, reviews, PRs | Hourly run caps; auto mode; repo `.claude/settings.json` applies |
| Cloudflare container (optional) | Headless `claude -p` jobs, paid mode | No Remote Control; credentials injected by the Worker |

Rules:
- A device takes only items labelled with its lanes; it takes a fallback lane
  item only when that item has waited 2 hours unclaimed.
- Each device keeps its own clones under `~/fleet/` (WSL) or its Windows
  workspace. No shared network folders between devices.
- Results go back as a pull request comment or a new pull request, with the
  exact command, commit SHA and outcome.
