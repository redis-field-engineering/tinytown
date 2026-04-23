/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Agent runtime adapters.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use async_trait::async_trait;
use futures_core::Stream;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child as TokioChild, ChildStdin, ChildStdout, Command as TokioCommand};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::Result;

#[must_use]
pub fn supports_persistent_runtime(cli_name: &str) -> bool {
    matches!(cli_name, "codex" | "codex-mini")
}

#[must_use]
pub fn build_cli_command(cli_name: &str, cli_cmd: &str, prompt_file: &Path) -> String {
    let prompt_file = shell_quote(prompt_file);
    if cli_name == "auggie" {
        format!("{} --instruction-file {}", cli_cmd, prompt_file)
    } else {
        format!("cat {} | {}", prompt_file, cli_cmd)
    }
}

#[must_use]
fn shell_quote(path: &Path) -> String {
    shell_quote_str(&path.to_string_lossy())
}

#[must_use]
fn shell_quote_str(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

#[derive(Debug, Clone)]
pub struct AgentTurn {
    pub prompt: String,
    pub prompt_file: PathBuf,
    pub output_file: PathBuf,
}

#[derive(Debug, Clone)]
pub enum AgentInput {
    UserMessage(AgentTurn),
    UrgentMessage(AgentTurn),
    Cancel,
}

#[derive(Debug, Clone)]
pub enum AgentEvent {
    SessionReady {
        session_id: String,
    },
    TurnStarted,
    AssistantDelta(String),
    ToolCall {
        name: String,
        args: serde_json::Value,
    },
    TurnCompleted {
        summary: Option<String>,
    },
    AwaitingInput,
    SessionError(String),
    Exited(ExitStatus),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownMode {
    Graceful,
    Immediate,
}

#[derive(Debug)]
pub struct AgentTurnResult {
    pub status: std::io::Result<ExitStatus>,
}

pub type AgentEventStream<'a> = Pin<Box<dyn Stream<Item = AgentEvent> + Send + 'a>>;

pub struct AgentEventReceiver {
    rx: mpsc::UnboundedReceiver<AgentEvent>,
}

impl AgentEventReceiver {
    pub async fn recv(&mut self) -> Option<AgentEvent> {
        self.rx.recv().await
    }
}

pub struct AgentRuntime {
    pub agent: RuntimeAgent,
    pub events: AgentEventReceiver,
}

pub struct RuntimeAgent {
    inner: Box<dyn CodingAgent>,
    event_tx: mpsc::UnboundedSender<AgentEvent>,
}

impl RuntimeAgent {
    fn new(inner: Box<dyn CodingAgent>, event_tx: mpsc::UnboundedSender<AgentEvent>) -> Self {
        Self { inner, event_tx }
    }

    pub async fn send(&mut self, input: AgentInput) -> Result<AgentTurnResult> {
        let result = self.inner.send(input).await;
        self.flush_events();
        result
    }

    pub async fn shutdown(&mut self, mode: ShutdownMode) -> Result<Option<ExitStatus>> {
        let result = self.inner.shutdown(mode).await;
        self.flush_events();
        result
    }

    #[must_use]
    pub fn session_id(&self) -> Option<String> {
        self.inner.session_id()
    }

    fn flush_events(&mut self) {
        for event in self.inner.take_events() {
            let _ = self.event_tx.send(event);
        }
    }
}

#[async_trait]
pub trait CodingAgent: Send {
    async fn send(&mut self, input: AgentInput) -> Result<AgentTurnResult>;
    fn events(&mut self) -> AgentEventStream<'_>;
    fn take_events(&mut self) -> Vec<AgentEvent>;
    async fn shutdown(&mut self, mode: ShutdownMode) -> Result<Option<ExitStatus>>;

    fn session_id(&self) -> Option<String> {
        None
    }
}

#[derive(Debug, Clone)]
pub struct OneShotAgentConfig {
    pub cli_name: String,
    pub cli_cmd: String,
    pub workdir: PathBuf,
    pub env: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct StreamingAgentConfig {
    pub cli_name: String,
    pub cli_cmd: String,
    pub workdir: PathBuf,
    pub env: Vec<(String, String)>,
    pub resume_session_id: Option<String>,
}

#[derive(Debug)]
pub struct OneShotAgent {
    config: OneShotAgentConfig,
    events: VecDeque<AgentEvent>,
    last_exit_status: Option<ExitStatus>,
}

impl OneShotAgent {
    #[must_use]
    pub fn new(config: OneShotAgentConfig) -> Self {
        Self {
            config,
            events: VecDeque::new(),
            last_exit_status: None,
        }
    }

    #[must_use]
    pub fn runtime(config: OneShotAgentConfig) -> AgentRuntime {
        let (tx, rx) = mpsc::unbounded_channel();
        AgentRuntime {
            agent: RuntimeAgent::new(Box::new(Self::new(config)), tx),
            events: AgentEventReceiver { rx },
        }
    }

    fn run_turn(&mut self, turn: AgentTurn) -> Result<AgentTurnResult> {
        self.events.push_back(AgentEvent::TurnStarted);
        std::fs::write(&turn.prompt_file, &turn.prompt)?;

        let status = (|| -> std::io::Result<ExitStatus> {
            let output = std::fs::File::create(&turn.output_file)?;
            let shell_cmd = build_cli_command(
                &self.config.cli_name,
                &self.config.cli_cmd,
                &turn.prompt_file,
            );
            let mut cmd = Command::new("sh");
            cmd.arg("-c")
                .arg(&shell_cmd)
                .current_dir(&self.config.workdir)
                .stdin(Stdio::null())
                .stdout(output.try_clone()?)
                .stderr(output);

            for (key, value) in &self.config.env {
                cmd.env(key, value);
            }

            cmd.status()
        })();

        let _ = std::fs::remove_file(&turn.prompt_file);

        match &status {
            Ok(exit_status) => {
                self.last_exit_status = Some(exit_status.clone());
                self.events
                    .push_back(AgentEvent::Exited(exit_status.clone()));
                if exit_status.success() {
                    self.events
                        .push_back(AgentEvent::TurnCompleted { summary: None });
                } else {
                    self.events.push_back(AgentEvent::SessionError(format!(
                        "CLI exited with status {}",
                        describe_exit_status(exit_status)
                    )));
                }
            }
            Err(err) => {
                self.events.push_back(AgentEvent::SessionError(format!(
                    "Failed to run CLI: {}",
                    err
                )));
            }
        }

        Ok(AgentTurnResult { status })
    }
}

#[async_trait]
impl CodingAgent for OneShotAgent {
    async fn send(&mut self, input: AgentInput) -> Result<AgentTurnResult> {
        match input {
            AgentInput::UserMessage(turn) | AgentInput::UrgentMessage(turn) => self.run_turn(turn),
            AgentInput::Cancel => {
                self.events.push_back(AgentEvent::SessionError(
                    "Cancel is not supported by the one-shot runtime".to_string(),
                ));
                Ok(AgentTurnResult {
                    status: Err(std::io::Error::other(
                        "Cancel is not supported by the one-shot runtime",
                    )),
                })
            }
        }
    }

    fn events(&mut self) -> AgentEventStream<'_> {
        Box::pin(BufferedEventStream {
            events: &mut self.events,
        })
    }

    fn take_events(&mut self) -> Vec<AgentEvent> {
        self.events.drain(..).collect()
    }

    async fn shutdown(&mut self, _mode: ShutdownMode) -> Result<Option<ExitStatus>> {
        Ok(self.last_exit_status.clone())
    }
}

#[derive(Debug, Clone)]
struct CodexAppServerLaunchConfig {
    launch_cmd: String,
    workdir: PathBuf,
    env: Vec<(String, String)>,
    resume_thread_id: Option<String>,
    model: Option<String>,
    reasoning_effort: Option<String>,
}

impl CodexAppServerLaunchConfig {
    fn from_streaming(config: StreamingAgentConfig) -> Self {
        let (model, reasoning_effort) = codex_model_overrides(&config.cli_name);
        Self {
            launch_cmd: build_persistent_launch_command(&config.cli_name, &config.cli_cmd),
            workdir: config.workdir,
            env: config.env,
            resume_thread_id: config.resume_session_id,
            model,
            reasoning_effort,
        }
    }
}

#[derive(Debug, Default)]
struct PendingRequestState {
    next_id: u64,
    pending: HashMap<u64, oneshot::Sender<std::io::Result<Value>>>,
}

#[derive(Debug)]
pub struct CodexAppServerAgent {
    config: CodexAppServerLaunchConfig,
    child: TokioChild,
    stdin: Option<ChildStdin>,
    events: VecDeque<AgentEvent>,
    last_exit_status: Option<ExitStatus>,
    thread_id: Arc<Mutex<Option<String>>>,
    active_turn_id: Arc<Mutex<Option<String>>>,
    output_files: Arc<Mutex<HashMap<String, PathBuf>>>,
    request_state: Arc<Mutex<PendingRequestState>>,
    event_tx: mpsc::UnboundedSender<AgentEvent>,
    reader_task: JoinHandle<()>,
}

impl CodexAppServerAgent {
    async fn spawn(
        config: CodexAppServerLaunchConfig,
        event_tx: mpsc::UnboundedSender<AgentEvent>,
    ) -> Result<Self> {
        let mut cmd = TokioCommand::new("sh");
        cmd.arg("-lc")
            .arg(&config.launch_cmd)
            .current_dir(&config.workdir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        for (key, value) in &config.env {
            cmd.env(key, value);
        }

        let mut child = cmd.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| std::io::Error::other("codex app-server stdin unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| std::io::Error::other("codex app-server stdout unavailable"))?;

        let thread_id = Arc::new(Mutex::new(None));
        let active_turn_id = Arc::new(Mutex::new(None));
        let output_files = Arc::new(Mutex::new(HashMap::new()));
        let request_state = Arc::new(Mutex::new(PendingRequestState::default()));
        let reader_task = spawn_codex_reader(
            stdout,
            event_tx.clone(),
            thread_id.clone(),
            active_turn_id.clone(),
            output_files.clone(),
            request_state.clone(),
        );

        let mut agent = Self {
            config,
            child,
            stdin: Some(stdin),
            events: VecDeque::new(),
            last_exit_status: None,
            thread_id,
            active_turn_id,
            output_files,
            request_state,
            event_tx,
            reader_task,
        };

        agent.initialize().await?;
        let thread_id = agent.start_or_resume_thread().await?;
        let _ = agent.event_tx.send(AgentEvent::SessionReady {
            session_id: thread_id,
        });

        Ok(agent)
    }

    async fn initialize(&mut self) -> Result<()> {
        let params = json!({
            "clientInfo": {
                "name": "tinytown",
                "title": "Tinytown",
                "version": env!("CARGO_PKG_VERSION"),
            }
        });
        let _ = self.send_request("initialize", params).await?;
        self.send_notification("initialized", json!({})).await?;
        Ok(())
    }

    async fn start_or_resume_thread(&mut self) -> Result<String> {
        let params = json!({
            "approvalPolicy": "never",
            "cwd": self.config.workdir,
            "sandbox": "danger-full-access",
            "model": self.config.model,
        });

        let result = if let Some(thread_id) = self.config.resume_thread_id.as_ref() {
            let mut resume = params;
            if let Some(object) = resume.as_object_mut() {
                object.insert("threadId".to_string(), Value::String(thread_id.clone()));
            }
            self.send_request("thread/resume", resume).await?
        } else {
            self.send_request("thread/start", params).await?
        };

        let thread_id = result
            .get("thread")
            .and_then(|thread| thread.get("id"))
            .and_then(Value::as_str)
            .ok_or_else(|| std::io::Error::other("codex app-server thread id missing"))?
            .to_string();
        *self.thread_id.lock().expect("thread id mutex poisoned") = Some(thread_id.clone());
        Ok(thread_id)
    }

    async fn send_request(&mut self, method: &str, params: Value) -> Result<Value> {
        let (request_id, rx) = {
            let mut state = self
                .request_state
                .lock()
                .expect("request state mutex poisoned");
            state.next_id += 1;
            let request_id = state.next_id;
            let (tx, rx) = oneshot::channel();
            state.pending.insert(request_id, tx);
            (request_id, rx)
        };

        self.write_json(&json!({
            "id": request_id,
            "method": method,
            "params": params,
        }))
        .await?;

        match rx.await {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(err)) => Err(err.into()),
            Err(_) => Err(std::io::Error::other(format!(
                "codex app-server request dropped before response: {}",
                method
            ))
            .into()),
        }
    }

    async fn send_notification(&mut self, method: &str, params: Value) -> Result<()> {
        self.write_json(&json!({
            "method": method,
            "params": params,
        }))
        .await
    }

    async fn write_json(&mut self, payload: &Value) -> Result<()> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| std::io::Error::other("codex app-server stdin closed"))?;
        stdin.write_all(payload.to_string().as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;
        Ok(())
    }

    async fn start_turn(&mut self, turn: AgentTurn) -> Result<AgentTurnResult> {
        std::fs::write(&turn.prompt_file, &turn.prompt)?;

        let mut params = json!({
            "approvalPolicy": "never",
            "cwd": self.config.workdir,
            "threadId": self
                .session_id()
                .ok_or_else(|| std::io::Error::other("codex thread id missing"))?,
            "input": [{"type": "text", "text": turn.prompt}],
        });
        if let Some(object) = params.as_object_mut() {
            if let Some(model) = self.config.model.as_ref() {
                object.insert("model".to_string(), Value::String(model.clone()));
            }
            if let Some(effort) = self.config.reasoning_effort.as_ref() {
                object.insert("effort".to_string(), Value::String(effort.clone()));
            }
            object.insert(
                "sandboxPolicy".to_string(),
                Value::String("danger-full-access".to_string()),
            );
        }

        let result = self.send_request("turn/start", params).await;
        let _ = std::fs::remove_file(&turn.prompt_file);

        match result {
            Ok(payload) => {
                let turn_id = payload
                    .get("turn")
                    .and_then(|value| value.get("id"))
                    .and_then(Value::as_str)
                    .ok_or_else(|| std::io::Error::other("codex turn id missing"))?
                    .to_string();
                *self
                    .active_turn_id
                    .lock()
                    .expect("active turn mutex poisoned") = Some(turn_id.clone());
                queue_output_file(&self.output_files, &turn_id, turn.output_file)?;
                Ok(AgentTurnResult {
                    status: Ok(success_exit_status()),
                })
            }
            Err(err) => {
                self.events.push_back(AgentEvent::SessionError(format!(
                    "Failed to start Codex turn: {}",
                    err
                )));
                Ok(AgentTurnResult {
                    status: Err(as_io_error(err)),
                })
            }
        }
    }

    async fn steer_turn(&mut self, turn: AgentTurn, turn_id: String) -> Result<AgentTurnResult> {
        std::fs::write(&turn.prompt_file, &turn.prompt)?;
        let result = self
            .send_request(
                "turn/steer",
                json!({
                    "threadId": self
                        .session_id()
                        .ok_or_else(|| std::io::Error::other("codex thread id missing"))?,
                    "expectedTurnId": turn_id,
                    "input": [{"type": "text", "text": turn.prompt}],
                }),
            )
            .await;
        let _ = std::fs::remove_file(&turn.prompt_file);

        match result {
            Ok(_) => Ok(AgentTurnResult {
                status: Ok(success_exit_status()),
            }),
            Err(err) => {
                self.events.push_back(AgentEvent::SessionError(format!(
                    "Failed to steer Codex turn: {}",
                    err
                )));
                Ok(AgentTurnResult {
                    status: Err(as_io_error(err)),
                })
            }
        }
    }

    async fn interrupt_active_turn(&mut self) -> Result<()> {
        let Some(thread_id) = self.session_id() else {
            return Ok(());
        };
        let Some(turn_id) = self
            .active_turn_id
            .lock()
            .expect("active turn mutex poisoned")
            .clone()
        else {
            return Ok(());
        };

        let _ = self
            .send_request(
                "turn/interrupt",
                json!({
                    "threadId": thread_id,
                    "turnId": turn_id,
                }),
            )
            .await?;
        Ok(())
    }
}

fn spawn_codex_reader(
    stdout: ChildStdout,
    event_tx: mpsc::UnboundedSender<AgentEvent>,
    thread_id: Arc<Mutex<Option<String>>>,
    active_turn_id: Arc<Mutex<Option<String>>>,
    output_files: Arc<Mutex<HashMap<String, PathBuf>>>,
    request_state: Arc<Mutex<PendingRequestState>>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();

        while let Ok(Some(line)) = lines.next_line().await {
            handle_codex_line(
                &line,
                &event_tx,
                &thread_id,
                &active_turn_id,
                &output_files,
                &request_state,
            );
        }

        fail_pending_requests(
            &request_state,
            "codex app-server closed before sending a response",
        );
    })
}

