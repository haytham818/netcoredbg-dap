use std::{fs, io::Read, path::Path};

use sha2::{Digest, Sha256};
use zed_extension_api::{
    self as zed, serde_json, Architecture, AttachRequest, DebugAdapterBinary, DebugConfig,
    DebugRequest, DebugScenario, DebugTaskDefinition, DownloadedFileType, LaunchRequest, Os,
    StartDebuggingRequestArguments, StartDebuggingRequestArgumentsRequest, Worktree,
};

const ADAPTER_NAME: &str = "netcoredbg";
const NETCOREDBG_REPOSITORY: &str = "Samsung/netcoredbg";
const NETCOREDBG_VERSION: &str = "3.1.3-1062";
const NETCOREDBG_INSTALL_DIR: &str = "netcoredbg";

struct NetcoredbgExtension {
    cached_binary_path: Option<String>,
}

impl zed::Extension for NetcoredbgExtension {
    fn new() -> Self {
        Self {
            cached_binary_path: None,
        }
    }

    fn get_dap_binary(
        &mut self,
        adapter_name: String,
        config: DebugTaskDefinition,
        user_provided_debug_adapter_path: Option<String>,
        worktree: &Worktree,
    ) -> Result<DebugAdapterBinary, String> {
        if adapter_name != ADAPTER_NAME {
            return Err(format!("unknown debug adapter: {adapter_name}"));
        }

        let config_value: serde_json::Value = serde_json::from_str(&config.config)
            .map_err(|error| format!("failed to parse debug configuration: {error}"))?;
        let request = request_kind_from_config(&config_value)?;
        let configuration = resolve_debug_configuration(config_value, worktree)?.to_string();

        let command = match user_provided_debug_adapter_path {
            Some(path) => path,
            None => self.netcoredbg_binary_path(worktree)?,
        };

        Ok(DebugAdapterBinary {
            command: Some(command),
            arguments: vec!["--interpreter=vscode".to_string()],
            envs: vec![],
            cwd: None,
            connection: None,
            request_args: StartDebuggingRequestArguments {
                configuration,
                request,
            },
        })
    }

    fn dap_request_kind(
        &mut self,
        adapter_name: String,
        config: serde_json::Value,
    ) -> Result<StartDebuggingRequestArgumentsRequest, String> {
        if adapter_name != ADAPTER_NAME {
            return Err(format!("unknown debug adapter: {adapter_name}"));
        }

        request_kind_from_config(&config)
    }

    fn dap_config_to_scenario(&mut self, config: DebugConfig) -> Result<DebugScenario, String> {
        let (scenario_config, build) = match config.request {
            DebugRequest::Launch(LaunchRequest {
                program,
                cwd,
                args,
                envs,
            }) => {
                let env = envs
                    .into_iter()
                    .map(|(key, value)| (key, serde_json::Value::String(value)))
                    .collect::<serde_json::Map<_, _>>();

                if program.ends_with(".csproj") {
                    (
                        serde_json::json!({
                            "request": "launch",
                            "project": program,
                            "cwd": cwd,
                            "args": args,
                            "env": env,
                            "stopAtEntry": config.stop_on_entry.unwrap_or(false),
                        }),
                        None,
                    )
                } else {
                    (
                        serde_json::json!({
                            "request": "launch",
                            "program": program,
                            "cwd": cwd,
                            "args": args,
                            "env": env,
                            "stopAtEntry": config.stop_on_entry.unwrap_or(false),
                        }),
                        None,
                    )
                }
            }
            DebugRequest::Attach(AttachRequest { process_id }) => {
                let mut scenario = serde_json::json!({
                    "request": "attach",
                    "stopAtEntry": config.stop_on_entry.unwrap_or(false),
                });

                if let Some(process_id) = process_id {
                    scenario["processId"] = process_id.into();
                }

                (scenario, None)
            }
        };

        Ok(DebugScenario {
            label: config.label,
            adapter: ADAPTER_NAME.to_string(),
            build,
            config: scenario_config.to_string(),
            tcp_connection: None,
        })
    }
}

impl NetcoredbgExtension {
    fn netcoredbg_binary_path(&mut self, worktree: &Worktree) -> Result<String, String> {
        if let Some(path) = &self.cached_binary_path {
            if fs::metadata(path).is_ok_and(|metadata| metadata.is_file())
                && verify_cached_netcoredbg(path).is_ok()
            {
                return Ok(path.clone());
            }
            self.cached_binary_path = None;
        }

        if let Some(path) = worktree.which(binary_name()) {
            self.cached_binary_path = Some(path.clone());
            return Ok(path);
        }

        match self.download_netcoredbg() {
            Ok(path) => Ok(path),
            Err(download_error) => worktree.which(binary_name()).ok_or_else(|| {
                format!(
                    "Could not install or find '{binary}'. Install netcoredbg from \
                     https://github.com/Samsung/netcoredbg/releases, add it to PATH, or \
                     configure a custom debug adapter path in Zed. Installation error: {download_error}",
                    binary = binary_name(),
                )
            }),
        }
    }

