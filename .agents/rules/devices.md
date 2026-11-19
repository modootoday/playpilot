# Devices and lanes

Each physical machine is a separate node; never assume one can see another's
files. Which machine serves which lane is instance configuration, kept in the
instance's private configuration repository, not here.

| Lane | Needs | Typical node |
| --- | --- | --- |
| `needs-heavy-build` | `build:heavy` | a build node with a WSL ext4 workspace |
| `needs-windows-run` | `os:windows`, `toolchain:msvc` | a Windows workstation |
| `needs-gui-check` | `gui`, `human-present` | the workstation where the owner approves GUI quality |

Rules:
- A node takes only items labelled with its lanes; it takes a fallback lane
  item only when that item has waited past the fleet's `fallbackAfter`.
- Each node keeps its own clones; no shared network folders between nodes.
- Results go back as a pull request comment or a new pull request, with the
  exact command, commit SHA and outcome.