fn handle_codex_line(
    line: &str,
    event_tx: &mpsc::UnboundedSender<AgentEvent>,
    thread_id: &Arc<Mutex<Option<String>>>,
    active_turn_id: &Arc<Mutex<Option<String>>>,
    output_files: &Arc<Mutex<HashMap<String, PathBuf>>>,
    request_state: &Arc<Mutex<PendingRequestState>>,
) {
    let Ok(payload) = serde_json::from_str::<Value>(line) else {
        let _ = event_tx.send(AgentEvent::SessionError(format!(
            "Invalid codex app-server event: {}",
            line
        )));
        return;
    };

    if let Some(id) = payload.get("id").and_then(Value::as_u64) {
        let response = if let Some(result) = payload.get("result") {
            Ok(result.clone())
        } else if let Some(err) = payload.get("error") {
            let message = err
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown JSON-RPC error");
            Err(std::io::Error::other(message.to_string()))
        } else {
            Err(std::io::Error::other("missing JSON-RPC result"))
        };

        if let Some(tx) = request_state
            .lock()
            .expect("request state mutex poisoned")
            .pending
            .remove(&id)
        {
            let _ = tx.send(response);
        }
        return;
    }

    let Some(method) = payload.get("method").and_then(Value::as_str) else {
        return;
    };
    let params = payload.get("params").unwrap_or(&Value::Null);

    match method {
        "thread/started" => {
            if let Some(id) = params
                .get("thread")
                .and_then(|thread| thread.get("id"))
                .and_then(Value::as_str)
            {
                *thread_id.lock().expect("thread id mutex poisoned") = Some(id.to_string());
            }
        }
        "turn/started" => {
            if let Some(id) = params
                .get("turn")
                .and_then(|turn| turn.get("id"))
                .and_then(Value::as_str)
            {
                *active_turn_id.lock().expect("active turn mutex poisoned") = Some(id.to_string());
            }
            let _ = event_tx.send(AgentEvent::TurnStarted);
        }
        "item/agentMessage/delta" => {
            if let Some(delta) = params.get("delta").and_then(Value::as_str) {
                if let Some(turn_id) = params.get("turnId").and_then(Value::as_str) {
                    append_to_turn_output(output_files, turn_id, delta);
                }
                let _ = event_tx.send(AgentEvent::AssistantDelta(delta.to_string()));
            }
        }
        "item/completed" => {
            if let Some((name, args)) =
                extract_tool_call(params.get("item").unwrap_or(&Value::Null))
            {
                let _ = event_tx.send(AgentEvent::ToolCall { name, args });
            }
        }
        "turn/completed" => {
            let turn_id = params
                .get("turn")
                .and_then(|turn| turn.get("id"))
                .and_then(Value::as_str)
                .map(str::to_string);
            if let Some(turn_id) = turn_id.as_ref() {
                finish_turn_output(output_files, turn_id);
                let mut active = active_turn_id.lock().expect("active turn mutex poisoned");
                if active.as_deref() == Some(turn_id.as_str()) {
                    *active = None;
                }
            }

            let summary = params
                .get("turn")
                .and_then(extract_turn_summary)
                .or_else(|| turn_id.map(|id| format!("turn {}", id)));
            let _ = event_tx.send(AgentEvent::TurnCompleted { summary });
            let _ = event_tx.send(AgentEvent::AwaitingInput);
        }
        _ => {}
    }
}

