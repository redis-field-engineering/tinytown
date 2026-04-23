/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Agent runtime adapters.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use async_trait::async_trait;
use futures_core::Stream;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child as TokioChild, ChildStdin, ChildStdout, Command as TokioCommand};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::Result;

const TURN_STATUS_MARKER: &str = "__TINYTOWN_TURN_STATUS__";

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

#[must_use]
fn build_turn_script(
    cli_name: &str,
    cli_cmd: &str,
    prompt_file: &Path,
    output_file: &Path,
) -> String {
    let shell_cmd = build_cli_command(cli_name, cli_cmd, prompt_file);
    let output_file = shell_quote(output_file);
    format!("{shell_cmd} > {output_file} 2>&1\nprintf '%s%s\\n' '{TURN_STATUS_MARKER}' \"$?\"\n")
}

#[must_use]
fn parse_turn_status(line: &str) -> Option<i32> {
    line.strip_prefix(TURN_STATUS_MARKER)
        .and_then(|status| status.trim().parse::<i32>().ok())
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
pub struct PersistentShellAgent {
    config: OneShotAgentConfig,
    events: VecDeque<AgentEvent>,
    last_exit_status: Option<ExitStatus>,
    session: Option<PersistentShellSession>,
    session_id: Option<String>,
}

#[derive(Debug)]
struct PersistentShellSession {
    child: TokioChild,
    stdin: ChildStdin,
    stdout: Lines<BufReader<ChildStdout>>,
}

impl PersistentShellSession {
    async fn start(config: &OneShotAgentConfig) -> Result<Self> {
        let mut cmd = TokioCommand::new("sh");
        cmd.current_dir(&config.workdir)
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
            .ok_or_else(|| std::io::Error::other("persistent runtime stdin unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| std::io::Error::other("persistent runtime stdout unavailable"))?;

        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout).lines(),
        })
    }
}

impl PersistentShellAgent {
    #[must_use]
    pub fn new(config: OneShotAgentConfig) -> Self {
        Self::with_session_id(config, None)
    }

    #[must_use]
    pub fn with_session_id(config: OneShotAgentConfig, session_id: Option<String>) -> Self {
        Self {
            config,
            events: VecDeque::new(),
            last_exit_status: None,
            session: None,
            session_id,
        }
    }

    async fn session(&mut self) -> Result<&mut PersistentShellSession> {
        if self.session.is_none() {
            self.session = Some(PersistentShellSession::start(&self.config).await?);
        }
        Ok(self.session.as_mut().expect("session initialized"))
    }

    async fn run_turn(&mut self, turn: AgentTurn) -> Result<AgentTurnResult> {
        self.events.push_back(AgentEvent::TurnStarted);
        std::fs::write(&turn.prompt_file, &turn.prompt)?;

        let script = build_turn_script(
            &self.config.cli_name,
            &self.config.cli_cmd,
            &turn.prompt_file,
            &turn.output_file,
        );
        let send_result = self.send_script(script).await;
        let _ = std::fs::remove_file(&turn.prompt_file);

        match &send_result {
            Ok(status) => {
                self.last_exit_status = Some(status.clone());
                if status.success() {
                    self.events
                        .push_back(AgentEvent::TurnCompleted { summary: None });
                    self.events.push_back(AgentEvent::AwaitingInput);
                } else {
                    self.events.push_back(AgentEvent::SessionError(format!(
                        "CLI exited with status {}",
                        describe_exit_status(status)
                    )));
                }
            }
            Err(err) => {
                self.events.push_back(AgentEvent::SessionError(format!(
                    "Persistent runtime failed: {}",
                    err
                )));
            }
        }

        Ok(AgentTurnResult {
            status: send_result,
        })
    }

    async fn send_script(&mut self, script: String) -> std::io::Result<ExitStatus> {
        let session = self.session().await.map_err(as_io_error)?;
        session.stdin.write_all(script.as_bytes()).await?;
        session.stdin.flush().await?;

        loop {
            match session.stdout.next_line().await? {
                Some(line) => {
                    if let Some(status) = parse_turn_status(&line) {
                        return Ok(exit_status_from_code(status));
                    }
                }
                None => {
                    let exit_status = session.child.wait().await?;
                    self.last_exit_status = Some(exit_status.clone());
                    self.events
                        .push_back(AgentEvent::Exited(exit_status.clone()));
                    self.session = None;
                    return Err(std::io::Error::other(format!(
                        "persistent runtime exited unexpectedly ({})",
                        describe_exit_status(&exit_status)
                    )));
                }
            }
        }
    }
}

#[derive(Debug)]
struct StreamingProcessAgent {
    child: TokioChild,
    stdin: Option<ChildStdin>,
    events: VecDeque<AgentEvent>,
    session_id: Arc<Mutex<Option<String>>>,
    output_files: Arc<Mutex<VecDeque<PathBuf>>>,
    event_tx: mpsc::UnboundedSender<AgentEvent>,
    reader_task: JoinHandle<()>,
}

#[derive(Default)]
struct StreamingReaderState {
    text_summary: String,
    tool_name: Option<String>,
    tool_json: String,
}