    fn download_netcoredbg(&mut self) -> Result<String, String> {
        let (asset_name, file_type, binary_sha256) = netcoredbg_asset()?;
        let release = zed::github_release_by_tag_name(NETCOREDBG_REPOSITORY, NETCOREDBG_VERSION)?;

        let version = safe_path_component(&release.version);
        let version_dir = format!("{NETCOREDBG_INSTALL_DIR}/{version}");
        let binary_path = format!("{version_dir}/{NETCOREDBG_INSTALL_DIR}/{}", binary_name());

        if fs::metadata(&binary_path).is_ok_and(|metadata| metadata.is_file()) {
            verify_cached_netcoredbg(&binary_path)?;
            zed::make_file_executable(&binary_path)?;
            self.cached_binary_path = Some(binary_path.clone());
            return Ok(binary_path);
        }

        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == asset_name)
            .ok_or_else(|| {
                format!(
                    "release {} does not contain an asset named {asset_name}",
                    release.version
                )
            })?;

        zed::download_file(&asset.download_url, &version_dir, file_type)?;
        verify_sha256(&binary_path, binary_sha256).map_err(|error| {
            let _ = fs::remove_dir_all(&version_dir);
            error
        })?;
        zed::make_file_executable(&binary_path)?;
        remove_old_netcoredbg_versions(&version);

        self.cached_binary_path = Some(binary_path.clone());
        Ok(binary_path)
    }
}

fn request_kind_from_config(
    config: &serde_json::Value,
) -> Result<StartDebuggingRequestArgumentsRequest, String> {
    match config.get("request").and_then(|request| request.as_str()) {
        Some("launch") => Ok(StartDebuggingRequestArgumentsRequest::Launch),
        Some("attach") => Ok(StartDebuggingRequestArgumentsRequest::Attach),
        Some(request) => Err(format!(
            "unknown debug request '{request}', expected 'launch' or 'attach'"
        )),
        None => Err("debug configuration is missing a 'request' field".to_string()),
    }
}

fn resolve_debug_configuration(
    mut config: serde_json::Value,
    worktree: &Worktree,
) -> Result<serde_json::Value, String> {
    if config.get("request").and_then(|request| request.as_str()) != Some("launch") {
        return Ok(config);
    }

    if config
        .get("program")
        .and_then(|program| program.as_str())
        .filter(|program| !program.trim().is_empty())
        .is_some()
    {
        return Ok(config);
    }

    let project = config
        .get("project")
        .and_then(|project| project.as_str())
        .map(|project| normalize_project_path(project, worktree))
        .or_else(|| infer_root_project(worktree));
    let project = project.ok_or_else(|| {
        "launch configuration must include either 'program' or a resolvable '.csproj' 'project'"
            .to_string()
    })?;

    let project_contents = worktree
        .read_text_file(&project)
        .map_err(|error| format!("failed to read project file '{project}': {error}"))?;
    let project_info = DotnetProject::from_project_file(&project, &project_contents, &config)?;
    let program = project_info.output_dll_path();

    let worktree_root = worktree.root_path();
    config["program"] = serde_json::Value::String(absolute_worktree_path(&program, &worktree_root));
    if config.get("cwd").and_then(|cwd| cwd.as_str()).is_none() {
        config["cwd"] = serde_json::Value::String(absolute_worktree_path(
            project_info.project_dir(),
            &worktree_root,
        ));
    }

    Ok(config)
}

fn infer_root_project(worktree: &Worktree) -> Option<String> {
    let root = worktree.root_path();
    let project_name = Path::new(&root).file_name()?.to_str()?;
    let project = format!("{project_name}.csproj");

    worktree.read_text_file(&project).ok()?;
    Some(project)
}

fn normalize_worktree_path(path: &str) -> String {
    let path = path.trim();
    path.strip_prefix("$ZED_WORKTREE_ROOT/")
        .or_else(|| path.strip_prefix("${ZED_WORKTREE_ROOT}/"))
        .unwrap_or(path)
        .trim_start_matches("./")
        .replace('\\', "/")
}

fn normalize_project_path(path: &str, worktree: &Worktree) -> String {
    let path = normalize_worktree_path(path);
    let root = worktree.root_path().replace('\\', "/");
    path.strip_prefix(&format!("{root}/"))
        .unwrap_or(&path)
        .to_string()
}