fn fail_pending_requests(request_state: &Arc<Mutex<PendingRequestState>>, message: &str) {
    let pending = {
        let mut state = request_state.lock().expect("request state mutex poisoned");
        std::mem::take(&mut state.pending)
    };

    for (_, tx) in pending {
        let _ = tx.send(Err(std::io::Error::other(message.to_string())));
    }
}

fn extract_tool_call(item: &Value) -> Option<(String, Value)> {
    match item.get("type").and_then(Value::as_str) {
        Some("commandExecution") => Some((
            item.get("command")
                .and_then(Value::as_str)
                .unwrap_or("commandExecution")
                .to_string(),
            json!({
                "status": item.get("status").and_then(Value::as_str),
            }),
        )),
        Some("mcpToolCall") => {
            let server = item.get("server").and_then(Value::as_str).unwrap_or("mcp");
            let tool = item.get("tool").and_then(Value::as_str).unwrap_or("tool");
            Some((
                format!("{}:{}", server, tool),
                item.get("arguments").cloned().unwrap_or(Value::Null),
            ))
        }
        _ => None,
    }
}

fn extract_turn_summary(turn: &Value) -> Option<String> {
    if let Some(summary) = turn.get("summary").and_then(Value::as_str)
        && !summary.trim().is_empty()
    {
        return Some(summary.trim().to_string());
    }

    turn.get("status")
        .and_then(Value::as_str)
        .map(|status| status.to_string())
}