impl StreamingProcessAgent {
    fn spawn(
        config: StreamingAgentConfig,
        event_tx: mpsc::UnboundedSender<AgentEvent>,
    ) -> Result<Self> {
        let mut command_line = config.cli_cmd.clone();
        if let Some(session_id) = config.resume_session_id.as_ref() {
            command_line.push_str(" --resume ");
            command_line.push_str(&shell_quote_str(session_id));
        }

        let mut cmd = TokioCommand::new("sh");
        cmd.arg("-lc")
            .arg(command_line)
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
            .ok_or_else(|| std::io::Error::other("streaming runtime stdin unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| std::io::Error::other("streaming runtime stdout unavailable"))?;

        let session_id = Arc::new(Mutex::new(config.resume_session_id));
        let output_files = Arc::new(Mutex::new(VecDeque::new()));
        let reader_task = spawn_streaming_reader(
            stdout,
            event_tx.clone(),
            session_id.clone(),
            output_files.clone(),
        );

        Ok(Self {
            child,
            stdin: Some(stdin),
            events: VecDeque::new(),
            session_id,
            output_files,
            event_tx,
            reader_task,
        })
    }
}

fn spawn_streaming_reader(
    stdout: ChildStdout,
    event_tx: mpsc::UnboundedSender<AgentEvent>,
    session_id: Arc<Mutex<Option<String>>>,
    output_files: Arc<Mutex<VecDeque<PathBuf>>>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        let mut state = StreamingReaderState::default();
        while let Ok(Some(line)) = lines.next_line().await {
            handle_streaming_line(&line, &event_tx, &session_id, &output_files, &mut state);
        }
    })
}

