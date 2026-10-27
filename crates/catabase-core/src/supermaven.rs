//! Editor protocol adapter for the official Supermaven sm-agent. Protocol source:
//! https://github.com/supermaven-inc/supermaven-nvim (MIT; see third_party license).
//! Only the requested SQL document and schema definitions are supplied; no rows or credentials.
use crate::CoreError;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, Command},
    sync::{Mutex, watch},
    task::JoinHandle,
    time::{Instant, timeout},
};

const MAX_DOCUMENT: usize = 1024 * 1024;
const MAX_SCHEMA_CONTEXT: usize = 128 * 1024;
const MAX_LINE: usize = 256 * 1024;
const MAX_COMPLETION: usize = 16 * 1024;
const MAX_BINARY: usize = 100 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupermavenStatus {
    pub enabled: bool,
    pub state: String,
    pub message: String,
    pub activation_url: Option<String>,
    pub service_tier: Option<String>,
}

impl Default for SupermavenStatus {
    fn default() -> Self {
        Self {
            enabled: false,
            state: "off".into(),
            message: "Supermaven is disabled. Built-in schema suggestions remain available.".into(),
            activation_url: None,
            service_tier: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupermavenRequest {
    pub request_id: String,
    pub document_id: String,
    pub text: String,
    #[serde(default)]
    pub schema_context: String,
    /// Monaco offset in UTF-16 code units; the agent expects UTF-8 bytes.
    pub position: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SupermavenCompletion {
    pub text: String,
    pub delete_before: usize,
}

struct Pending {
    request_id: String,
    state_id: u64,
    items: Vec<Value>,
    bytes: usize,
    changed: Instant,
    ended: bool,
}

#[derive(Default)]
struct AgentState {
    status: SupermavenStatus,
    generation: u64,
    state_id: u64,
    pending: Option<Pending>,
    started: Option<Instant>,
    connected: Option<bool>,
    connection_error: Option<&'static str>,
    latest_request: Option<String>,
    #[cfg(test)]
    diagnostics: (usize, usize, std::collections::BTreeSet<String>),
}

impl AgentState {
    fn can_complete(&self) -> bool {
        self.status.enabled
            && self.status.activation_url.is_none()
            && matches!(
                self.status.state.as_str(),
                "ready" | "connecting" | "disconnected"
            )
    }

    fn ready(&mut self) {
        self.started = None;
        self.connection_error = None;
        self.status.state = "ready".into();
        self.status.message = "Supermaven inline SQL suggestions are ready.".into();
    }

    fn disconnected(&mut self) {
        self.started = None;
        self.pending = None;
        self.status.state = "disconnected".into();
        self.status.message = format!(
            "{} Edit SQL or press Option/Alt+\\ to reconnect; Retry in Settings restarts the agent. Built-in schema suggestions remain available.",
            self.connection_error
                .unwrap_or("The Supermaven service disconnected.")
        );
    }
}

struct AgentRun {
    child: Child,
    stdin: ChildStdin,
    reader: JoinHandle<()>,
    stderr: JoinHandle<()>,
}
impl Drop for AgentRun {
    fn drop(&mut self) {
        self.reader.abort();
        self.stderr.abort();
    }
}

pub struct SupermavenClient {
    root: PathBuf,
    run: Mutex<Option<AgentRun>>,
    state: std::sync::Arc<Mutex<AgentState>>,
    changed: watch::Sender<u64>,
}

impl SupermavenClient {
    pub fn new(root: PathBuf) -> Self {
        let (changed, _) = watch::channel(0);
        Self {
            root,
            run: Mutex::new(None),
            state: Default::default(),
            changed,
        }
    }

    pub async fn status(&self) -> SupermavenStatus {
        let mut state = self.state.lock().await;
        if matches!(state.status.state.as_str(), "starting" | "connecting")
            && state
                .started
                .is_some_and(|started| started.elapsed() > Duration::from_secs(45))
        {
            if state.status.state == "connecting" {
                state.connection_error.get_or_insert("The agent started, but the Supermaven service did not connect within 45 seconds.");
                state.disconnected();
            } else {
                state.status.state = "error".into();
                state.status.message =
                    "Supermaven agent did not respond within 45 seconds. Retry from Settings."
                        .into();
            }
        }
        state.status.clone()
    }

    pub async fn set_enabled(&self, enabled: bool) -> Result<SupermavenStatus, CoreError> {
        let mut run = self.run.lock().await;
        {
            let mut state = self.state.lock().await;
            state.generation += 1;
            state.pending = None;
            state.latest_request = None;
            state.state_id = 0;
            state.started = None;
            state.connected = None;
            state.connection_error = None;
            state.status = if enabled {
                SupermavenStatus {
                    enabled: true,
                    state: "starting".into(),
                    message: "Starting the official Supermaven agent…".into(),
                    ..Default::default()
                }
            } else {
                Default::default()
            };
        }
        self.changed.send_modify(|value| *value += 1);
        if let Some(mut old) = run.take() {
            let _ = old.child.start_kill();
            let _ = timeout(Duration::from_secs(2), old.child.wait()).await;
        }
        if !enabled {
            return Ok(self.status().await);
        }
        let result = self.start().await;
        match result {
            Ok(agent) => *run = Some(agent),
            Err(error) => {
                let mut state = self.state.lock().await;
                state.status.state = "error".into();
                state.status.message = error.to_string();
                return Err(error);
            }
        }
        Ok(self.status().await)
    }

    async fn start(&self) -> Result<AgentRun, CoreError> {
        let binary = install_binary(&self.root).await?;
        let mut command = Command::new(binary);
        command.arg("stdio");
        self.spawn_agent(command).await
    }

    async fn spawn_agent(&self, mut command: Command) -> Result<AgentRun, CoreError> {
        let workspace = self.root.join("context");
        tokio::fs::create_dir_all(&workspace).await?;
        let mut child = command
            .current_dir(&workspace)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        // Download requests have their own deadlines. Only time agent startup
        // once the process exists, not while installing/verifying the binary.
        self.state.lock().await.started = Some(Instant::now());
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| CoreError::message("Supermaven stdin unavailable"))?;
        // Same greeting as the Neovim plugin. No repository or file discovery
        // messages are sent, and the process runs outside any project directory.
        write_message(
            &mut stdin,
            &json!({"kind":"greeting","allowGitignore":false}),
        )
        .await?;
        // Like Neovim's initial document event, prime the connection without
        // exposing any draft. Some agent versions authenticate lazily.
        let initial = workspace.join("query.sql").to_string_lossy().into_owned();
        write_message(
            &mut stdin,
            &json!({"kind":"inform_file_changed","path":initial}),
        )
        .await?;
        write_message(
            &mut stdin,
            &json!({"kind":"state_update","newId":"0","updates":[
                {"kind":"cursor_update","path":initial,"offset":0},
                {"kind":"file_update","path":initial,"content":""}
            ]}),
        )
        .await?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| CoreError::message("Supermaven stdout unavailable"))?;
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| CoreError::message("Supermaven stderr unavailable"))?;
        let state = self.state.clone();
        let changed = self.changed.clone();
        let generation = state.lock().await.generation;
        let reader = tokio::spawn(async move {
            let mut output = BufReader::new(stdout);
            let mut line = Vec::new();
            loop {
                line.clear();
                // Bound malformed/unterminated output without ever logging it.
                let read = (&mut output)
                    .take((MAX_LINE + 1) as u64)
                    .read_until(b'\n', &mut line)
                    .await;
                if !matches!(read, Ok(size) if size > 0 && size <= MAX_LINE) {
                    break;
                }
                let Ok(text) = std::str::from_utf8(&line) else {
                    continue;
                };
                #[cfg(test)]
                {
                    state.lock().await.diagnostics.0 += 1;
                }
                let Some(payload) = text.strip_prefix("SM-MESSAGE ") else {
                    let mut state = state.lock().await;
                    if state.generation != generation {
                        return;
                    }
                    if process_agent_log(&mut state, text) {
                        changed.send_modify(|value| *value += 1);
                    }
                    continue;
                };
                let Ok(message) = serde_json::from_str::<Value>(payload) else {
                    continue;
                };
                let mut state = state.lock().await;
                if state.generation != generation {
                    return;
                }
                process_message(&mut state, &message);
                changed.send_modify(|value| *value += 1);
            }
            let mut state = state.lock().await;
            if state.generation == generation {
                state.status.state = "error".into();
                state.status.message = "Supermaven agent stopped. Retry from Settings.".into();
                state.pending = None;
                changed.send_modify(|value| *value += 1);
            }
        });
        let stderr = tokio::spawn(async move {
            // Drain without retaining logs: provider output may contain private context.
            let mut buffer = [0u8; 4096];
            while matches!(stderr.read(&mut buffer).await, Ok(size) if size > 0) {}
        });
        Ok(AgentRun {
            child,
            stdin,
            reader,
            stderr,
        })
    }

    pub async fn use_free_version(&self) -> Result<(), CoreError> {
        self.send(&json!({"kind":"use_free_version"})).await
    }

    pub async fn cancel(&self, request_id: &str) {
        let mut state = self.state.lock().await;
        if state.latest_request.as_deref() == Some(request_id) {
            state.latest_request = None;
        }
        if state
            .pending
            .as_ref()
            .is_some_and(|pending| pending.request_id == request_id)
        {
            state.pending = None;
            self.changed.send_modify(|value| *value += 1);
        }
    }

    async fn send(&self, message: &Value) -> Result<(), CoreError> {
        let mut run = self.run.lock().await;
        let agent = run
            .as_mut()
            .ok_or_else(|| CoreError::message("Enable Supermaven in Settings first"))?;
        let result = write_message(&mut agent.stdin, message).await;
        if result.is_err() {
            let _ = agent.child.start_kill();
            let mut state = self.state.lock().await;
            state.pending = None;
            state.status.state = "error".into();
            state.status.message =
                "Supermaven agent is not responding. Retry from Settings.".into();
            self.changed.send_modify(|value| *value += 1);
        }
        result
    }

    pub async fn complete(
        &self,
        request: SupermavenRequest,
    ) -> Result<Option<SupermavenCompletion>, CoreError> {
        if request.text.len() > MAX_DOCUMENT || request.text.trim().is_empty() {
            return Ok(None);
        }
        if request.request_id.is_empty()
            || request.request_id.len() > 128
            || request.document_id.is_empty()
            || request.document_id.len() > 128
            || !request
                .document_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(CoreError::message("Invalid completion document"));
        }
        let offset = utf16_byte_offset(&request.text, request.position)
            .ok_or_else(|| CoreError::message("Invalid completion cursor"))?;
        let (document, agent_offset) = agent_document(&request, offset)?;
        let mut updates = self.changed.subscribe();
        {
            let mut state = self.state.lock().await;
            if !state.can_complete() {
                return Ok(None);
            }
            state.latest_request = Some(request.request_id.clone());
        }
        self.changed.send_modify(|value| *value += 1);
        let mut run = self.run.lock().await;
        let Some(agent) = run.as_mut() else {
            return Ok(None);
        };
        let state_id = {
            let mut state = self.state.lock().await;
            if !state.can_complete() || state.latest_request.as_deref() != Some(&request.request_id)
            {
                return Ok(None);
            }
            // The official agent reconnects lazily on a new state_update, just
            // like Neovim. Gating requests on ready deadlocks after a disconnect.
            if state.status.state == "disconnected" {
                state.started = Some(Instant::now());
                state.status.state = "connecting".into();
                state.status.message = "Reconnecting to Supermaven…".into();
            }
            state.state_id += 1;
            let id = state.state_id;
            state.pending = Some(Pending {
                request_id: request.request_id.clone(),
                state_id: id,
                items: Vec::new(),
                bytes: 0,
                changed: Instant::now(),
                ended: false,
            });
            id
        };
        self.changed.send_modify(|value| *value += 1);
        let path = self
            .root
            .join("context")
            // A UUID filename biases the model toward inventing a UUID table.
            // Reuse one neutral virtual document; replace, don't accumulate,
            // context when switching tabs or database connections.
            .join("query.sql")
            .to_string_lossy()
            .into_owned();
        let sent = async {
            write_message(
                &mut agent.stdin,
                &json!({"kind":"inform_file_changed","path":path}),
            )
            .await?;
            write_message(
                &mut agent.stdin,
                &json!({"kind":"state_update","newId":state_id.to_string(),"updates":[
                    {"kind":"cursor_update","path":path,"offset":agent_offset},
                    {"kind":"file_update","path":path,"content":document}
                ]}),
            )
            .await
        }
        .await;
        if let Err(error) = sent {
            let _ = agent.child.start_kill();
            let mut state = self.state.lock().await;
            state.pending = None;
            state.status.state = "error".into();
            state.status.message =
                "Supermaven agent is not responding. Retry from Settings.".into();
            self.changed.send_modify(|value| *value += 1);
            return Err(error);
        }
        drop(run);
        // Match the Neovim plugin's five-second polling window. A cold model
        // request can take longer than two seconds, especially with schema context.
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            {
                let state = self.state.lock().await;
                let Some(pending) = state.pending.as_ref().filter(|pending| {
                    pending.state_id == state_id
                        && state.latest_request.as_deref() == Some(&request.request_id)
                }) else {
                    return Ok(None);
                };
                if pending.ended || Instant::now() >= deadline {
                    return Ok(derive_completion(&pending.items, &request.text[..offset]));
                }
                if pending.changed.elapsed() >= Duration::from_millis(45)
                    && let Some(completion) =
                        derive_completion(&pending.items, &request.text[..offset])
                {
                    return Ok(Some(completion));
                }
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            let _ = timeout(Duration::from_millis(45), updates.changed()).await;
        }
    }