fn queue_output_file(
    output_files: &Arc<Mutex<HashMap<String, PathBuf>>>,
    turn_id: &str,
    output_file: PathBuf,
) -> std::io::Result<()> {
    let _ = std::fs::File::create(&output_file)?;
    output_files
        .lock()
        .expect("output file mutex poisoned")
        .insert(turn_id.to_string(), output_file);
    Ok(())
}

fn append_to_turn_output(
    output_files: &Arc<Mutex<HashMap<String, PathBuf>>>,
    turn_id: &str,
    text: &str,
) {
    let path = output_files
        .lock()
        .expect("output file mutex poisoned")
        .get(turn_id)
        .cloned();
    if let Some(path) = path
        && let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(path)
    {
        let _ = std::io::Write::write_all(&mut file, text.as_bytes());
    }
}

fn finish_turn_output(output_files: &Arc<Mutex<HashMap<String, PathBuf>>>, turn_id: &str) {
    output_files
        .lock()
        .expect("output file mutex poisoned")
        .remove(turn_id);
}

#[must_use]
fn build_persistent_launch_command(cli_name: &str, cli_cmd: &str) -> String {
    if supports_persistent_runtime(cli_name) && cli_cmd.trim_start().starts_with("codex") {
        "codex app-server --listen stdio://".to_string()
    } else {
        cli_cmd.to_string()
    }
}

