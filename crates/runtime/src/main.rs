use everything_codex_adapter::{AgentEvent, RunConfig, Sandbox};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::sync::{Mutex, mpsc, oneshot};
use uuid::Uuid;

const PROTOCOL_VERSION: &str = "1.0";

#[derive(Debug, Deserialize)]
struct RpcRequest {
    jsonrpc: String,
    id: Value,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct RpcErrorBody {
    code: i64,
    message: String,
    data: Value,
}

#[derive(Debug)]
struct ServiceError {
    rpc_code: i64,
    code: &'static str,
    message: String,
}

impl ServiceError {
    fn invalid(message: impl Into<String>) -> Self {
        Self {
            rpc_code: -32602,
            code: "INVALID_PARAMS",
            message: message.into(),
        }
    }

    fn domain(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            rpc_code: -32000,
            code,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum TaskStatus {
    Running,
    Stopping,
    Completed,
    Failed,
    Interrupted,
}

struct TaskEntry {
    status: TaskStatus,
    stop: Option<oneshot::Sender<()>>,
}

#[derive(Clone)]
struct TaskService {
    initialized: Arc<Mutex<bool>>,
    tasks: Arc<Mutex<HashMap<String, TaskEntry>>>,
    outgoing: mpsc::UnboundedSender<Value>,
    binary: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InitializeParams {
    protocol_version: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartParams {
    task_id: Option<String>,
    prompt: String,
    workspace: PathBuf,
    thread_id: Option<String>,
    model: Option<String>,
    #[serde(default = "default_sandbox")]
    sandbox: Sandbox,
    #[serde(default)]
    images: Vec<PathBuf>,
}

fn default_sandbox() -> Sandbox {
    Sandbox::WorkspaceWrite
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskIdParams {
    task_id: String,
}

impl TaskService {
    fn new(outgoing: mpsc::UnboundedSender<Value>) -> Self {
        let binary = env::var_os("EVERYTHING_CODEX_BIN")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("codex"));
        Self {
            initialized: Arc::new(Mutex::new(false)),
            tasks: Arc::new(Mutex::new(HashMap::new())),
            outgoing,
            binary,
        }
    }

    async fn handle(&self, method: &str, params: Value) -> Result<Value, ServiceError> {
        if method == "runtime/initialize" {
            return self.initialize(params).await;
        }
        if !*self.initialized.lock().await {
            return Err(ServiceError::domain(
                "RUNTIME_NOT_INITIALIZED",
                "runtime/initialize must be called first",
            ));
        }
        match method {
            "codex/check" => self.check_codex().await,
            "task/start" => self.start_task(params).await,
            "task/status" => self.task_status(params).await,
            "task/stop" => self.stop_task(params).await,
            "runtime/shutdown" => {
                self.shutdown().await;
                Ok(json!({ "accepted": true }))
            }
            _ => Err(ServiceError {
                rpc_code: -32601,
                code: "METHOD_NOT_FOUND",
                message: format!("unknown method: {method}"),
            }),
        }
    }

    async fn initialize(&self, params: Value) -> Result<Value, ServiceError> {
        let params: InitializeParams = serde_json::from_value(params)
            .map_err(|error| ServiceError::invalid(error.to_string()))?;
        if params.protocol_version != PROTOCOL_VERSION {
            return Err(ServiceError::domain(
                "PROTOCOL_VERSION_UNSUPPORTED",
                format!("protocol {} is unsupported", params.protocol_version),
            ));
        }
        *self.initialized.lock().await = true;
        Ok(json!({
            "runtimeVersion": env!("CARGO_PKG_VERSION"),
            "protocolVersion": PROTOCOL_VERSION,
            "platform": env::consts::OS,
            "capabilities": ["codex", "tasks", "streaming", "resume", "stop"]
        }))
    }

    async fn check_codex(&self) -> Result<Value, ServiceError> {
        let output = tokio::process::Command::new(&self.binary)
            .arg("--version")
            .output()
            .await
            .map_err(|_| ServiceError::domain("CODEX_NOT_FOUND", "Codex CLI was not found"))?;
        if !output.status.success() {
            return Err(ServiceError::domain(
                "CODEX_UNAVAILABLE",
                "Codex CLI did not pass its version check",
            ));
        }
        Ok(json!({
            "available": true,
            "binary": self.binary.to_string_lossy(),
            "version": String::from_utf8_lossy(&output.stdout).trim()
        }))
    }

    async fn start_task(&self, params: Value) -> Result<Value, ServiceError> {
        let params: StartParams = serde_json::from_value(params)
            .map_err(|error| ServiceError::invalid(error.to_string()))?;
        if params.prompt.trim().is_empty() {
            return Err(ServiceError::invalid("prompt must not be empty"));
        }
        let workspace = validate_workspace(&params.workspace)?;
        let task_id = params.task_id.unwrap_or_else(|| Uuid::new_v4().to_string());

        let mut tasks = self.tasks.lock().await;
        if tasks.contains_key(&task_id) {
            return Err(ServiceError::domain(
                "TASK_ALREADY_EXISTS",
                format!("task already exists: {task_id}"),
            ));
        }
        let config = RunConfig {
            binary: self.binary.clone(),
            workspace,
            prompt: params.prompt,
            thread_id: params.thread_id,
            model: params.model,
            sandbox: params.sandbox,
            images: params.images,
        };
        let mut child = everything_codex_adapter::spawn(&config)
            .await
            .map_err(|error| ServiceError::domain("TASK_SPAWN_FAILED", error.to_string()))?;
        let stdout = child.stdout.take().ok_or_else(|| {
            ServiceError::domain("TASK_SPAWN_FAILED", "Codex stdout was unavailable")
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            ServiceError::domain("TASK_SPAWN_FAILED", "Codex stderr was unavailable")
        })?;
        let (stop_tx, stop_rx) = oneshot::channel();
        tasks.insert(
            task_id.clone(),
            TaskEntry {
                status: TaskStatus::Running,
                stop: Some(stop_tx),
            },
        );
        drop(tasks);

        let service = self.clone();
        let spawned_task_id = task_id.clone();
        tokio::spawn(async move {
            service
                .monitor_task(spawned_task_id, child, stdout, stderr, stop_rx)
                .await;
        });
        Ok(json!({ "taskId": task_id }))
    }

    async fn monitor_task<R, E>(
        &self,
        task_id: String,
        mut child: tokio::process::Child,
        stdout: R,
        stderr: E,
        mut stop_rx: oneshot::Receiver<()>,
    ) where
        R: AsyncRead + Unpin + Send + 'static,
        E: AsyncRead + Unpin + Send + 'static,
    {
        let stderr_task = tokio::spawn(copy_stderr(stderr));
        let mut lines = BufReader::new(stdout).lines();
        let mut thread_id = None;
        let mut terminal_failure = None;
        let mut failure_emitted = false;

        loop {
            tokio::select! {
                _ = &mut stop_rx => {
                    let _ = child.kill().await;
                    self.emit(&task_id, AgentEvent::Interrupted);
                    self.finish(&task_id, TaskStatus::Interrupted).await;
                    let _ = stderr_task.await;
                    return;
                }
                line = lines.next_line() => {
                    match line {
                        Ok(Some(line)) => {
                            if let Some(event) = everything_codex_adapter::translate_line(&line) {
                                if let AgentEvent::ThreadStarted { thread_id: id } = &event {
                                    thread_id = Some(id.clone());
                                }
                                if let AgentEvent::Failed { message } = &event {
                                    terminal_failure = Some(message.clone());
                                    failure_emitted = true;
                                }
                                self.emit(&task_id, event);
                            }
                        }
                        Ok(None) => break,
                        Err(error) => {
                            terminal_failure = Some(format!("failed to read Codex output: {error}"));
                            break;
                        }
                    }
                }
            }
        }

        let status = child.wait().await;
        let stderr = stderr_task.await.unwrap_or_default();
        match status {
            Ok(exit) if exit.success() && terminal_failure.is_none() => {
                self.emit(&task_id, AgentEvent::Completed { thread_id });
                self.finish(&task_id, TaskStatus::Completed).await;
            }
            Ok(exit) => {
                let message = terminal_failure.unwrap_or_else(|| {
                    let detail = stderr.trim();
                    if detail.is_empty() {
                        format!("Codex exited with {exit}")
                    } else {
                        format!("Codex exited with {exit}: {detail}")
                    }
                });
                if !failure_emitted {
                    self.emit(&task_id, AgentEvent::Failed { message });
                }
                self.finish(&task_id, TaskStatus::Failed).await;
            }
            Err(error) => {
                self.emit(
                    &task_id,
                    AgentEvent::Failed {
                        message: error.to_string(),
                    },
                );
                self.finish(&task_id, TaskStatus::Failed).await;
            }
        }
    }

    fn emit(&self, task_id: &str, event: AgentEvent) {
        let _ = self.outgoing.send(json!({
            "jsonrpc": "2.0",
            "method": "task/event",
            "params": { "taskId": task_id, "event": event }
        }));
    }

    async fn finish(&self, task_id: &str, status: TaskStatus) {
        if let Some(entry) = self.tasks.lock().await.get_mut(task_id) {
            entry.status = status;
            entry.stop = None;
        }
    }

    async fn task_status(&self, params: Value) -> Result<Value, ServiceError> {
        let params: TaskIdParams = serde_json::from_value(params)
            .map_err(|error| ServiceError::invalid(error.to_string()))?;
        let tasks = self.tasks.lock().await;
        let entry = tasks.get(&params.task_id).ok_or_else(|| {
            ServiceError::domain(
                "TASK_NOT_FOUND",
                format!("task not found: {}", params.task_id),
            )
        })?;
        Ok(json!({ "taskId": params.task_id, "status": entry.status }))
    }

    async fn stop_task(&self, params: Value) -> Result<Value, ServiceError> {
        let params: TaskIdParams = serde_json::from_value(params)
            .map_err(|error| ServiceError::invalid(error.to_string()))?;
        let mut tasks = self.tasks.lock().await;
        let entry = tasks.get_mut(&params.task_id).ok_or_else(|| {
            ServiceError::domain(
                "TASK_NOT_FOUND",
                format!("task not found: {}", params.task_id),
            )
        })?;
        let stop = entry.stop.take().ok_or_else(|| {
            ServiceError::domain(
                "TASK_NOT_RUNNING",
                format!("task is not running: {}", params.task_id),
            )
        })?;
        entry.status = TaskStatus::Stopping;
        let _ = stop.send(());
        Ok(json!({ "taskId": params.task_id, "accepted": true }))
    }

    async fn shutdown(&self) {
        let mut tasks = self.tasks.lock().await;
        for entry in tasks.values_mut() {
            if let Some(stop) = entry.stop.take() {
                entry.status = TaskStatus::Stopping;
                let _ = stop.send(());
            }
        }
    }
}

async fn copy_stderr<R: AsyncRead + Unpin>(stderr: R) -> String {
    let mut lines = BufReader::new(stderr).lines();
    let mut captured = String::new();
    while let Ok(Some(line)) = lines.next_line().await {
        eprintln!("[codex] {line}");
        if captured.len() < 4096 {
            captured.push_str(&line);
            captured.push('\n');
        }
    }
    captured
}

fn validate_workspace(path: &Path) -> Result<PathBuf, ServiceError> {
    if !path.is_absolute() || !path.is_dir() {
        return Err(ServiceError::domain(
            "WORKSPACE_INVALID",
            "workspace must be an existing absolute directory",
        ));
    }
    let canonical = path.canonicalize().map_err(|_| {
        ServiceError::domain("WORKSPACE_INVALID", "workspace could not be resolved")
    })?;
    let home = env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
    if canonical.parent().is_none() || home.as_ref().is_some_and(|home| canonical == *home) {
        return Err(ServiceError::domain(
            "WORKSPACE_TOO_BROAD",
            "workspace may not be a filesystem or home root",
        ));
    }
    Ok(canonical)
}

fn success(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn failure(id: Value, error: ServiceError) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": RpcErrorBody {
            code: error.rpc_code,
            message: error.message,
            data: json!({ "code": error.code })
        }
    })
}

#[tokio::main]
async fn main() {
    let (outgoing_tx, mut outgoing_rx) = mpsc::unbounded_channel::<Value>();
    let writer = tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        while let Some(message) = outgoing_rx.recv().await {
            let mut line = match serde_json::to_vec(&message) {
                Ok(line) => line,
                Err(error) => {
                    eprintln!("failed to serialize response: {error}");
                    continue;
                }
            };
            line.push(b'\n');
            if stdout.write_all(&line).await.is_err() || stdout.flush().await.is_err() {
                break;
            }
        }
    });