    pub async fn open_activation(&self) -> Result<(), CoreError> {
        let url = self
            .status()
            .await
            .activation_url
            .filter(|url| valid_activation_url(url))
            .ok_or_else(|| CoreError::message("No Supermaven activation link is available yet"))?;
        #[cfg(target_os = "macos")]
        let mut command = {
            let mut command = Command::new("open");
            command.arg(&url);
            command
        };
        #[cfg(target_os = "linux")]
        let mut command = {
            let mut command = Command::new("xdg-open");
            command.arg(&url);
            command
        };
        #[cfg(target_os = "windows")]
        let mut command = {
            let mut command = Command::new("rundll32.exe");
            command.arg("url.dll,FileProtocolHandler").arg(&url);
            command
        };
        let status = timeout(Duration::from_secs(10), command.status())
            .await
            .map_err(|_| CoreError::message("Opening activation link timed out"))??;
        if !status.success() {
            return Err(CoreError::message(
                "Could not open the Supermaven activation link",
            ));
        }
        Ok(())
    }
}

async fn write_message(stdin: &mut ChildStdin, message: &Value) -> Result<(), CoreError> {
    let mut bytes = serde_json::to_vec(message)?;
    bytes.push(b'\n');
    timeout(Duration::from_secs(2), stdin.write_all(&bytes))
        .await
        .map_err(|_| CoreError::message("Supermaven agent is not responding"))??;
    Ok(())
}