#[must_use]
fn codex_model_overrides(cli_name: &str) -> (Option<String>, Option<String>) {
    match cli_name {
        "codex-mini" => (Some("gpt-5.4-mini".to_string()), Some("medium".to_string())),
        _ => (None, None),
    }
}

pub struct StreamingAgent;

impl StreamingAgent {
    pub async fn runtime(config: StreamingAgentConfig) -> Result<AgentRuntime> {
        if !supports_persistent_runtime(&config.cli_name) {
            return Err(std::io::Error::other(format!(
                "persistent runtime not supported for {}",
                config.cli_name
            ))
            .into());
        }

        let (tx, rx) = mpsc::unbounded_channel();
        let agent = CodexAppServerAgent::spawn(
            CodexAppServerLaunchConfig::from_streaming(config),
            tx.clone(),
        )
        .await?;

        Ok(AgentRuntime {
            agent: RuntimeAgent::new(Box::new(agent), tx),
            events: AgentEventReceiver { rx },
        })
    }
}

#[async_trait]
impl CodingAgent for CodexAppServerAgent {
    async fn send(&mut self, input: AgentInput) -> Result<AgentTurnResult> {
        match input {
            AgentInput::UserMessage(turn) => self.start_turn(turn).await,
            AgentInput::UrgentMessage(turn) => {
                let active_turn_id = self
                    .active_turn_id
                    .lock()
                    .expect("active turn mutex poisoned")
                    .clone();
                if let Some(active_turn_id) = active_turn_id {
                    self.steer_turn(turn, active_turn_id).await
                } else {
                    self.start_turn(turn).await
                }
            }
            AgentInput::Cancel => {
                self.interrupt_active_turn().await?;
                Ok(AgentTurnResult {
                    status: Ok(success_exit_status()),
                })
            }
        }
    }