fn handle_streaming_line(
    line: &str,
    event_tx: &mpsc::UnboundedSender<AgentEvent>,
    session_id: &Arc<Mutex<Option<String>>>,
    output_files: &Arc<Mutex<VecDeque<PathBuf>>>,
    state: &mut StreamingReaderState,
) {
    let Ok(payload) = serde_json::from_str::<serde_json::Value>(line) else {
        let _ = event_tx.send(AgentEvent::SessionError(format!(
            "Invalid streaming event: {}",
            line
        )));
        return;
    };

    if let Some(id) = payload
        .get("session_id")
        .and_then(serde_json::Value::as_str)
    {
        *session_id.lock().expect("session id mutex poisoned") = Some(id.to_string());
    }

    match payload.get("type").and_then(serde_json::Value::as_str) {
        Some("system")
            if payload.get("subtype").and_then(serde_json::Value::as_str) == Some("init") =>
        {
            if let Some(id) = payload
                .get("session_id")
                .and_then(serde_json::Value::as_str)
            {
                let _ = event_tx.send(AgentEvent::SessionReady {
                    session_id: id.to_string(),
                });
            }
        }
        Some("stream_event") => {
            let Some(event) = payload.get("event") else {
                return;
            };
            match event.get("type").and_then(serde_json::Value::as_str) {
                Some("content_block_start")
                    if event
                        .get("content_block")
                        .and_then(|block| block.get("type"))
                        .and_then(serde_json::Value::as_str)
                        == Some("tool_use") =>
                {
                    state.tool_name = event
                        .get("content_block")
                        .and_then(|block| block.get("name"))
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string);
                    state.tool_json.clear();
                }
                Some("content_block_delta") => {
                    if let Some(delta) = event.get("delta") {
                        match delta.get("type").and_then(serde_json::Value::as_str) {
                            Some("input_json_delta") => {
                                if let Some(partial) = delta
                                    .get("partial_json")
                                    .and_then(serde_json::Value::as_str)
                                {
                                    state.tool_json.push_str(partial);
                                }
                            }
                            Some("text_delta") => {
                                if let Some(text) =
                                    delta.get("text").and_then(serde_json::Value::as_str)
                                {
                                    state.text_summary.push_str(text);
                                    append_to_current_output(output_files, text);
                                    let _ =
                                        event_tx.send(AgentEvent::AssistantDelta(text.to_string()));
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Some("content_block_stop") => {
                    if let Some(name) = state.tool_name.take() {
                        let args = serde_json::from_str(&state.tool_json)
                            .unwrap_or_else(|_| serde_json::Value::String(state.tool_json.clone()));
                        state.tool_json.clear();
                        let _ = event_tx.send(AgentEvent::ToolCall { name, args });
                    }
                }
                Some("message_stop") => {
                    let summary = if state.text_summary.trim().is_empty() {
                        None
                    } else {
                        Some(state.text_summary.trim().to_string())
                    };
                    state.text_summary.clear();
                    finish_current_output(output_files);
                    let _ = event_tx.send(AgentEvent::TurnCompleted { summary });
                    let _ = event_tx.send(AgentEvent::AwaitingInput);
                }
                _ => {}
            }
        }
        _ => {}
    }
}

fn queue_output_file(
    output_files: &Arc<Mutex<VecDeque<PathBuf>>>,
    output_file: PathBuf,
) -> std::io::Result<()> {
    let _ = std::fs::File::create(&output_file)?;
    output_files
        .lock()
        .expect("output queue mutex poisoned")
        .push_back(output_file);
    Ok(())
}

fn append_to_current_output(output_files: &Arc<Mutex<VecDeque<PathBuf>>>, text: &str) {
    let current = output_files
        .lock()
        .expect("output queue mutex poisoned")
        .front()
        .cloned();
    if let Some(path) = current
        && let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(path)
    {
        let _ = std::io::Write::write_all(&mut file, text.as_bytes());
    }
}

fn finish_current_output(output_files: &Arc<Mutex<VecDeque<PathBuf>>>) {
    let _ = output_files
        .lock()
        .expect("output queue mutex poisoned")
        .pop_front();
}

pub struct StreamingAgent;

impl StreamingAgent {
    pub fn runtime(config: StreamingAgentConfig) -> Result<AgentRuntime> {
        let (tx, rx) = mpsc::unbounded_channel();
        let agent: Box<dyn CodingAgent> = if config.cli_name == "claude" {
            Box::new(StreamingProcessAgent::spawn(config, tx.clone())?)
        } else {
            let session_id = config
                .resume_session_id
                .unwrap_or_else(|| Uuid::new_v4().to_string());
            let _ = tx.send(AgentEvent::SessionReady {
                session_id: session_id.clone(),
            });
            Box::new(PersistentShellAgent::with_session_id(
                OneShotAgentConfig {
                    cli_name: config.cli_name,
                    cli_cmd: config.cli_cmd,
                    workdir: config.workdir,
                    env: config.env,
                },
                Some(session_id),
            ))
        };

        Ok(AgentRuntime {
            agent: RuntimeAgent::new(agent, tx),
            events: AgentEventReceiver { rx },
        })
    }
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

#[async_trait]
impl CodingAgent for PersistentShellAgent {
    async fn send(&mut self, input: AgentInput) -> Result<AgentTurnResult> {
        match input {
            AgentInput::UserMessage(turn) | AgentInput::UrgentMessage(turn) => {
                self.run_turn(turn).await
            }
            AgentInput::Cancel => {
                self.events.push_back(AgentEvent::SessionError(
                    "Cancel is not supported by the persistent runtime".to_string(),
                ));
                Ok(AgentTurnResult {
                    status: Err(std::io::Error::other(
                        "Cancel is not supported by the persistent runtime",
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

    async fn shutdown(&mut self, mode: ShutdownMode) -> Result<Option<ExitStatus>> {
        let Some(session) = self.session.as_mut() else {
            return Ok(self.last_exit_status.clone());
        };

        let status = match mode {
            ShutdownMode::Graceful => {
                session.stdin.write_all(b"exit\n").await?;
                session.stdin.flush().await?;
                session.child.wait().await?
            }
            ShutdownMode::Immediate => {
                session.child.start_kill()?;
                session.child.wait().await?
            }
        };

        self.last_exit_status = Some(status.clone());
        self.events.push_back(AgentEvent::Exited(status.clone()));
        self.session = None;
        Ok(Some(status))
    }

    fn session_id(&self) -> Option<String> {
        self.session_id.clone()
    }
}

#[async_trait]
impl CodingAgent for StreamingProcessAgent {
    async fn send(&mut self, input: AgentInput) -> Result<AgentTurnResult> {
        match input {
            AgentInput::UserMessage(turn) | AgentInput::UrgentMessage(turn) => {
                std::fs::write(&turn.prompt_file, &turn.prompt)?;
                queue_output_file(&self.output_files, turn.output_file)?;
                let payload = serde_json::json!({
                    "type": "user",
                    "message": {
                        "content": [{ "type": "text", "text": turn.prompt }]
                    }
                });
                let _ = self.event_tx.send(AgentEvent::TurnStarted);
                let stdin = self
                    .stdin
                    .as_mut()
                    .ok_or_else(|| std::io::Error::other("streaming runtime stdin closed"))?;
                stdin.write_all(payload.to_string().as_bytes()).await?;
                stdin.write_all(b"\n").await?;
                stdin.flush().await?;
                Ok(AgentTurnResult {
                    status: Ok(success_exit_status()),
                })
            }
            AgentInput::Cancel => {
                let _ = self.stdin.take();
                self.child.start_kill()?;
                let status = self.child.wait().await?;
                let _ = self.event_tx.send(AgentEvent::Exited(status.clone()));
                Ok(AgentTurnResult { status: Ok(status) })
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
        let status = match mode {
            ShutdownMode::Graceful => {
                if let Some(mut stdin) = self.stdin.take() {
                    let _ = stdin.shutdown().await;
                }
                self.child.wait().await?
            }
            ShutdownMode::Immediate => {
                let _ = self.stdin.take();
                self.child.start_kill()?;
                self.child.wait().await?
            }
        };
        self.reader_task.abort();
        let _ = self.event_tx.send(AgentEvent::Exited(status.clone()));
        Ok(Some(status))
    }

    fn session_id(&self) -> Option<String> {
        self.session_id
            .lock()
            .expect("session id mutex poisoned")
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

#[cfg(windows)]
fn exit_status_from_code(code: i32) -> ExitStatus {
    std::os::windows::process::ExitStatusExt::from_raw(code as u32)
}
