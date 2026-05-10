use zed_extension_api::{
    self as zed, serde_json, AttachRequest, DebugAdapterBinary, DebugConfig, DebugRequest,
    DebugScenario, DebugTaskDefinition, LaunchRequest, StartDebuggingRequestArguments,
    StartDebuggingRequestArgumentsRequest, Worktree,
};

const ADAPTER_NAME: &str = "netcoredbg";

struct NetcoredbgExtension;

impl zed::Extension for NetcoredbgExtension {
    fn new() -> Self {
        Self
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

        let command = match user_provided_debug_adapter_path {
            Some(path) => path,
            None => worktree.which(ADAPTER_NAME).ok_or_else(|| {
                format!(
                    "Could not find '{ADAPTER_NAME}' on PATH. Install netcoredbg from \
                     https://github.com/Samsung/netcoredbg/releases or configure a custom \
                     debug adapter path in Zed."
                )
            })?,
        };

        Ok(DebugAdapterBinary {
            command: Some(command),
            arguments: vec!["--interpreter=vscode".to_string()],
            envs: vec![],
            cwd: None,
            connection: None,
            request_args: StartDebuggingRequestArguments {
                configuration: config.config,
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
        let scenario_config = match config.request {
            DebugRequest::Launch(LaunchRequest {
                program,
                cwd,
                args,
                envs,
            }) => serde_json::json!({
                "request": "launch",
                "program": program,
                "cwd": cwd,
                "args": args,
                "env": envs
                    .into_iter()
                    .map(|(key, value)| (key, serde_json::Value::String(value)))
                    .collect::<serde_json::Map<_, _>>(),
                "stopAtEntry": config.stop_on_entry.unwrap_or(false),
            }),
            DebugRequest::Attach(AttachRequest { process_id }) => {
                let mut scenario = serde_json::json!({
                    "request": "attach",
                    "stopAtEntry": config.stop_on_entry.unwrap_or(false),
                });

                if let Some(process_id) = process_id {
                    scenario["processId"] = process_id.into();
                }

                scenario
            }
        };

        Ok(DebugScenario {
            label: config.label,
            adapter: ADAPTER_NAME.to_string(),
            build: None,
            config: scenario_config.to_string(),
            tcp_connection: None,
        })
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

zed::register_extension!(NetcoredbgExtension);