fn absolute_worktree_path(path: &str, worktree_root: &str) -> String {
    let path = normalize_worktree_path(path);
    let worktree_root = worktree_root.replace('\\', "/");
    let worktree_root = worktree_root.trim_end_matches('/');

    if path.is_empty() || path == "." {
        return worktree_root.to_string();
    }

    if is_absolute_path(&path) {
        return path;
    }

    if worktree_root.is_empty() {
        path
    } else {
        format!("{worktree_root}/{}", path.trim_start_matches('/'))
    }
}

fn is_absolute_path(path: &str) -> bool {
    Path::new(path).is_absolute()
        || path.starts_with('/')
        || path
            .as_bytes()
            .get(1)
            .is_some_and(|separator| *separator == b':')
            && path
                .as_bytes()
                .get(2)
                .is_some_and(|separator| *separator == b'/' || *separator == b'\\')
}

fn binary_name() -> &'static str {
    match zed::current_platform().0 {
        Os::Windows => "netcoredbg.exe",
        Os::Mac | Os::Linux => "netcoredbg",
    }
}

fn netcoredbg_asset() -> Result<(String, DownloadedFileType, &'static str), String> {
    let (os, architecture) = zed::current_platform();

    match (os, architecture) {
        (Os::Linux, Architecture::X8664) => Ok((
            "netcoredbg-linux-amd64.tar.gz".to_string(),
            DownloadedFileType::GzipTar,
            "4958ddef73adf4080841424f72ee49b7169f9f196475df0c65d61bd823704921",
        )),
        (Os::Linux, Architecture::Aarch64) => Ok((
            "netcoredbg-linux-arm64.tar.gz".to_string(),
            DownloadedFileType::GzipTar,
            "a157f67f081dc629427d15b3c4f76c4ab663271989503d0c63a60311e4a4b7d2",
        )),
        (Os::Mac, Architecture::X8664) => Ok((
            "netcoredbg-osx-amd64.tar.gz".to_string(),
            DownloadedFileType::GzipTar,
            "d3fc47b2ab894c81a8b3c8ac970c5b47ae7fc51423c1ea633ad4dece7d716020",
        )),
        (Os::Windows, Architecture::X8664) => Ok((
            "netcoredbg-win64.zip".to_string(),
            DownloadedFileType::Zip,
            "f5ee03e1f279f96ee64b9c9d53840f04a09f746301589026e9b5a1de2e6a5d3d",
        )),
        (Os::Mac, Architecture::Aarch64) => Err(
            "netcoredbg does not publish macOS arm64 release assets; install netcoredbg manually"
                .to_string(),
        ),
        _ => Err("netcoredbg does not publish release assets for this platform".to_string()),
    }
}