fn process_agent_log(state: &mut AgentState, line: &str) -> bool {
    // Agent logs can contain SQL, account emails or credentials. Classify only
    // known transport failures; never retain, forward or persist raw log text.
    if !line.contains("ERROR [sm_agent::edge_connection]") {
        return false;
    }
    let reason = if line.contains("Connection reset without closing handshake") {
        "The Supermaven WebSocket connection was reset before closing cleanly."
    } else if line.contains("error reading from websocket:") {
        "The Supermaven WebSocket connection failed."
    } else {
        return false;
    };
    state.connection_error = Some(reason);
    if state.status.state == "disconnected" {
        state.disconnected();
    }
    true
}

fn process_message(state: &mut AgentState, message: &Value) {
    #[cfg(test)]
    {
        state.diagnostics.1 += 1;
        let kind = match message["kind"].as_str().unwrap_or("") {
            kind @ ("response" | "metadata" | "activation_request" | "activation_success"
            | "passthrough" | "popup" | "task_status" | "active_repo" | "service_tier"
            | "apology" | "set") => kind,
            other
                if !other.is_empty()
                    && other.len() <= 48
                    && other
                        .bytes()
                        .all(|byte| byte.is_ascii_lowercase() || byte == b'_') =>
            {
                other
            }
            _ => "unrecognized",
        };
        state.diagnostics.2.insert(kind.into());
        if matches!(
            kind,
            "popup" | "unrecognized" | "status" | "status_update" | "auth_status" | "ready"
        ) || !matches!(
            kind,
            "response"
                | "metadata"
                | "activation_request"
                | "activation_success"
                | "passthrough"
                | "task_status"
                | "active_repo"
                | "service_tier"
                | "apology"
                | "set"
        ) {
            let keys = message
                .as_object()
                .map(|object| {
                    object
                        .keys()
                        .filter(|key| {
                            key.len() <= 48
                                && key
                                    .bytes()
                                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                        })
                        .cloned()
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            state
                .diagnostics
                .2
                .insert(format!("{kind} fields {keys:?}"));
        }
    }
    match message["kind"].as_str().unwrap_or("") {
        "passthrough" => {
            // Only one wrapper is accepted; do not recurse through untrusted output.
            if message["passthrough"]["kind"] != "passthrough" {
                process_message(state, &message["passthrough"]);
            }
        }
        "activation_request" => {
            if let Some(url) = message["activateUrl"]
                .as_str()
                .filter(|url| valid_activation_url(url))
            {
                state.status.activation_url = Some(url.into());
                state.status.state = "activation".into();
                state.status.message =
                    "Sign in with an existing Supermaven account, or choose the free tier.".into();
            }
        }
        "activation_success" => {
            state.status.activation_url = None;
            if state.connected != Some(false) {
                state.ready();
            }
        }
        "service_tier" => {
            state.status.service_tier = message["display"]
                .as_str()
                .map(|value| value.chars().take(80).collect());
            state.status.activation_url = None;
            // Legacy agents announce only a tier. Modern agents also report
            // transport state; a late account message must not mask a disconnect.
            if state.connected != Some(false) {
                state.ready();
            }
        }
        "connection_status" => {
            let Some(connected) = message["is_connected"].as_bool() else {
                return;
            };
            state.connected = Some(connected);
            if state.status.enabled && state.status.activation_url.is_none() {
                if connected {
                    state.ready();
                } else if message["status_text"]
                    .as_str()
                    .is_some_and(|text| text.eq_ignore_ascii_case("disconnected"))
                {
                    state.disconnected();
                } else {
                    if state.status.state != "connecting" {
                        state.started = Some(Instant::now());
                    }
                    state.status.state = "connecting".into();
                    state.status.message = "Connecting to Supermaven…".into();
                }
            }
        }
        "user_status" => {
            // Account email is deliberately not retained or exposed by Catabase.
            state.status.service_tier = message["tier"]
                .as_str()
                .map(|tier| tier.chars().take(80).collect());
        }
        // Like the Neovim plugin, ignore generic popups. These include product
        // announcements with vendor links, not necessarily activation requests.
        "response" => {
            let id = message["stateId"]
                .as_str()
                .and_then(|value| value.parse::<u64>().ok())
                .or_else(|| message["stateId"].as_u64());
            let Some(pending) = state
                .pending
                .as_mut()
                .filter(|pending| Some(pending.state_id) == id)
            else {
                return;
            };
            if let Some(items) = message["items"].as_array() {
                for item in items.iter().take(256) {
                    pending.bytes += item["text"].as_str().map_or(0, str::len);
                    if pending.bytes > MAX_COMPLETION || pending.items.len() >= 256 {
                        pending.ended = true;
                        break;
                    }
                    pending.ended |= matches!(
                        item["kind"].as_str(),
                        Some("end" | "barrier" | "finish_edit" | "jump" | "delete" | "skip")
                    );
                    let kind = item["kind"]
                        .as_str()
                        .filter(|kind| {
                            matches!(
                                *kind,
                                "text"
                                    | "dedent"
                                    | "barrier"
                                    | "finish_edit"
                                    | "end"
                                    | "jump"
                                    | "delete"
                                    | "skip"
                            )
                        })
                        .unwrap_or("unsupported");
                    pending
                        .items
                        .push(json!({"kind":kind,"text":item["text"].as_str().unwrap_or("")}));
                }
                pending.changed = Instant::now();
            }
        }
        _ => {}
    }
}

fn valid_activation_url(raw: &str) -> bool {
    url::Url::parse(raw).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str() == Some("supermaven.com")
            && url.username().is_empty()
            && url.password().is_none()
            && url.port_or_known_default() == Some(443)
    })
}

