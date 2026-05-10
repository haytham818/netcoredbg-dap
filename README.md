# netcoredbg DAP for Zed

This extension adds a `netcoredbg` debug adapter to Zed for C#/.NET projects. It starts [Samsung netcoredbg](https://github.com/Samsung/netcoredbg) in VS Code DAP mode:

```sh
netcoredbg --interpreter=vscode
```

## Requirements

Install `netcoredbg` and make sure the `netcoredbg` executable is available on the PATH seen by Zed, or configure a custom debug adapter path in Zed.

## Example `debug.json`

Build your project first, then point `program` at the built DLL:

```json
[
  {
    "adapter": "netcoredbg",
    "label": "Debug .NET app",
    "request": "launch",
    "program": "$ZED_WORKTREE_ROOT/bin/Debug/net8.0/MyGame.dll",
    "cwd": "$ZED_WORKTREE_ROOT",
    "args": [],
    "env": {},
    "stopAtEntry": false
  }
]
```

Attach to an existing .NET process:

```json
[
  {
    "adapter": "netcoredbg",
    "label": "Attach .NET process",
    "request": "attach",
    "processId": 12345
  }
]
```

## Local development

In Zed, run `zed: install dev extension` and select this repository.
