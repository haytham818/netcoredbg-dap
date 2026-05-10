# netcoredbg DAP for Zed

This extension adds a `netcoredbg` debug adapter to Zed for C#/.NET projects. It starts [Samsung netcoredbg](https://github.com/Samsung/netcoredbg) in VS Code DAP mode:

```sh
netcoredbg --interpreter=vscode
```

## netcoredbg installation

The extension first uses a custom debug adapter path if you configured one in Zed, then checks for `netcoredbg` on PATH. If it cannot find one, it detects the current platform and downloads the matching asset from the pinned `Samsung/netcoredbg` `3.1.3-1062` GitHub Release into the extension cache. The downloaded binary is verified with a built-in SHA-256 checksum before use.

Currently supported upstream assets are:

- Linux x64: `netcoredbg-linux-amd64.tar.gz`
- Linux arm64: `netcoredbg-linux-arm64.tar.gz`
- macOS x64: `netcoredbg-osx-amd64.tar.gz`
- Windows x64: `netcoredbg-win64.zip`

If Samsung does not publish an asset for your platform, install `netcoredbg` manually and put it on PATH or configure a custom debug adapter path in Zed.

## Automatic project debugging

You can point the debug configuration at a `.csproj` instead of a built DLL. The extension reads `TargetFramework`, `AssemblyName`, and related project metadata, then resolves the DLL under `bin/<Configuration>/<TargetFramework>/` automatically. This resolves the launch target; use the task locator below when you also want Zed to build before debugging.

```json
[
  {
    "adapter": "netcoredbg",
    "label": "Debug project",
    "request": "launch",
    "project": "$ZED_WORKTREE_ROOT/MyGame.csproj",
    "configuration": "Debug",
    "cwd": "$ZED_WORKTREE_ROOT",
    "args": [],
    "env": {},
    "stopAtEntry": false
  }
]
```

If the project file is named the same as the worktree root directory and is located at the root, `project` can be omitted and the extension will try to infer it.

## Automatic build from Zed tasks

The extension registers a `dotnet` debug locator. Zed can convert matching `dotnet run` or `dotnet build` tasks with an explicit `.csproj` path into debug sessions that run `dotnet build` before launching netcoredbg.

The locator understands task arguments such as:

```sh
dotnet run --project src/MyGame/MyGame.csproj -c Debug -f net8.0
dotnet build src/MyGame/MyGame.csproj --configuration Debug --framework net8.0
```

## Example `debug.json`

You can still point `program` directly at an already built DLL:

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
