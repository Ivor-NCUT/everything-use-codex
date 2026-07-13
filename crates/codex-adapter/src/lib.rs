use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Sandbox {
    ReadOnly,
    WorkspaceWrite,
    DangerFullAccess,
}

impl Sandbox {
    fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::WorkspaceWrite => "workspace-write",
            Self::DangerFullAccess => "danger-full-access",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RunConfig {
    pub binary: PathBuf,
    pub workspace: PathBuf,
    pub prompt: String,
    pub thread_id: Option<String>,
    pub model: Option<String>,
    pub sandbox: Sandbox,
    pub images: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    ThreadStarted {
        thread_id: String,
    },
    TextDelta {
        text: String,
    },
    ToolStarted {
        id: String,
        name: String,
        input: Value,
    },
    ToolCompleted {
        id: String,
        output: String,
        is_error: bool,
    },
    Usage {
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
        cached_input_tokens: Option<u64>,
        reasoning_output_tokens: Option<u64>,
    },
    Completed {
        thread_id: Option<String>,
    },
    Failed {
        message: String,
    },
    Interrupted,
}

#[derive(Debug, Error)]
pub enum AdapterError {
    #[error("failed to start Codex: {0}")]
    Spawn(#[source] std::io::Error),
    #[error("Codex stdin was unavailable")]
    MissingStdin,
    #[error("failed to write prompt to Codex: {0}")]
    WritePrompt(#[source] std::io::Error),
}

pub fn build_args(config: &RunConfig) -> Vec<String> {
    let mut common = vec![
        "--sandbox".into(),
        config.sandbox.as_str().into(),
        "--skip-git-repo-check".into(),
        "-C".into(),
        config.workspace.to_string_lossy().into_owned(),
    ];
    if let Some(model) = &config.model {
        common.extend(["--model".into(), model.clone()]);
    }

    let image_args = config
        .images
        .iter()
        .flat_map(|path| ["--image".into(), path.to_string_lossy().into_owned()])
        .collect::<Vec<_>>();

    if let Some(thread_id) = &config.thread_id {
        let mut args = vec!["exec".into()];
        args.append(&mut common);
        args.extend(["resume".into(), "--json".into()]);
        args.extend(image_args);
        args.extend([thread_id.clone(), "-".into()]);
        args
    } else {
        let mut args = vec!["exec".into(), "--json".into()];
        args.append(&mut common);
        args.extend(image_args);
        args.push("-".into());
        args
    }
}

pub async fn spawn(config: &RunConfig) -> Result<Child, AdapterError> {
    let args = build_args(config);
    let mut child = Command::new(&config.binary)
        .args(args)
        .current_dir(&config.workspace)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(AdapterError::Spawn)?;

    let mut stdin = child.stdin.take().ok_or(AdapterError::MissingStdin)?;
    stdin
        .write_all(config.prompt.as_bytes())
        .await
        .map_err(AdapterError::WritePrompt)?;
    stdin.shutdown().await.map_err(AdapterError::WritePrompt)?;
    Ok(child)
}

pub fn translate_line(line: &str) -> Option<AgentEvent> {
    let raw: Value = serde_json::from_str(line).ok()?;
    match raw.get("type")?.as_str()? {
        "thread.started" => string_at(&raw, &["thread_id", "threadId"])
            .map(|thread_id| AgentEvent::ThreadStarted { thread_id }),
        "agent_message" => {
            string_at(&raw, &["message", "text"]).map(|text| AgentEvent::TextDelta { text })
        }
        "item.started" => translate_item_started(&raw),
        "item.completed" => translate_item_completed(&raw),
        "turn.completed" => Some(AgentEvent::Usage {
            input_tokens: number_at(&raw, &["usage", "input_tokens"]),
            output_tokens: number_at(&raw, &["usage", "output_tokens"]),
            cached_input_tokens: number_at(&raw, &["usage", "cached_input_tokens"]),
            reasoning_output_tokens: number_at(&raw, &["usage", "reasoning_output_tokens"]),
        }),
        "turn.failed" => Some(AgentEvent::Failed {
            message: error_message(&raw, "Codex turn failed"),
        }),
        _ => None,
    }
}

fn translate_item_started(raw: &Value) -> Option<AgentEvent> {
    let item = raw.get("item")?;
    if item.get("type")?.as_str()? != "command_execution" {
        return None;
    }
    Some(AgentEvent::ToolStarted {
        id: item.get("id")?.as_str()?.to_owned(),
        name: "command_execution".into(),
        input: serde_json::json!({ "command": item.get("command").and_then(Value::as_str).unwrap_or("") }),
    })
}

fn translate_item_completed(raw: &Value) -> Option<AgentEvent> {
    let item = raw.get("item")?;
    match item.get("type")?.as_str()? {
        "agent_message" => {
            string_at(item, &["text", "message"]).map(|text| AgentEvent::TextDelta { text })
        }
        "command_execution" => Some(AgentEvent::ToolCompleted {
            id: item.get("id")?.as_str()?.to_owned(),
            output: string_at(item, &["output", "aggregated_output", "stdout"]).unwrap_or_default(),
            is_error: item
                .get("exit_code")
                .and_then(Value::as_i64)
                .is_some_and(|code| code != 0),
        }),
        _ => None,
    }
}

fn string_at(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .map(str::to_owned)
}

fn number_at(value: &Value, path: &[&str]) -> Option<u64> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    current.as_u64()
}

fn error_message(raw: &Value, fallback: &str) -> String {
    raw.get("message")
        .and_then(Value::as_str)
        .or_else(|| raw.pointer("/error/message").and_then(Value::as_str))
        .unwrap_or(fallback)
        .to_owned()
}

pub fn is_absolute_existing_directory(path: &Path) -> bool {
    path.is_absolute() && path.is_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(thread_id: Option<&str>) -> RunConfig {
        RunConfig {
            binary: "codex".into(),
            workspace: "/tmp/project".into(),
            prompt: "hello".into(),
            thread_id: thread_id.map(str::to_owned),
            model: None,
            sandbox: Sandbox::WorkspaceWrite,
            images: vec![],
        }
    }

    #[test]
    fn builds_new_task_args() {
        assert_eq!(
            build_args(&config(None)),
            [
                "exec",
                "--json",
                "--sandbox",
                "workspace-write",
                "--skip-git-repo-check",
                "-C",
                "/tmp/project",
                "-"
            ]
        );
    }

    #[test]
    fn builds_resume_args() {
        assert_eq!(
            build_args(&config(Some("thread-1"))),
            [
                "exec",
                "--sandbox",
                "workspace-write",
                "--skip-git-repo-check",
                "-C",
                "/tmp/project",
                "resume",
                "--json",
                "thread-1",
                "-"
            ]
        );
    }

    #[test]
    fn translates_text_and_tool_events() {
        assert_eq!(
            translate_line(
                r#"{"type":"item.completed","item":{"type":"agent_message","text":"done"}}"#
            ),
            Some(AgentEvent::TextDelta {
                text: "done".into()
            })
        );
        assert!(matches!(
            translate_line(
                r#"{"type":"item.started","item":{"type":"command_execution","id":"1","command":"pwd"}}"#
            ),
            Some(AgentEvent::ToolStarted { .. })
        ));
    }
}
