use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConnectionStatus {
    Disconnected,
    Connecting,
    Connected,
}

impl Default for ConnectionStatus {
    fn default() -> Self {
        Self::Disconnected
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionSnapshot {
    pub status: ConnectionStatus,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerConfig {
    pub websocket_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlleycatConfig {
    pub pair_payload: String,
    pub agent: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSummary {
    pub id: String,
    pub title: String,
    pub preview: String,
    pub status: String,
    pub cwd: Option<String>,
    pub updated_at: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationItem {
    pub id: String,
    pub kind: String,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadDetail {
    pub summary: ThreadSummary,
    pub items: Vec<ConversationItem>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebSnapshot {
    pub connection: ConnectionSnapshot,
    pub server_url: Option<String>,
    pub alleycat_agent: Option<String>,
    pub threads: Vec<ThreadSummary>,
    pub active_thread_id: Option<String>,
    pub active_thread: Option<ThreadDetail>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum WebEffect {
    OpenWebSocket { url: String },
    OpenAlleycat { config: AlleycatConfig },
    SendRpc { message: Value },
    PersistConfig { config: ServerConfig },
    PersistAlleycatConfig { config: AlleycatConfig },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebDispatchResult {
    pub snapshot: WebSnapshot,
    pub effects: Vec<WebEffect>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PendingRequest {
    method: String,
    thread_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WebModel {
    connection: ConnectionSnapshot,
    server: Option<ServerConfig>,
    alleycat: Option<AlleycatConfig>,
    threads: Vec<ThreadSummary>,
    active_thread_id: Option<String>,
    active_thread: Option<ThreadDetail>,
    last_error: Option<String>,
    next_request_id: i64,
    pending: BTreeMap<i64, PendingRequest>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum WebEvent {
    ConfigureServer {
        url: String,
    },
    ConfigureAlleycat {
        #[serde(rename = "pairPayload")]
        pair_payload: String,
        agent: String,
    },
    WebSocketOpened,
    WebSocketClosed {
        reason: Option<String>,
    },
    WebSocketMessage {
        text: String,
    },
    RequestThreads,
    SelectThread {
        #[serde(rename = "threadId")]
        thread_id: String,
    },
    SendTurn {
        #[serde(rename = "threadId")]
        thread_id: String,
        text: String,
    },
    ClearError,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum WebClientError {
    #[error("websocket URL must start with ws://, wss://, http://, or https://")]
    InvalidWebSocketUrl,
    #[error("invalid event JSON: {0}")]
    InvalidEvent(String),
    #[error("invalid websocket message JSON: {0}")]
    InvalidMessage(String),
    #[error("invalid alleycat pair payload: {0}")]
    InvalidAlleycatPayload(String),
    #[error("serialize result failed: {0}")]
    Serialize(String),
}

#[derive(Debug, Default, Clone)]
pub struct WebApp {
    model: WebModel,
}

impl WebApp {
    pub fn new() -> Self {
        Self {
            model: WebModel {
                next_request_id: 1,
                ..WebModel::default()
            },
        }
    }

    pub fn snapshot(&self) -> WebSnapshot {
        WebSnapshot {
            connection: self.model.connection.clone(),
            server_url: self
                .model
                .server
                .as_ref()
                .map(|server| server.websocket_url.clone()),
            alleycat_agent: self
                .model
                .alleycat
                .as_ref()
                .map(|config| config.agent.clone()),
            threads: self.model.threads.clone(),
            active_thread_id: self.model.active_thread_id.clone(),
            active_thread: self.model.active_thread.clone(),
            last_error: self.model.last_error.clone(),
        }
    }

    pub fn snapshot_json(&self) -> Result<String, WebClientError> {
        serde_json::to_string(&self.snapshot())
            .map_err(|error| WebClientError::Serialize(error.to_string()))
    }

    pub fn dispatch_json(&mut self, event_json: &str) -> Result<String, WebClientError> {
        let event: WebEvent = serde_json::from_str(event_json)
            .map_err(|error| WebClientError::InvalidEvent(error.to_string()))?;
        let result = self.dispatch(event)?;
        serde_json::to_string(&result).map_err(|error| WebClientError::Serialize(error.to_string()))
    }

    pub fn dispatch(&mut self, event: WebEvent) -> Result<WebDispatchResult, WebClientError> {
        let mut effects = Vec::new();

        match event {
            WebEvent::ConfigureServer { url } => {
                let websocket_url = normalize_websocket_url(&url)?;
                let config = ServerConfig { websocket_url };
                self.reset_connection_state();
                self.model.connection = ConnectionSnapshot {
                    status: ConnectionStatus::Connecting,
                    message: Some("Connecting".to_string()),
                };
                self.model.server = Some(config.clone());
                self.model.alleycat = None;
                self.model.last_error = None;
                effects.push(WebEffect::PersistConfig {
                    config: config.clone(),
                });
                effects.push(WebEffect::OpenWebSocket {
                    url: config.websocket_url,
                });
            }
            WebEvent::ConfigureAlleycat {
                pair_payload,
                agent,
            } => {
                let config = normalize_alleycat_config(pair_payload, agent)?;
                self.reset_connection_state();
                self.model.connection = ConnectionSnapshot {
                    status: ConnectionStatus::Connecting,
                    message: Some("Connecting".to_string()),
                };
                self.model.server = None;
                self.model.alleycat = Some(config.clone());
                self.model.last_error = None;
                effects.push(WebEffect::PersistAlleycatConfig {
                    config: config.clone(),
                });
                effects.push(WebEffect::OpenAlleycat { config });
            }
            WebEvent::WebSocketOpened => {
                self.model.connection = ConnectionSnapshot {
                    status: ConnectionStatus::Connecting,
                    message: Some("Initializing".to_string()),
                };
                effects.push(self.initialize_effect());
            }
            WebEvent::WebSocketClosed { reason } => {
                self.model.connection = ConnectionSnapshot {
                    status: ConnectionStatus::Disconnected,
                    message: reason,
                };
            }
            WebEvent::WebSocketMessage { text } => {
                self.handle_websocket_message(&text, &mut effects)?;
            }
            WebEvent::RequestThreads => {
                effects.push(self.thread_list_effect());
            }
            WebEvent::SelectThread { thread_id } => {
                self.model.active_thread_id = Some(thread_id.clone());
                effects.push(self.thread_read_effect(&thread_id));
            }
            WebEvent::SendTurn { thread_id, text } => {
                if text.trim().is_empty() {
                    self.model.last_error = Some("Message is empty".to_string());
                } else {
                    effects.push(self.turn_start_effect(&thread_id, text));
                }
            }
            WebEvent::ClearError => {
                self.model.last_error = None;
            }
        }

        Ok(WebDispatchResult {
            snapshot: self.snapshot(),
            effects,
        })
    }

    fn reset_connection_state(&mut self) {
        self.model.threads.clear();
        self.model.active_thread_id = None;
        self.model.active_thread = None;
        self.model.pending.clear();
    }

    fn handle_websocket_message(
        &mut self,
        text: &str,
        effects: &mut Vec<WebEffect>,
    ) -> Result<(), WebClientError> {
        let message: Value = serde_json::from_str(text)
            .map_err(|error| WebClientError::InvalidMessage(error.to_string()))?;

        if message.get("id").is_some() {
            self.handle_rpc_response(message, effects);
            return Ok(());
        }

        if let Some(method) = message.get("method").and_then(Value::as_str) {
            let params = message.get("params").cloned().unwrap_or(Value::Null);
            self.handle_notification(method, &params, effects);
        }

        Ok(())
    }

    fn handle_rpc_response(&mut self, message: Value, effects: &mut Vec<WebEffect>) {
        let Some(id) = message.get("id").and_then(Value::as_i64) else {
            return;
        };
        let pending = self.model.pending.remove(&id);

        if let Some(error) = message.get("error") {
            self.model.last_error = Some(format_rpc_error(error));
            return;
        }

        let Some(pending) = pending else {
            return;
        };
        let result = message.get("result").cloned().unwrap_or(Value::Null);

        match pending.method.as_str() {
            "initialize" => {
                self.model.connection = ConnectionSnapshot {
                    status: ConnectionStatus::Connected,
                    message: Some("Connected".to_string()),
                };
                effects.push(self.initialized_effect());
                effects.push(self.thread_list_effect());
            }
            "thread/list" => {
                self.model.threads = result
                    .get("data")
                    .and_then(Value::as_array)
                    .map(|threads| threads.iter().map(thread_summary_from_value).collect())
                    .unwrap_or_default();
            }
            "thread/read" => {
                if let Some(thread) = result.get("thread") {
                    self.model.active_thread = Some(thread_detail_from_value(thread));
                }
            }
            "turn/start" => {
                if let Some(thread_id) = pending.thread_id {
                    effects.push(self.thread_read_effect(&thread_id));
                }
            }
            _ => {}
        }
    }

    fn handle_notification(&mut self, method: &str, params: &Value, effects: &mut Vec<WebEffect>) {
        match method {
            "thread/started" => {
                if let Some(thread) = params.get("thread") {
                    self.upsert_thread(thread_summary_from_value(thread));
                }
            }
            "thread/statusChanged" => {
                if let (Some(thread_id), Some(status)) = (
                    params.get("threadId").and_then(Value::as_str),
                    params.get("status"),
                ) {
                    self.update_thread_status(thread_id, status_label(status));
                }
            }
            "thread/nameUpdated" => {
                if let Some(thread_id) = params.get("threadId").and_then(Value::as_str) {
                    let name = params
                        .get("threadName")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned);
                    self.update_thread_title(thread_id, name);
                }
            }
            "thread/archived" => {
                if let Some(thread_id) = params.get("threadId").and_then(Value::as_str) {
                    self.model.threads.retain(|thread| thread.id != thread_id);
                }
            }
            _ => {}
        }

        if let Some(thread_id) = notification_thread_id(params) {
            if self.model.active_thread_id.as_deref() == Some(thread_id) {
                effects.push(self.thread_read_effect(thread_id));
            }
        }
    }

    fn upsert_thread(&mut self, summary: ThreadSummary) {
        if let Some(existing) = self
            .model
            .threads
            .iter_mut()
            .find(|thread| thread.id == summary.id)
        {
            *existing = summary;
        } else {
            self.model.threads.insert(0, summary);
        }
    }

    fn update_thread_status(&mut self, thread_id: &str, status: String) {
        if let Some(thread) = self
            .model
            .threads
            .iter_mut()
            .find(|thread| thread.id == thread_id)
        {
            thread.status = status.clone();
        }
        if let Some(active) = self.model.active_thread.as_mut() {
            if active.summary.id == thread_id {
                active.summary.status = status;
            }
        }
    }

    fn update_thread_title(&mut self, thread_id: &str, title: Option<String>) {
        let Some(title) = title else {
            return;
        };
        if let Some(thread) = self
            .model
            .threads
            .iter_mut()
            .find(|thread| thread.id == thread_id)
        {
            thread.title = title.clone();
        }
        if let Some(active) = self.model.active_thread.as_mut() {
            if active.summary.id == thread_id {
                active.summary.title = title;
            }
        }
    }

    fn thread_list_effect(&mut self) -> WebEffect {
        self.rpc_effect(
            "thread/list",
            None,
            json!({
                "limit": 50,
                "sortKey": "updated_at",
                "sortDirection": "desc",
                "archived": false,
                "useStateDbOnly": false
            }),
        )
    }

    fn initialize_effect(&mut self) -> WebEffect {
        self.rpc_effect(
            "initialize",
            None,
            json!({
                "clientInfo": {
                    "name": "Litter",
                    "title": "Litter Web",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": {
                    "experimentalApi": true
                }
            }),
        )
    }

    fn initialized_effect(&self) -> WebEffect {
        WebEffect::SendRpc {
            message: json!({
                "method": "initialized"
            }),
        }
    }

    fn thread_read_effect(&mut self, thread_id: &str) -> WebEffect {
        self.rpc_effect(
            "thread/read",
            Some(thread_id.to_string()),
            json!({
                "threadId": thread_id,
                "includeTurns": true
            }),
        )
    }

    fn turn_start_effect(&mut self, thread_id: &str, text: String) -> WebEffect {
        self.rpc_effect(
            "turn/start",
            Some(thread_id.to_string()),
            json!({
                "threadId": thread_id,
                "input": [
                    {
                        "type": "text",
                        "text": text,
                        "textElements": []
                    }
                ]
            }),
        )
    }

    fn rpc_effect(&mut self, method: &str, thread_id: Option<String>, params: Value) -> WebEffect {
        let id = self.model.next_request_id;
        self.model.next_request_id += 1;
        self.model.pending.insert(
            id,
            PendingRequest {
                method: method.to_string(),
                thread_id,
            },
        );

        WebEffect::SendRpc {
            message: json!({
                "id": id,
                "method": method,
                "params": params
            }),
        }
    }
}

fn normalize_websocket_url(url: &str) -> Result<String, WebClientError> {
    let trimmed = url.trim();
    if trimmed.starts_with("ws://") || trimmed.starts_with("wss://") {
        Ok(trimmed.to_string())
    } else if let Some(rest) = trimmed.strip_prefix("http://") {
        Ok(format!("ws://{rest}"))
    } else if let Some(rest) = trimmed.strip_prefix("https://") {
        Ok(format!("wss://{rest}"))
    } else {
        Err(WebClientError::InvalidWebSocketUrl)
    }
}

fn normalize_alleycat_config(
    pair_payload: String,
    agent: String,
) -> Result<AlleycatConfig, WebClientError> {
    let pair_payload = pair_payload.trim().to_string();
    let agent = agent.trim().to_string();
    if pair_payload.is_empty() {
        return Err(WebClientError::InvalidAlleycatPayload(
            "pair payload is empty".to_string(),
        ));
    }
    if agent.is_empty() {
        return Err(WebClientError::InvalidAlleycatPayload(
            "agent is empty".to_string(),
        ));
    }
    let value: Value = serde_json::from_str(&pair_payload)
        .map_err(|error| WebClientError::InvalidAlleycatPayload(error.to_string()))?;
    if value.get("v").and_then(Value::as_u64) != Some(1) {
        return Err(WebClientError::InvalidAlleycatPayload(
            "pair payload must have v=1".to_string(),
        ));
    }
    if value
        .get("node_id")
        .and_then(Value::as_str)
        .is_none_or(|node_id| node_id.trim().is_empty())
    {
        return Err(WebClientError::InvalidAlleycatPayload(
            "pair payload must include node_id".to_string(),
        ));
    }
    if value
        .get("token")
        .and_then(Value::as_str)
        .is_none_or(|token| token.trim().is_empty())
    {
        return Err(WebClientError::InvalidAlleycatPayload(
            "pair payload must include token".to_string(),
        ));
    }
    Ok(AlleycatConfig {
        pair_payload,
        agent,
    })
}

fn format_rpc_error(error: &Value) -> String {
    error
        .get("message")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| error.to_string())
}

fn thread_summary_from_value(thread: &Value) -> ThreadSummary {
    let preview = string_field(thread, "preview").unwrap_or_default();
    let title = string_field(thread, "name")
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| first_line_or_fallback(&preview, "Untitled thread"));

    ThreadSummary {
        id: string_field(thread, "id").unwrap_or_default(),
        title,
        preview,
        status: thread
            .get("status")
            .map(status_label)
            .unwrap_or_else(|| "unknown".to_string()),
        cwd: string_field(thread, "cwd"),
        updated_at: thread.get("updatedAt").and_then(Value::as_i64),
    }
}

fn thread_detail_from_value(thread: &Value) -> ThreadDetail {
    let summary = thread_summary_from_value(thread);
    let items = thread
        .get("turns")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|turn| {
            turn.get("items")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(conversation_item_from_value)
        })
        .collect();

    ThreadDetail { summary, items }
}

fn conversation_item_from_value(item: &Value) -> ConversationItem {
    let kind = string_field(item, "type").unwrap_or_else(|| "unknown".to_string());
    let id = string_field(item, "id").unwrap_or_else(|| format!("item-{}", kind));
    let text = match kind.as_str() {
        "userMessage" => item
            .get("content")
            .and_then(Value::as_array)
            .map(|content| {
                content
                    .iter()
                    .filter_map(|input| string_field(input, "text"))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default(),
        "agentMessage" | "plan" => string_field(item, "text").unwrap_or_default(),
        "reasoning" => item
            .get("summary")
            .or_else(|| item.get("content"))
            .and_then(Value::as_array)
            .map(|lines| {
                lines
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default(),
        "commandExecution" => {
            let command = string_field(item, "command").unwrap_or_default();
            let output = string_field(item, "aggregatedOutput").unwrap_or_default();
            if output.is_empty() {
                command
            } else {
                format!("{command}\n\n{output}")
            }
        }
        _ => item.to_string(),
    };

    ConversationItem { id, kind, text }
}

fn status_label(status: &Value) -> String {
    let base = string_field(status, "type").unwrap_or_else(|| "unknown".to_string());
    let flags = status
        .get("activeFlags")
        .and_then(Value::as_array)
        .map(|flags| {
            flags
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();

    if flags.is_empty() {
        base
    } else {
        format!("{base}: {flags}")
    }
}

fn notification_thread_id(params: &Value) -> Option<&str> {
    params.get("threadId").and_then(Value::as_str).or_else(|| {
        params
            .get("thread")
            .and_then(|thread| thread.get("id"))
            .and_then(Value::as_str)
    })
}

fn string_field(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn first_line_or_fallback(value: &str, fallback: &str) -> String {
    value
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

#[cfg(target_arch = "wasm32")]
mod wasm_exports {
    use super::{WebApp, WebClientError};
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
    use getrandom::fill as fill_random;
    use iroh::endpoint::{Connection, RecvStream, SendStream, VarInt};
    use iroh::{Endpoint, EndpointAddr, EndpointId, RelayUrl, SecretKey};
    use serde::{Deserialize, Serialize};
    use sha1::{Digest, Sha1};
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::str::FromStr;
    use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
    use wasm_bindgen::prelude::*;

    const ALLEYCAT_PROTOCOL_VERSION: u32 = 1;
    const ALLEYCAT_ALPN: &[u8] = b"alleycat/1";
    const MAX_FRAME_BYTES: usize = 1024 * 1024;
    const MAX_WS_MESSAGE_BYTES: usize = 128 * 1024 * 1024;

    #[wasm_bindgen]
    pub struct WasmWebClient {
        inner: WebApp,
    }

    #[wasm_bindgen]
    impl WasmWebClient {
        #[wasm_bindgen(constructor)]
        pub fn new() -> Self {
            Self {
                inner: WebApp::new(),
            }
        }

        #[wasm_bindgen(js_name = snapshotJson)]
        pub fn snapshot_json(&self) -> Result<String, JsValue> {
            self.inner.snapshot_json().map_err(to_js_error)
        }

        #[wasm_bindgen(js_name = dispatchEventJson)]
        pub fn dispatch_event_json(&mut self, event_json: &str) -> Result<String, JsValue> {
            self.inner.dispatch_json(event_json).map_err(to_js_error)
        }
    }

    #[wasm_bindgen]
    pub struct WasmAlleycatConnection {
        _endpoint: Endpoint,
        connection: Connection,
        send: Rc<RefCell<Option<SendStream>>>,
        recv: Rc<RefCell<Option<RecvStream>>>,
    }

    #[wasm_bindgen]
    impl WasmAlleycatConnection {
        #[wasm_bindgen(js_name = connect)]
        pub async fn connect(pair_payload_json: String, agent: String) -> Result<Self, JsValue> {
            let params = parse_pair_payload(&pair_payload_json).map_err(to_js_string_error)?;
            let agent = agent.trim().to_string();
            if agent.is_empty() {
                return Err(JsValue::from_str("agent is empty"));
            }

            let endpoint = Endpoint::builder(iroh::endpoint::presets::N0)
                .secret_key(SecretKey::generate())
                .bind()
                .await
                .map_err(to_js_string_error)?;
            let (connection, mut send, mut recv) = open_stream_on(&endpoint, &params)
                .await
                .map_err(to_js_string_error)?;
            write_json_frame(
                &mut send,
                &AlleycatRequest::Connect {
                    v: ALLEYCAT_PROTOCOL_VERSION,
                    token: params.token.clone(),
                    agent,
                },
            )
            .await
            .map_err(to_js_string_error)?;
            let response: AlleycatResponse = read_json_frame(&mut recv)
                .await
                .map_err(to_js_string_error)?;
            validate_response(&response).map_err(to_js_string_error)?;
            websocket_handshake(&mut send, &mut recv)
                .await
                .map_err(to_js_string_error)?;

            Ok(Self {
                _endpoint: endpoint,
                connection,
                send: Rc::new(RefCell::new(Some(send))),
                recv: Rc::new(RefCell::new(Some(recv))),
            })
        }

        #[wasm_bindgen(js_name = sendJson)]
        pub async fn send_json(&self, text: String) -> Result<(), JsValue> {
            let mut send = take_stream(&self.send, "send stream is busy")?;
            let result = write_ws_text(&mut send, text.as_bytes()).await;
            put_stream(&self.send, send);
            result.map_err(to_js_string_error)
        }

        #[wasm_bindgen(js_name = nextMessageJson)]
        pub async fn next_message_json(&self) -> Result<JsValue, JsValue> {
            let mut recv = take_stream(&self.recv, "receive stream is busy")?;
            let result = loop {
                match read_ws_frame(&mut recv).await {
                    Ok(WsFrame::Text(text)) => break Ok(JsValue::from_str(&text)),
                    Ok(WsFrame::Close) => break Ok(JsValue::NULL),
                    Ok(WsFrame::Ping(payload)) => {
                        let mut send = take_stream(&self.send, "send stream is busy")?;
                        if let Err(error) = write_ws_control(&mut send, 0xA, &payload).await {
                            put_stream(&self.send, send);
                            break Err(error);
                        }
                        put_stream(&self.send, send);
                    }
                    Ok(WsFrame::Pong) => {}
                    Err(error) => break Err(error),
                }
            };
            put_stream(&self.recv, recv);
            result.map_err(to_js_string_error)
        }

        pub fn close(&self) {
            self.connection
                .close(VarInt::from_u32(0), b"web disconnect");
        }
    }

    fn to_js_error(error: WebClientError) -> JsValue {
        JsValue::from_str(&error.to_string())
    }

    fn to_js_string_error(error: impl ToString) -> JsValue {
        JsValue::from_str(&error.to_string())
    }

    fn take_stream<T>(slot: &Rc<RefCell<Option<T>>>, message: &str) -> Result<T, JsValue> {
        slot.borrow_mut()
            .take()
            .ok_or_else(|| JsValue::from_str(message))
    }

    fn put_stream<T>(slot: &Rc<RefCell<Option<T>>>, value: T) {
        *slot.borrow_mut() = Some(value);
    }

    #[derive(Debug, Deserialize)]
    struct PairPayloadWire {
        v: u32,
        node_id: String,
        token: String,
        relay: Option<String>,
    }

    #[derive(Debug, Clone)]
    struct ParsedPairPayload {
        node_id: String,
        token: String,
        relay: Option<String>,
    }

    #[derive(Debug, Serialize)]
    #[serde(tag = "op", rename_all = "snake_case")]
    enum AlleycatRequest {
        Connect {
            v: u32,
            token: String,
            agent: String,
        },
    }

    #[derive(Debug, Deserialize)]
    struct AlleycatResponse {
        v: u32,
        ok: bool,
        error: Option<String>,
    }

    fn parse_pair_payload(json: &str) -> Result<ParsedPairPayload, String> {
        let wire: PairPayloadWire =
            serde_json::from_str(json).map_err(|error| format!("malformed JSON: {error}"))?;
        if wire.v != ALLEYCAT_PROTOCOL_VERSION {
            return Err(format!(
                "protocol version mismatch: payload={} client={ALLEYCAT_PROTOCOL_VERSION}",
                wire.v
            ));
        }
        if wire.node_id.trim().is_empty() {
            return Err("empty node_id".to_string());
        }
        EndpointId::from_str(&wire.node_id).map_err(|error| format!("invalid node_id: {error}"))?;
        if wire.token.trim().is_empty() {
            return Err("empty token".to_string());
        }
        if let Some(relay) = wire.relay.as_deref() {
            RelayUrl::from_str(relay).map_err(|error| format!("invalid relay URL: {error}"))?;
        }
        Ok(ParsedPairPayload {
            node_id: wire.node_id,
            token: wire.token,
            relay: wire.relay,
        })
    }

    async fn open_stream_on(
        endpoint: &Endpoint,
        params: &ParsedPairPayload,
    ) -> Result<(Connection, SendStream, RecvStream), String> {
        let id = EndpointId::from_str(&params.node_id)
            .map_err(|error| format!("invalid node_id: {error}"))?;
        let mut addr = EndpointAddr::new(id);
        if let Some(relay) = params.relay.as_deref() {
            let relay =
                RelayUrl::from_str(relay).map_err(|error| format!("invalid relay URL: {error}"))?;
            addr = addr.with_relay_url(relay);
        }
        let conn = endpoint
            .connect(addr, ALLEYCAT_ALPN)
            .await
            .map_err(|error| format!("connecting iroh endpoint: {error}"))?;
        let (send, recv) = conn
            .open_bi()
            .await
            .map_err(|error| format!("opening iroh stream: {error}"))?;
        Ok((conn, send, recv))
    }

    async fn read_json_frame<T, R>(reader: &mut R) -> Result<T, String>
    where
        T: for<'de> Deserialize<'de>,
        R: AsyncRead + Unpin,
    {
        let len = reader
            .read_u32()
            .await
            .map_err(|error| format!("reading frame length: {error}"))? as usize;
        if len > MAX_FRAME_BYTES {
            return Err(format!("frame too large: {len} bytes"));
        }
        let mut buf = vec![0u8; len];
        reader
            .read_exact(&mut buf)
            .await
            .map_err(|error| format!("reading frame body: {error}"))?;
        serde_json::from_slice(&buf).map_err(|error| format!("decoding frame JSON: {error}"))
    }

    async fn write_json_frame<T, W>(writer: &mut W, value: &T) -> Result<(), String>
    where
        T: Serialize,
        W: AsyncWrite + Unpin,
    {
        let buf =
            serde_json::to_vec(value).map_err(|error| format!("encoding frame JSON: {error}"))?;
        if buf.len() > MAX_FRAME_BYTES {
            return Err(format!("frame too large: {} bytes", buf.len()));
        }
        writer
            .write_u32(buf.len() as u32)
            .await
            .map_err(|error| format!("writing frame length: {error}"))?;
        writer
            .write_all(&buf)
            .await
            .map_err(|error| format!("writing frame body: {error}"))?;
        writer
            .flush()
            .await
            .map_err(|error| format!("flushing frame: {error}"))?;
        Ok(())
    }

    async fn websocket_handshake<W, R>(writer: &mut W, reader: &mut R) -> Result<(), String>
    where
        W: AsyncWrite + Unpin,
        R: AsyncRead + Unpin,
    {
        let mut nonce = [0u8; 16];
        fill_random(&mut nonce).map_err(|error| format!("generating websocket key: {error}"))?;
        let key = BASE64.encode(nonce);
        let request = format!(
            "GET /rpc HTTP/1.1\r\n\
             Host: codex-app-server-proxy.localhost\r\n\
             Connection: Upgrade\r\n\
             Upgrade: websocket\r\n\
             Sec-WebSocket-Version: 13\r\n\
             Sec-WebSocket-Key: {key}\r\n\
             \r\n"
        );
        writer
            .write_all(request.as_bytes())
            .await
            .map_err(|error| format!("writing websocket handshake: {error}"))?;
        writer
            .flush()
            .await
            .map_err(|error| format!("flushing websocket handshake: {error}"))?;

        let response = read_http_response(reader).await?;
        let expected_accept = websocket_accept_key(&key);
        validate_websocket_handshake_response(&response, &expected_accept)
    }

    async fn read_http_response<R>(reader: &mut R) -> Result<String, String>
    where
        R: AsyncRead + Unpin,
    {
        let mut buf = Vec::new();
        let mut byte = [0u8; 1];
        while buf.len() < 8192 {
            reader
                .read_exact(&mut byte)
                .await
                .map_err(|error| format!("reading websocket handshake: {error}"))?;
            buf.push(byte[0]);
            if buf.ends_with(b"\r\n\r\n") {
                return String::from_utf8(buf)
                    .map_err(|error| format!("decoding websocket handshake: {error}"));
            }
        }
        Err("websocket handshake response too large".to_string())
    }

    fn websocket_accept_key(key: &str) -> String {
        let mut sha1 = Sha1::new();
        sha1.update(key.as_bytes());
        sha1.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
        BASE64.encode(sha1.finalize())
    }

    fn validate_websocket_handshake_response(
        response: &str,
        expected_accept: &str,
    ) -> Result<(), String> {
        let mut lines = response.split("\r\n");
        let status = lines.next().unwrap_or_default();
        if !status.starts_with("HTTP/1.1 101") && !status.starts_with("HTTP/1.0 101") {
            return Err(format!("websocket upgrade failed: {status}"));
        }

        let mut accept = None;
        for line in lines {
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            if name.eq_ignore_ascii_case("sec-websocket-accept") {
                accept = Some(value.trim());
            }
        }

        match accept {
            Some(value) if value == expected_accept => Ok(()),
            Some(value) => Err(format!(
                "websocket accept mismatch: expected {expected_accept}, got {value}"
            )),
            None => Err("websocket upgrade missing Sec-WebSocket-Accept".to_string()),
        }
    }

    fn validate_response(response: &AlleycatResponse) -> Result<(), String> {
        if response.v != ALLEYCAT_PROTOCOL_VERSION {
            return Err(format!(
                "protocol version mismatch: host={} client={ALLEYCAT_PROTOCOL_VERSION}",
                response.v
            ));
        }
        if !response.ok {
            return Err(response
                .error
                .clone()
                .unwrap_or_else(|| "host rejected request".to_string()));
        }
        Ok(())
    }

    enum WsFrame {
        Text(String),
        Ping(Vec<u8>),
        Pong,
        Close,
    }

    async fn write_ws_text<W>(writer: &mut W, payload: &[u8]) -> Result<(), String>
    where
        W: AsyncWrite + Unpin,
    {
        write_ws_frame(writer, 0x1, payload, true).await
    }

    async fn write_ws_control<W>(writer: &mut W, opcode: u8, payload: &[u8]) -> Result<(), String>
    where
        W: AsyncWrite + Unpin,
    {
        if payload.len() > 125 {
            return Err("websocket control frame too large".to_string());
        }
        write_ws_frame(writer, opcode, payload, true).await
    }

    async fn write_ws_frame<W>(
        writer: &mut W,
        opcode: u8,
        payload: &[u8],
        masked: bool,
    ) -> Result<(), String>
    where
        W: AsyncWrite + Unpin,
    {
        let mut header = Vec::with_capacity(14);
        header.push(0x80 | (opcode & 0x0f));
        let mask_bit = if masked { 0x80 } else { 0 };
        match payload.len() {
            len @ 0..=125 => header.push(mask_bit | len as u8),
            len @ 126..=65_535 => {
                header.push(mask_bit | 126);
                header.extend_from_slice(&(len as u16).to_be_bytes());
            }
            len => {
                header.push(mask_bit | 127);
                header.extend_from_slice(&(len as u64).to_be_bytes());
            }
        }

        if masked {
            let mut mask = [0u8; 4];
            fill_random(&mut mask)
                .map_err(|error| format!("generating websocket mask: {error}"))?;
            header.extend_from_slice(&mask);
            writer
                .write_all(&header)
                .await
                .map_err(|error| format!("writing websocket header: {error}"))?;
            let mut masked_payload = payload.to_vec();
            for (index, byte) in masked_payload.iter_mut().enumerate() {
                *byte ^= mask[index % 4];
            }
            writer
                .write_all(&masked_payload)
                .await
                .map_err(|error| format!("writing websocket payload: {error}"))?;
        } else {
            writer
                .write_all(&header)
                .await
                .map_err(|error| format!("writing websocket header: {error}"))?;
            writer
                .write_all(payload)
                .await
                .map_err(|error| format!("writing websocket payload: {error}"))?;
        }
        writer
            .flush()
            .await
            .map_err(|error| format!("flushing websocket frame: {error}"))?;
        Ok(())
    }

    async fn read_ws_frame<R>(reader: &mut R) -> Result<WsFrame, String>
    where
        R: AsyncRead + Unpin,
    {
        loop {
            let mut header = [0u8; 2];
            reader
                .read_exact(&mut header)
                .await
                .map_err(|error| format!("reading websocket header: {error}"))?;
            let opcode = header[0] & 0x0f;
            let masked = header[1] & 0x80 != 0;
            let mut len = (header[1] & 0x7f) as u64;
            if len == 126 {
                len = reader
                    .read_u16()
                    .await
                    .map_err(|error| format!("reading websocket 16-bit length: {error}"))?
                    as u64;
            } else if len == 127 {
                len = reader
                    .read_u64()
                    .await
                    .map_err(|error| format!("reading websocket 64-bit length: {error}"))?;
            }
            if len as usize > MAX_WS_MESSAGE_BYTES {
                return Err(format!("websocket message too large: {len} bytes"));
            }

            let mut mask = [0u8; 4];
            if masked {
                reader
                    .read_exact(&mut mask)
                    .await
                    .map_err(|error| format!("reading websocket mask: {error}"))?;
            }
            let mut payload = vec![0u8; len as usize];
            reader
                .read_exact(&mut payload)
                .await
                .map_err(|error| format!("reading websocket payload: {error}"))?;
            if masked {
                for (index, byte) in payload.iter_mut().enumerate() {
                    *byte ^= mask[index % 4];
                }
            }

            match opcode {
                0x0 => continue,
                0x1 => {
                    return String::from_utf8(payload)
                        .map(WsFrame::Text)
                        .map_err(|error| format!("decoding websocket text: {error}"));
                }
                0x8 => return Ok(WsFrame::Close),
                0x9 => return Ok(WsFrame::Ping(payload)),
                0xA => return Ok(WsFrame::Pong),
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn configure_server_normalizes_http_to_ws_and_opens_socket() {
        let mut app = WebApp::new();

        let result = app
            .dispatch(WebEvent::ConfigureServer {
                url: "http://127.0.0.1:4500/rpc".to_string(),
            })
            .expect("configure server");

        assert_eq!(
            result.snapshot.connection.status,
            ConnectionStatus::Connecting
        );
        assert_eq!(
            result.snapshot.server_url.as_deref(),
            Some("ws://127.0.0.1:4500/rpc")
        );
        assert_eq!(result.snapshot.alleycat_agent, None);
        assert_eq!(
            result.effects,
            vec![
                WebEffect::PersistConfig {
                    config: ServerConfig {
                        websocket_url: "ws://127.0.0.1:4500/rpc".to_string()
                    }
                },
                WebEffect::OpenWebSocket {
                    url: "ws://127.0.0.1:4500/rpc".to_string()
                }
            ]
        );
    }

    #[test]
    fn configure_alleycat_clears_websocket_config_and_opens_alleycat() {
        let mut app = WebApp::new();
        app.dispatch(WebEvent::ConfigureServer {
            url: "ws://127.0.0.1:8390".to_string(),
        })
        .expect("configure server");

        let result = app
            .dispatch(WebEvent::ConfigureAlleycat {
                pair_payload:
                    r#"{"v":1,"node_id":"node-a","token":"token-a","relay":null}"#.to_string(),
                agent: " codex ".to_string(),
            })
            .expect("configure alleycat");

        assert_eq!(
            result.snapshot.connection.status,
            ConnectionStatus::Connecting
        );
        assert_eq!(result.snapshot.server_url, None);
        assert_eq!(result.snapshot.alleycat_agent.as_deref(), Some("codex"));
        assert_eq!(
            result.effects,
            vec![
                WebEffect::PersistAlleycatConfig {
                    config: AlleycatConfig {
                        pair_payload:
                            r#"{"v":1,"node_id":"node-a","token":"token-a","relay":null}"#
                                .to_string(),
                        agent: "codex".to_string()
                    }
                },
                WebEffect::OpenAlleycat {
                    config: AlleycatConfig {
                        pair_payload:
                            r#"{"v":1,"node_id":"node-a","token":"token-a","relay":null}"#
                                .to_string(),
                        agent: "codex".to_string()
                    }
                }
            ]
        );
    }

    #[test]
    fn web_socket_opened_requests_initialize() {
        let mut app = WebApp::new();

        let result = app
            .dispatch(WebEvent::WebSocketOpened)
            .expect("websocket opened");

        assert_eq!(
            result.snapshot.connection.status,
            ConnectionStatus::Connecting
        );
        assert_eq!(
            result.snapshot.connection.message.as_deref(),
            Some("Initializing")
        );
        assert_eq!(result.effects.len(), 1);
        let WebEffect::SendRpc { message } = &result.effects[0] else {
            panic!("expected SendRpc");
        };
        assert_eq!(message["method"], "initialize");
        assert_eq!(message["params"]["clientInfo"]["name"], "Litter");
        assert_eq!(message["params"]["clientInfo"]["title"], "Litter Web");
        assert_eq!(message["params"]["capabilities"]["experimentalApi"], true);
    }

    #[test]
    fn initialize_response_sends_initialized_and_thread_list() {
        let mut app = WebApp::new();
        let opened = app
            .dispatch(WebEvent::WebSocketOpened)
            .expect("websocket opened");
        let WebEffect::SendRpc { message } = &opened.effects[0] else {
            panic!("expected SendRpc");
        };
        let id = message["id"].as_i64().expect("id");

        let result = app
            .dispatch(WebEvent::WebSocketMessage {
                text: json!({
                    "id": id,
                    "result": {
                        "userAgent": "codex-test",
                        "codexHome": "/tmp/codex",
                        "platformFamily": "unix",
                        "platformOs": "linux"
                    }
                })
                .to_string(),
            })
            .expect("initialize response");

        assert_eq!(
            result.snapshot.connection.status,
            ConnectionStatus::Connected
        );
        assert_eq!(
            result.snapshot.connection.message.as_deref(),
            Some("Connected")
        );
        assert_eq!(result.effects.len(), 2);
        let WebEffect::SendRpc {
            message: initialized,
        } = &result.effects[0]
        else {
            panic!("expected initialized notification");
        };
        assert_eq!(initialized["method"], "initialized");
        assert!(initialized.get("id").is_none());
        let WebEffect::SendRpc {
            message: thread_list,
        } = &result.effects[1]
        else {
            panic!("expected thread/list request");
        };
        assert_eq!(thread_list["method"], "thread/list");
        assert_eq!(thread_list["params"]["limit"], 50);
    }

    #[test]
    fn thread_list_response_updates_snapshot() {
        let mut app = initialized_app();

        let result = app
            .dispatch(WebEvent::RequestThreads)
            .expect("request threads");
        let WebEffect::SendRpc { message } = &result.effects[0] else {
            panic!("expected SendRpc");
        };
        let id = message["id"].as_i64().expect("id");

        let response = json!({
            "id": id,
            "result": {
                "data": [
                    {
                        "id": "thread-1",
                        "preview": "Build PWA",
                        "status": {"type": "idle"},
                        "cwd": "/workspace",
                        "updatedAt": 42
                    }
                ],
                "nextCursor": null,
                "backwardsCursor": null
            }
        });
        let result = app
            .dispatch(WebEvent::WebSocketMessage {
                text: response.to_string(),
            })
            .expect("message");

        assert_eq!(result.snapshot.threads.len(), 1);
        assert_eq!(result.snapshot.threads[0].id, "thread-1");
        assert_eq!(result.snapshot.threads[0].title, "Build PWA");
    }

    fn initialized_app() -> WebApp {
        let mut app = WebApp::new();
        let opened = app
            .dispatch(WebEvent::WebSocketOpened)
            .expect("websocket opened");
        let WebEffect::SendRpc { message } = &opened.effects[0] else {
            panic!("expected initialize request");
        };
        let id = message["id"].as_i64().expect("id");
        app.dispatch(WebEvent::WebSocketMessage {
            text: json!({
                "id": id,
                "result": {
                    "userAgent": "codex-test",
                    "codexHome": "/tmp/codex",
                    "platformFamily": "unix",
                    "platformOs": "linux"
                }
            })
            .to_string(),
        })
        .expect("initialize response");
        app
    }

    #[test]
    fn select_thread_requests_thread_read_with_turns() {
        let mut app = WebApp::new();

        let result = app
            .dispatch(WebEvent::SelectThread {
                thread_id: "thread-1".to_string(),
            })
            .expect("select thread");

        let WebEffect::SendRpc { message } = &result.effects[0] else {
            panic!("expected SendRpc");
        };
        assert_eq!(message["method"], "thread/read");
        assert_eq!(message["params"]["threadId"], "thread-1");
        assert_eq!(message["params"]["includeTurns"], true);
    }

    #[test]
    fn send_turn_builds_turn_start_text_input() {
        let mut app = WebApp::new();

        let result = app
            .dispatch(WebEvent::SendTurn {
                thread_id: "thread-1".to_string(),
                text: "Implement plan".to_string(),
            })
            .expect("send turn");

        let WebEffect::SendRpc { message } = &result.effects[0] else {
            panic!("expected SendRpc");
        };
        assert_eq!(message["method"], "turn/start");
        assert_eq!(message["params"]["threadId"], "thread-1");
        assert_eq!(message["params"]["input"][0]["type"], "text");
        assert_eq!(message["params"]["input"][0]["text"], "Implement plan");
    }

    #[test]
    fn dispatch_json_accepts_js_camel_case_event_fields() {
        let mut app = WebApp::new();

        let json = app
            .dispatch_json(r#"{"type":"selectThread","threadId":"thread-1"}"#)
            .expect("dispatch json");
        let result: WebDispatchResult = serde_json::from_str(&json).expect("result json");

        let WebEffect::SendRpc { message } = &result.effects[0] else {
            panic!("expected SendRpc");
        };
        assert_eq!(message["method"], "thread/read");
        assert_eq!(message["params"]["threadId"], "thread-1");
    }
}