fn utf16_byte_offset(text: &str, position: usize) -> Option<usize> {
    let mut units = 0;
    for (byte, character) in text.char_indices() {
        if units == position {
            return Some(byte);
        }
        units += character.len_utf16();
        if units > position {
            return None;
        }
    }
    (units == position).then_some(text.len())
}

fn agent_document(
    request: &SupermavenRequest,
    offset: usize,
) -> Result<(String, usize), CoreError> {
    if request.schema_context.len() > MAX_SCHEMA_CONTEXT {
        return Err(CoreError::message(
            "Completion schema context exceeds the limit",
        ));
    }
    let mut document = String::new();
    if !request.schema_context.is_empty() {
        document.push_str(
            "-- PostgreSQL schema reference. Use only these existing relations/columns.\n",
        );
        // Every line is commented, including catalog names containing newlines.
        // This virtual preamble is never inserted in the editor or run on PostgreSQL.
        for line in request.schema_context.lines() {
            document.push_str("-- ");
            document.push_str(line);
            document.push('\n');
        }
        document.push_str("-- SQL query:\n");
    }
    let agent_offset = document.len() + offset;
    document.push_str(&request.text);
    Ok((document, agent_offset))
}

fn derive_completion(items: &[Value], prefix: &str) -> Option<SupermavenCompletion> {
    let mut text = String::new();
    let mut dedent = String::new();
    for item in items {
        match item["kind"].as_str()? {
            "text" => text.push_str(item["text"].as_str()?),
            "dedent" if text.is_empty() => dedent.push_str(item["text"].as_str()?),
            "barrier" | "finish_edit" | "end" => break,
            // Non-insertion edits and cross-file jumps are never applied.
            "delete" | "jump" | "skip" => return None,
            _ => return None,
        }
    }
    let line_prefix = prefix.rsplit('\n').next()?;
    if !line_prefix.ends_with(&dedent) {
        return None;
    }
    // Neovim strips an unchanged common prefix from replacement-style edits.
    // Completing `prop` by replacing `prop` with `properties` is just inserting
    // `erties`, not a destructive edit. Compare Unicode characters, trim bytes.
    let shared: usize = dedent
        .chars()
        .zip(text.chars())
        .take_while(|(left, right)| left == right)
        .map(|(character, _)| character.len_utf8())
        .sum();
    dedent.drain(..shared);
    text.drain(..shared);
    if text.trim().is_empty() || !dedent.bytes().all(|byte| matches!(byte, b' ' | b'\t')) {
        return None;
    }
    Some(SupermavenCompletion {
        text: text.trim_end().into(),
        delete_before: dedent.len(),
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    download_url: String,
    sha256_hash: String,
}

fn platform() -> Result<(&'static str, &'static str), CoreError> {
    let os = match std::env::consts::OS {
        "macos" => "macosx",
        "linux" => "linux",
        "windows" => "windows",
        _ => {
            return Err(CoreError::message(
                "Supermaven is not supported on this platform",
            ));
        }
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "aarch64",
        "x86_64" => "x86_64",
        _ => {
            return Err(CoreError::message(
                "Supermaven is not supported on this architecture",
            ));
        }
    };
    Ok((os, arch))
}

async fn install_binary(root: &Path) -> Result<PathBuf, CoreError> {
    let (platform, arch) = platform()?;
    let directory = root.join("binary").join(format!("{platform}-{arch}"));
    let binary = directory.join(if cfg!(windows) {
        "sm-agent.exe"
    } else {
        "sm-agent"
    });
    let http = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let manifest: Manifest = http
        .get("https://supermaven.com/api/download-path-v2")
        .query(&[("platform", platform), ("arch", arch), ("editor", "neovim")])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let url = url::Url::parse(&manifest.download_url)?;
    if url.scheme() != "https"
        || url.host_str() != Some("supermaven-public.s3.amazonaws.com")
        || manifest.sha256_hash.len() != 64
        || !manifest
            .sha256_hash
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(CoreError::message(
            "Supermaven returned an unexpected download manifest",
        ));
    }
    if tokio::fs::metadata(&binary)
        .await
        .is_ok_and(|metadata| metadata.len() <= MAX_BINARY as u64)
        && let Ok(bytes) = tokio::fs::read(&binary).await
        && verify_hash(&bytes, &manifest.sha256_hash)
    {
        return Ok(binary);
    }
    let mut response = http.get(url).send().await?.error_for_status()?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len() + chunk.len() > MAX_BINARY {
            return Err(CoreError::message(
                "Supermaven binary exceeds the download limit",
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    if !verify_hash(&bytes, &manifest.sha256_hash) {
        return Err(CoreError::message(
            "Supermaven agent checksum verification failed",
        ));
    }
    tokio::fs::create_dir_all(&directory).await?;
    let temporary = directory.join(format!("{}.tmp", uuid::Uuid::new_v4()));
    tokio::fs::write(&temporary, &bytes).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o700)).await?;
    }
    tokio::fs::rename(&temporary, &binary).await?;
    Ok(binary)
}

fn verify_hash(bytes: &[u8], expected: &str) -> bool {
    format!("{:x}", Sha256::digest(bytes)).eq_ignore_ascii_case(expected)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(id: &str, text: &str) -> SupermavenRequest {
        SupermavenRequest {
            request_id: id.into(),
            document_id: "test-tab".into(),
            text: text.into(),
            schema_context: String::new(),
            position: text.encode_utf16().count(),
        }
    }

    #[test]
    fn cursor_offsets_are_utf8_bytes_not_monaco_utf16_units() {
        let text = "ž😀\nselect ";
        assert_eq!(utf16_byte_offset(text, 0), Some(0));
        assert_eq!(utf16_byte_offset(text, 1), Some(2));
        assert_eq!(utf16_byte_offset(text, 2), None);
        assert_eq!(utf16_byte_offset(text, 3), Some(6));
        assert_eq!(
            utf16_byte_offset(text, text.encode_utf16().count()),
            Some(text.len())
        );
        assert_eq!(utf16_byte_offset(text, 999), None);
    }

    #[test]
    fn virtual_schema_is_commented_and_offsets_still_point_into_the_real_draft() {
        let mut request = request("test", "select 'ž😀', ");
        request.schema_context = "CREATE TABLE \"public\".\"properties\" (\"id\" text PRIMARY KEY);\nSELECT 'not executable';".into();
        let (document, offset) = agent_document(&request, request.text.len()).unwrap();
        assert!(document.contains("-- CREATE TABLE \"public\".\"properties\""));
        assert!(document.contains("\n-- SELECT 'not executable';\n"));
        assert!(document.ends_with(&request.text));
        assert_eq!(offset, document.len());
        assert!(!document.contains(&request.document_id));
        request.schema_context = "x".repeat(MAX_SCHEMA_CONTEXT + 1);
        assert!(agent_document(&request, 0).is_err());
    }

    #[test]
    fn streamed_insertions_and_whitespace_dedent_are_safe() {
        let items = [
            json!({"kind":"dedent","text":"  "}),
            json!({"kind":"text","text":"select "}),
            json!({"kind":"text","text":"1\n"}),
            json!({"kind":"finish_edit"}),
        ];
        assert_eq!(
            derive_completion(&items, "  "),
            Some(SupermavenCompletion {
                text: "select 1".into(),
                delete_before: 2
            })
        );
        assert!(derive_completion(&items, "x").is_none());
        for kind in ["delete", "jump", "skip", "unknown"] {
            assert!(derive_completion(&[json!({"kind":kind})], "select ").is_none());
        }
        assert!(
            derive_completion(
                &[
                    json!({"kind":"dedent","text":"select"}),
                    json!({"kind":"text","text":"other"})
                ],
                "select"
            )
            .is_none()
        );
        for (prefix, suggested, expected) in
            [("prop", "properties", "erties"), ("č", "číslo", "íslo")]
        {
            assert_eq!(
                derive_completion(
                    &[
                        json!({"kind":"dedent","text":prefix}),
                        json!({"kind":"text","text":suggested}),
                    ],
                    prefix
                ),
                Some(SupermavenCompletion {
                    text: expected.into(),
                    delete_before: 0
                })
            );
        }
    }

    #[test]
    fn stale_responses_and_unsupported_service_messages_are_ignored() {
        let mut state = AgentState {
            pending: Some(Pending {
                request_id: "latest".into(),
                state_id: 4,
                items: vec![],
                bytes: 0,
                changed: Instant::now(),
                ended: false,
            }),
            ..Default::default()
        };
        process_message(
            &mut state,
            &json!({"kind":"response","stateId":"3","items":[{"kind":"text","text":"stale"}]}),
        );
        assert!(state.pending.as_ref().unwrap().items.is_empty());
        process_message(
            &mut state,
            &json!({"kind":"passthrough","passthrough":{"kind":"response","stateId":"4","items":[{"kind":"text","text":"1"},{"kind":"end"}]}}),
        );
        assert_eq!(state.pending.as_ref().unwrap().items.len(), 2);
        assert!(state.pending.as_ref().unwrap().ended);
        process_message(
            &mut state,
            &json!({"kind":"execute_command","command":"ignored"}),
        );
        assert!(!state.status.enabled);
    }

    #[test]
    fn activation_is_user_initiated_and_links_are_vendor_https_only() {
        for invalid in [
            "http://supermaven.com/activate",
            "https://evil.example/activate",
            "file:///tmp/file",
            "javascript:alert(1)",
            "https://supermaven.com@evil.example/a",
            "https://supermaven.com:8443/activate",
        ] {
            assert!(!valid_activation_url(invalid));
        }
        let mut state = AgentState::default();
        process_message(
            &mut state,
            &json!({"kind":"activation_request","activateUrl":"https://evil.example/activate"}),
        );
        assert!(state.status.activation_url.is_none());
        process_message(
            &mut state,
            &json!({"kind":"activation_request","activateUrl":"https://supermaven.com/activate?token=synthetic"}),
        );
        assert_eq!(state.status.state, "activation");
        process_message(&mut state, &json!({"kind":"service_tier","display":"Free"}));
        assert_eq!(state.status.state, "ready");
        assert!(state.status.activation_url.is_none());
    }

    #[test]
    fn current_agent_status_ignores_generic_vendor_popups_and_private_account_fields() {
        let mut state = AgentState::default();
        state.status.enabled = true;
        process_message(
            &mut state,
            &json!({"kind":"passthrough","passthrough":{
                "kind":"connection_status","is_connected":false,"status_text":"Connecting…"
            }}),
        );
        assert_eq!(state.status.state, "connecting");
        process_message(
            &mut state,
            &json!({"kind":"user_status","tier":"Free","email":"not-retained@example.invalid"}),
        );
        assert_eq!(state.status.service_tier.as_deref(), Some("Free"));
        assert!(
            !serde_json::to_string(&state.status)
                .unwrap()
                .contains("email")
        );
        process_message(
            &mut state,
            &json!({"kind":"connection_status","is_connected":true}),
        );
        assert_eq!(state.status.state, "ready");
        process_message(
            &mut state,
            &json!({"kind":"popup","actions":[{"url":"https://evil.example/activate"}]}),
        );
        assert!(state.status.activation_url.is_none());
        process_message(
            &mut state,
            &json!({"kind":"popup","message":"News at https://supermaven.com/blog/sunsetting-supermaven"}),
        );
        assert_eq!(state.status.state, "ready");
        assert!(state.status.activation_url.is_none());
        process_message(
            &mut state,
            &json!({"kind":"activation_request","activateUrl":"https://supermaven.com/activate?token=synthetic"}),
        );
        process_message(
            &mut state,
            &json!({"kind":"connection_status","is_connected":false,"status_text":"Connecting…"}),
        );
        assert_eq!(state.status.state, "activation");
    }

    #[test]
    fn downloads_require_matching_sha256() {
        assert!(verify_hash(
            b"abc",
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        ));
        assert!(!verify_hash(
            b"tampered",
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        ));
    }

    #[test]
    fn disconnect_diagnostics_survive_late_account_messages_without_retaining_logs() {
        let mut state = AgentState::default();
        state.status.enabled = true;
        process_message(
            &mut state,
            &json!({"kind":"connection_status","is_connected":true}),
        );
        assert!(state.started.is_none());
        assert!(!process_agent_log(
            &mut state,
            "SQL: secret@example.invalid select 'private'"
        ));
        assert!(process_agent_log(
            &mut state,
            "ERROR [sm_agent::edge_connection] error reading from websocket: WebSocket protocol error: Connection reset without closing handshake private@example.invalid https://example.invalid?token=secret"
        ));
        process_message(
            &mut state,
            &json!({"kind":"connection_status","is_connected":false,"status_text":"disconnected"}),
        );
        assert_eq!(state.status.state, "disconnected");
        assert!(state.can_complete());
        assert!(
            state
                .status
                .message
                .contains("WebSocket connection was reset")
        );
        let public_status = serde_json::to_string(&state.status).unwrap();
        for private in ["secret", "private", "example.invalid"] {
            assert!(!public_status.contains(private));
        }
        process_message(&mut state, &json!({"kind":"service_tier","display":"Free"}));
        process_message(&mut state, &json!({"kind":"activation_success"}));
        assert_eq!(state.status.state, "disconnected");
        assert!(
            state
                .status
                .message
                .contains("WebSocket connection was reset")
        );
        process_message(
            &mut state,
            &json!({"kind":"connection_status","is_connected":true}),
        );
        assert_eq!(state.status.state, "ready");
        assert!(state.connection_error.is_none());
    }

    #[tokio::test]
    async fn connection_deadlines_restart_per_attempt_not_per_process_lifetime() {
        let directory = tempfile::tempdir().unwrap();
        let client = SupermavenClient::new(directory.path().to_owned());
        {
            let mut state = client.state.lock().await;
            state.status.enabled = true;
            state.status.state = "connecting".into();
            state.started = Some(Instant::now() - Duration::from_secs(46));
        }
        let status = client.status().await;
        assert_eq!(status.state, "disconnected");
        assert!(status.message.contains("agent started"));
        {
            let mut state = client.state.lock().await;
            process_message(
                &mut state,
                &json!({"kind":"connection_status","is_connected":false,"status_text":"connecting..."}),
            );
            assert!(state.started.unwrap().elapsed() < Duration::from_secs(1));
        }
        assert_eq!(client.status().await.state, "connecting");
        {
            let mut state = client.state.lock().await;
            state.started = Some(Instant::now() - Duration::from_secs(46));
            // Repeated progress messages must not extend a stalled attempt forever.
            process_message(
                &mut state,
                &json!({"kind":"connection_status","is_connected":false,"status_text":"connecting..."}),
            );
        }
        assert_eq!(client.status().await.state, "disconnected");
        {
            let mut state = client.state.lock().await;
            state.status.state = "starting".into();
            state.started = Some(Instant::now() - Duration::from_secs(46));
        }
        assert_eq!(client.status().await.state, "error");
    }

    #[test]
    fn fake_agent_fixture() {
        if std::env::var("CATABASE_FAKE_SUPERMAVEN").as_deref() != Ok("1") {
            return;
        }
        use std::io::{BufRead, Write};
        println!(); // Separate from the test harness's status prefix.
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else {
                break;
            };
            let message: Value = serde_json::from_str(&line).unwrap();
            match message["kind"].as_str() {
                Some("greeting") => println!(
                    "SM-MESSAGE {}",
                    json!({"kind":"service_tier","display":"Synthetic fixture"})
                ),
                Some("state_update") => {
                    let text = message["updates"][1]["content"].as_str().unwrap();
                    if text.contains("disconnect_probe") {
                        println!(
                            "ERROR [sm_agent::edge_connection] error reading from websocket: WebSocket protocol error: Connection reset without closing handshake"
                        );
                        println!(
                            "SM-MESSAGE {}",
                            json!({"kind":"connection_status","is_connected":false,"status_text":"disconnected"})
                        );
                        std::io::stdout().flush().unwrap();
                        continue;
                    }
                    println!(
                        "SM-MESSAGE {}",
                        json!({"kind":"connection_status","is_connected":true})
                    );
                    assert!(
                        message["updates"][1]["path"]
                            .as_str()
                            .unwrap()
                            .ends_with("/query.sql")
                            || message["updates"][1]["path"]
                                .as_str()
                                .unwrap()
                                .ends_with("\\query.sql")
                    );
                    if text.contains("context_probe") {
                        assert!(text.contains("-- CREATE TABLE properties (id text);"));
                        assert_eq!(
                            message["updates"][0]["offset"].as_u64().unwrap() as usize,
                            text.len()
                        );
                    }
                    if text.contains("slow") {
                        std::thread::sleep(Duration::from_millis(150));
                    }
                    println!(
                        "SM-MESSAGE {}",
                        json!({"kind":"response","stateId":message["newId"],"items":[{"kind":"text","text":"1 AS synthetic"},{"kind":"end"}]})
                    );
                }
                _ => {}
            }
            std::io::stdout().flush().unwrap();
        }
    }

    async fn mock_client(root: &Path) -> SupermavenClient {
        let client = SupermavenClient::new(root.to_owned());
        client.state.lock().await.status.enabled = true;
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "supermaven::tests::fake_agent_fixture",
                "--nocapture",
            ])
            .env("CATABASE_FAKE_SUPERMAVEN", "1");
        *client.run.lock().await = Some(client.spawn_agent(command).await.unwrap());
        for _ in 0..200 {
            if client.status().await.state == "ready" {
                return client;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("mock agent failed to start");
    }

    #[tokio::test]
    async fn process_protocol_completes_and_disable_kills_pending_work() {
        let directory = tempfile::tempdir().unwrap();
        let client = mock_client(directory.path()).await;
        let mut probe = request("context", "select 'ž😀' /* context_probe */ ");
        probe.schema_context = "CREATE TABLE properties (id text);".into();
        assert!(client.complete(probe).await.unwrap().is_some());
        assert_eq!(
            client
                .complete(request("first", "select "))
                .await
                .unwrap()
                .unwrap()
                .text,
            "1 AS synthetic"
        );
        let (response, disabled) =
            tokio::join!(client.complete(request("second", "select slow")), async {
                tokio::time::sleep(Duration::from_millis(20)).await;
                client.set_enabled(false).await.unwrap()
            });
        assert!(response.unwrap().is_none());
        assert!(!disabled.enabled);
        assert!(client.run.lock().await.is_none());
        assert!(
            client
                .complete(request("disabled", "select "))
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn next_edit_recovers_after_disconnect_without_restarting_the_agent() {
        let directory = tempfile::tempdir().unwrap();
        let client = mock_client(directory.path()).await;
        let child_id = client.run.lock().await.as_ref().unwrap().child.id();
        let result = timeout(
            Duration::from_secs(2),
            client.complete(request("lost", "select /* disconnect_probe */ ")),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(result.is_none());
        assert_eq!(client.status().await.state, "disconnected");
        assert!(client.state.lock().await.pending.is_none());
        assert!(
            client
                .status()
                .await
                .message
                .contains("WebSocket connection was reset")
        );
        let completion = client
            .complete(request("retry", "select "))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(completion.text, "1 AS synthetic");
        assert_eq!(client.status().await.state, "ready");
        assert_eq!(
            client.run.lock().await.as_ref().unwrap().child.id(),
            child_id
        );
        client.set_enabled(false).await.unwrap();
    }

    #[tokio::test]
    async fn superseded_requests_and_cancellation_never_return_stale_text() {
        let directory = tempfile::tempdir().unwrap();
        let client = mock_client(directory.path()).await;
        let (old, new) = tokio::join!(client.complete(request("old", "select slow")), async {
            tokio::time::sleep(Duration::from_millis(10)).await;
            client.complete(request("new", "select ")).await.unwrap()
        });
        assert!(old.unwrap().is_none());
        assert_eq!(new.unwrap().text, "1 AS synthetic");
        let (cancelled, ()) =
            tokio::join!(client.complete(request("cancel", "select slow")), async {
                tokio::time::sleep(Duration::from_millis(10)).await;
                client.cancel("cancel").await;
            });
        assert!(cancelled.unwrap().is_none());
        client.set_enabled(false).await.unwrap();
    }

    #[tokio::test]
    async fn invalid_paths_and_oversized_documents_are_never_sent() {
        let directory = tempfile::tempdir().unwrap();
        let client = mock_client(directory.path()).await;
        let mut invalid = request("bad", "select ");
        invalid.document_id = "../../credentials".into();
        assert!(client.complete(invalid).await.is_err());
        assert!(
            client
                .complete(request("large", &"x".repeat(MAX_DOCUMENT + 1)))
                .await
                .unwrap()
                .is_none()
        );
        assert!(client.state.lock().await.pending.is_none());
        client.set_enabled(false).await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires explicit CATABASE_SUPERMAVEN_LIVE_TEST=1; sends synthetic SQL only"]
    async fn official_agent_synthetic_completion() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            std::env::var("CATABASE_SUPERMAVEN_LIVE_TEST")?.as_str(),
            "1"
        );
        let directory = tempfile::tempdir()?;
        let client = SupermavenClient::new(directory.path().to_owned());
        client.set_enabled(true).await?;
        let mut previous_status = String::new();
        let mut attempts = 0;
        for _ in 0..600 {
            let status = client.status().await;
            let status_summary = format!("{}: {}", status.state, status.message);
            if previous_status != status_summary {
                println!("Agent status: {status_summary}");
                previous_status = status_summary;
            }
            if matches!(status.state.as_str(), "ready" | "disconnected") {
                // Allow delayed account/announcement messages to arrive before
                // asserting readiness, not just the first connection event.
                tokio::time::sleep(Duration::from_millis(750)).await;
                // A new state_update is also the official agent's reconnect
                // trigger. Exercise that path with synthetic context only.
                attempts += 1;
                let mut probe = request("synthetic-live-test", "select * from prop");
                probe.document_id = "00000000-1111-2222-3333-444444444444".into();
                probe.schema_context = "CREATE TABLE public.properties (id text PRIMARY KEY, landlord_id text, address text);\nCREATE TABLE public.landlords (id text PRIMARY KEY, name text);".into();
                let result = client.complete(probe).await?;
                if result.is_none() {
                    let status = client.status().await;
                    if attempts < 3
                        && matches!(
                            status.state.as_str(),
                            "ready" | "connecting" | "disconnected"
                        )
                    {
                        continue;
                    }
                    client.set_enabled(false).await?;
                    return Err(format!(
                        "No synthetic completion after {attempts} attempts. Agent status: {}: {}",
                        status.state, status.message
                    )
                    .into());
                }
                client.set_enabled(false).await?;
                let result = result.unwrap();
                assert!(
                    result.text.starts_with("erties"),
                    "agent did not complete the reflected synthetic table: {}",
                    result.text
                );
                println!(
                    "Official Supermaven agent completed the synthetic properties table using schema context."
                );
                return Ok(());
            }
            if matches!(status.state.as_str(), "activation" | "error") {
                client.set_enabled(false).await?;
                return Err(format!("Supermaven requires user action: {}", status.message).into());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        client.set_enabled(false).await?;
        let state = client.state.lock().await;
        Err(format!("Supermaven startup timed out (stdout lines {}, protocol messages {}, message kinds {:?})", state.diagnostics.0, state.diagnostics.1, state.diagnostics.2).into())
    }
}