    fn events(&mut self) -> AgentEventStream<'_> {
        Box::pin(BufferedEventStream {
            events: &mut self.events,
        })
    }

    fn take_events(&mut self) -> Vec<AgentEvent> {
        self.events.drain(..).collect()
    }

    async fn shutdown(&mut self, mode: ShutdownMode) -> Result<Option<ExitStatus>> {
        match mode {
            ShutdownMode::Graceful => {
                if let Some(mut stdin) = self.stdin.take() {
                    let _ = stdin.shutdown().await;
                }
                let status = match tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    self.child.wait(),
                )
                .await
                {
                    Ok(result) => result?,
                    Err(_) => {
                        self.child.start_kill()?;
                        self.child.wait().await?
                    }
                };
                self.last_exit_status = Some(status.clone());
                self.reader_task.abort();
                self.events.push_back(AgentEvent::Exited(status.clone()));
                Ok(Some(status))
            }
            ShutdownMode::Immediate => {
                let _ = self.interrupt_active_turn().await;
                let _ = self.stdin.take();
                self.child.start_kill()?;
                let status = self.child.wait().await?;
                self.last_exit_status = Some(status.clone());
                self.reader_task.abort();
                self.events.push_back(AgentEvent::Exited(status.clone()));
                Ok(Some(status))
            }
        }
    }

    fn session_id(&self) -> Option<String> {
        self.thread_id
            .lock()
            .expect("thread id mutex poisoned")
            .clone()
    }
}

struct BufferedEventStream<'a> {
    events: &'a mut VecDeque<AgentEvent>,
}

impl Stream for BufferedEventStream<'_> {
    type Item = AgentEvent;

    fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(self.events.pop_front())
    }
}

fn describe_exit_status(status: &ExitStatus) -> String {
    status
        .code()
        .map(|code| code.to_string())
        .unwrap_or_else(|| "terminated by signal".to_string())
}

fn as_io_error(err: crate::error::Error) -> std::io::Error {
    match err {
        crate::error::Error::Io(inner) => inner,
        other => std::io::Error::other(other.to_string()),
    }
}

#[must_use]
fn success_exit_status() -> ExitStatus {
    exit_status_from_code(0)
}

#[cfg(unix)]
fn exit_status_from_code(code: i32) -> ExitStatus {
    std::os::unix::process::ExitStatusExt::from_raw(code << 8)
}