    let service = TaskService::new(outgoing_tx.clone());
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }
        let request: RpcRequest = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(error) => {
                let _ = outgoing_tx.send(failure(
                    Value::Null,
                    ServiceError {
                        rpc_code: -32700,
                        code: "PARSE_ERROR",
                        message: error.to_string(),
                    },
                ));
                continue;
            }
        };
        let id = request.id.clone();
        let response = if request.jsonrpc != "2.0" {
            failure(id, ServiceError::invalid("jsonrpc must be 2.0"))
        } else {
            match service.handle(&request.method, request.params).await {
                Ok(result) => success(id, result),
                Err(error) => failure(id, error),
            }
        };
        let shutdown = request.method == "runtime/shutdown";
        let _ = outgoing_tx.send(response);
        if shutdown {
            break;
        }
    }
    service.shutdown().await;
    drop(outgoing_tx);
    let _ = writer.await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_relative_workspace() {
        assert_eq!(
            validate_workspace(Path::new("relative")).unwrap_err().code,
            "WORKSPACE_INVALID"
        );
    }

    #[test]
    fn response_has_stable_error_code() {
        let value = failure(json!(1), ServiceError::domain("TASK_NOT_FOUND", "missing"));
        assert_eq!(
            value.pointer("/error/data/code"),
            Some(&json!("TASK_NOT_FOUND"))
        );
    }
}