fn verify_sha256(path: &str, expected_sha256: &str) -> Result<(), String> {
    let mut file = fs::File::open(path)
        .map_err(|error| format!("failed to open downloaded netcoredbg binary: {error}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0; 8192];

    loop {
        let bytes_read = file
            .read(&mut buffer)
            .map_err(|error| format!("failed to read downloaded netcoredbg binary: {error}"))?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    let actual_sha256 = format!("{:x}", hasher.finalize());
    if actual_sha256 == expected_sha256 {
        Ok(())
    } else {
        Err(format!(
            "downloaded netcoredbg checksum mismatch: expected {expected_sha256}, got {actual_sha256}"
        ))
    }
}

fn verify_cached_netcoredbg(path: &str) -> Result<(), String> {
    if !path
        .replace('\\', "/")
        .starts_with(&format!("{NETCOREDBG_INSTALL_DIR}/"))
    {
        return Ok(());
    }

    let (_, _, expected_sha256) = netcoredbg_asset()?;
    verify_sha256(path, expected_sha256)
}

fn remove_old_netcoredbg_versions(current_version: &str) {
    if let Ok(entries) = fs::read_dir(NETCOREDBG_INSTALL_DIR) {
        for entry in entries.flatten() {
            if entry.file_name().to_str() != Some(current_version) {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
    }
}

fn safe_path_component(component: &str) -> String {
    let component: String = component
        .chars()
        .map(|character| match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '_' | '-' => character,
            _ => '_',
        })
        .collect();

    if component.is_empty() || component == "." || component == ".." {
        "unknown".to_string()
    } else {
        component
    }
}

#[derive(Debug)]
struct DotnetProject {
    path: String,
    assembly_name: String,
    target_framework: String,
    configuration: String,
    runtime_identifier: Option<String>,
}

impl DotnetProject {
    fn from_project_file(
        path: &str,
        contents: &str,
        config: &serde_json::Value,
    ) -> Result<Self, String> {
        let assembly_name = config
            .get("assemblyName")
            .and_then(|assembly_name| assembly_name.as_str())
            .map(ToString::to_string)
            .or_else(|| xml_tag(contents, "AssemblyName"))
            .unwrap_or_else(|| project_file_stem(path));
        let target_framework = config
            .get("targetFramework")
            .and_then(|target_framework| target_framework.as_str())
            .map(ToString::to_string)
            .or_else(|| xml_tag(contents, "TargetFramework"))
            .or_else(|| {
                xml_tag(contents, "TargetFrameworks")
                    .and_then(|frameworks| frameworks.split(';').next().map(ToString::to_string))
            })
            .ok_or_else(|| {
                format!(
                    "could not determine TargetFramework for '{path}'. Add 'targetFramework' to the debug configuration"
                )
            })?;
        let configuration = config
            .get("configuration")
            .and_then(|configuration| configuration.as_str())
            .unwrap_or("Debug")
            .to_string();
        let runtime_identifier = config
            .get("runtimeIdentifier")
            .and_then(|runtime_identifier| runtime_identifier.as_str())
            .map(ToString::to_string)
            .or_else(|| xml_tag(contents, "RuntimeIdentifier"));

        Ok(Self {
            path: path.to_string(),
            assembly_name,
            target_framework,
            configuration,
            runtime_identifier,
        })
    }

    fn project_dir(&self) -> &str {
        self.path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("")
    }

    fn output_dll_path(&self) -> String {
        let mut path = String::new();
        let project_dir = self.project_dir();
        if !project_dir.is_empty() {
            path.push_str(project_dir);
            path.push('/');
        }
        path.push_str("bin/");
        path.push_str(&self.configuration);
        path.push('/');
        path.push_str(&self.target_framework);
        path.push('/');
        if let Some(runtime_identifier) = &self.runtime_identifier {
            path.push_str(runtime_identifier);
            path.push('/');
        }
        path.push_str(&self.assembly_name);
        path.push_str(".dll");
        path
    }
}

fn project_file_stem(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .and_then(|file_stem| file_stem.to_str())
        .unwrap_or("app")
        .to_string()
}

fn xml_tag(contents: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = contents.find(&open)? + open.len();
    let end = contents[start..].find(&close)? + start;
    Some(contents[start..end].trim().to_string()).filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_project_output_path() {
        let config = serde_json::json!({});
        let project = DotnetProject::from_project_file(
            "src/MyGame/MyGame.csproj",
            r#"
            <Project Sdk="Microsoft.NET.Sdk">
              <PropertyGroup>
                <TargetFramework>net8.0</TargetFramework>
              </PropertyGroup>
            </Project>
            "#,
            &config,
        )
        .unwrap();

        assert_eq!(project.assembly_name, "MyGame");
        assert_eq!(project.project_dir(), "src/MyGame");
        assert_eq!(
            project.output_dll_path(),
            "src/MyGame/bin/Debug/net8.0/MyGame.dll"
        );
    }

    #[test]
    fn config_overrides_project_metadata() {
        let config = serde_json::json!({
            "assemblyName": "Game.Desktop",
            "targetFramework": "net9.0",
            "configuration": "Release",
            "runtimeIdentifier": "linux-x64"
        });
        let project = DotnetProject::from_project_file(
            "Game.csproj",
            "<Project><PropertyGroup><TargetFramework>net8.0</TargetFramework></PropertyGroup></Project>",
            &config,
        )
        .unwrap();

        assert_eq!(
            project.output_dll_path(),
            "bin/Release/net9.0/linux-x64/Game.Desktop.dll"
        );
    }

    #[test]
    fn release_versions_are_safe_path_components() {
        assert_eq!(safe_path_component("3.1.3-1062"), "3.1.3-1062");
        assert_eq!(safe_path_component("../bad/tag"), ".._bad_tag");
        assert_eq!(safe_path_component(".."), "unknown");
    }

    #[test]
    fn expands_worktree_relative_paths_to_absolute_paths() {
        assert_eq!(
            absolute_worktree_path(
                "MonoMario.Game/bin/Debug/net9.0/MonoMario.Game.dll",
                "/repo"
            ),
            "/repo/MonoMario.Game/bin/Debug/net9.0/MonoMario.Game.dll"
        );
        assert_eq!(
            absolute_worktree_path("$ZED_WORKTREE_ROOT/MonoMario.Game", "/repo"),
            "/repo/MonoMario.Game"
        );
        assert_eq!(absolute_worktree_path(".", "/repo"), "/repo");
    }

    #[test]
    fn preserves_existing_absolute_paths() {
        assert_eq!(
            absolute_worktree_path(
                "/repo/MonoMario.Game/bin/Debug/net9.0/MonoMario.Game.dll",
                "/repo"
            ),
            "/repo/MonoMario.Game/bin/Debug/net9.0/MonoMario.Game.dll"
        );
        assert_eq!(
            absolute_worktree_path("C:/repo/MonoMario.Game", "D:/other"),
            "C:/repo/MonoMario.Game"
        );
    }
}

zed::register_extension!(NetcoredbgExtension);
