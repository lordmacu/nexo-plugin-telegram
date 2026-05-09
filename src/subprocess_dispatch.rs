//! Phase 81.18 — subprocess `tool.invoke` dispatcher.
//!
//! Maps the 6 tool names declared by [`telegram_tool_defs`] to direct
//! [`BotClient`] calls (or [`dispatch_custom`] for the format-aware
//! payloads). The legacy in-tree handlers in [`tool`] publish to the
//! broker for in-process daemons; the subprocess path bypasses the
//! broker entirely so a `tool.invoke` round-trip is at most one HTTP
//! call to the Bot API.
//!
//! [`telegram_tool_defs`]: crate::subprocess_dispatch::telegram_tool_defs
//! [`tool`]: crate::tool

use std::sync::Arc;

use nexo_microapp_sdk::plugin::{ToolDef as SdkToolDef, ToolInvocation, ToolInvocationError};
use serde_json::{json, Value};

use nexo_core::agent::plugin::{Command, Response};

use crate::bot::BotClient;
use crate::plugin::{dispatch_custom, TelegramPlugin};
use crate::tool::{
    TelegramEditMessageTool, TelegramSendLocationTool, TelegramSendMediaTool,
    TelegramSendMessageTool, TelegramSendReactionTool, TelegramSendReplyTool,
};

/// Map an in-tree `nexo_llm::ToolDef` (used by the broker-published
/// handlers in [`tool`]) to the subprocess SDK shape declared in
/// `initialize`. Field rename: `parameters` → `input_schema`.
///
/// [`tool`]: crate::tool
fn to_sdk_def(d: nexo_llm::ToolDef) -> SdkToolDef {
    SdkToolDef {
        name: d.name,
        description: d.description,
        input_schema: d.parameters,
    }
}

/// The full list of tool defs the subprocess advertises in its
/// `initialize` reply. Daemon-side `RemoteToolHandler` validates each
/// name against the manifest's tools allowlist before exposing them
/// to agents.
pub fn telegram_tool_defs() -> Vec<SdkToolDef> {
    [
        TelegramSendMessageTool::tool_def(),
        TelegramSendReplyTool::tool_def(),
        TelegramSendReactionTool::tool_def(),
        TelegramEditMessageTool::tool_def(),
        TelegramSendLocationTool::tool_def(),
        TelegramSendMediaTool::tool_def(),
    ]
    .into_iter()
    .map(to_sdk_def)
    .collect()
}

/// Route a single `tool.invoke` request to the right `BotClient`
/// call. Caller is responsible for resolving the live [`TelegramPlugin`]
/// (the subprocess holds one via `OnceCell`) and passing it here.
///
/// Returns the JSON value placed in the JSON-RPC `result` field.
/// Errors map to `ToolInvocationError`:
///   * malformed args → [`ToolInvocationError::ArgumentInvalid`]
///   * Bot API failure → [`ToolInvocationError::Internal`]
///   * plugin not started → [`ToolInvocationError::Unavailable`]
pub async fn dispatch_telegram_tool(
    plugin: &TelegramPlugin,
    invocation: ToolInvocation,
) -> Result<Value, ToolInvocationError> {
    let bot = plugin.bot_handle().ok_or_else(|| {
        ToolInvocationError::Unavailable(
            "telegram subprocess plugin not started (no bot client)".into(),
        )
    })?;

    let args = invocation.args;
    match invocation.tool_name.as_str() {
        "telegram_send_message" => {
            let chat_id = parse_chat_id(&args)?;
            let text = require_str(&args, "text")?;
            // Reuse the plugin's chunking + parse_mode logic via the
            // public `Command` API so behaviour matches the legacy
            // broker path byte-for-byte.
            let resp = run_command(
                &bot,
                Command::SendMessage {
                    to: chat_id.to_string(),
                    text: text.to_string(),
                },
            )
            .await?;
            Ok(response_to_value(resp, json!({"chat_id": chat_id})))
        }
        "telegram_send_reply" => {
            let chat_id = parse_chat_id(&args)?;
            let reply_to = parse_int_field(&args, "reply_to_message_id")?;
            let text = require_str(&args, "text")?;
            let resp = dispatch_custom(
                &bot,
                "reply",
                json!({"chat_id": chat_id, "msg_id": reply_to, "text": text}),
            )
            .await
            .map_err(|e| ToolInvocationError::ExecutionFailed(e.to_string()))?;
            Ok(response_to_value(resp, json!({"chat_id": chat_id})))
        }
        "telegram_send_reaction" => {
            let chat_id = parse_chat_id(&args)?;
            let message_id = parse_int_field(&args, "message_id")?;
            let emoji = require_str(&args, "emoji")?;
            dispatch_custom(
                &bot,
                "reaction",
                json!({"chat_id": chat_id, "message_id": message_id, "emoji": emoji}),
            )
            .await
            .map_err(|e| ToolInvocationError::ExecutionFailed(e.to_string()))?;
            Ok(json!({"ok": true, "chat_id": chat_id, "message_id": message_id}))
        }
        "telegram_edit_message" => {
            let chat_id = parse_chat_id(&args)?;
            let message_id = parse_int_field(&args, "message_id")?;
            let text = require_str(&args, "text")?;
            let parse_mode = args.get("parse_mode").and_then(|v: &Value| v.as_str());
            let resp = dispatch_custom(
                &bot,
                "edit_message",
                json!({
                    "chat_id": chat_id,
                    "message_id": message_id,
                    "text": text,
                    "parse_mode": parse_mode,
                }),
            )
            .await
            .map_err(|e| ToolInvocationError::ExecutionFailed(e.to_string()))?;
            Ok(response_to_value(resp, json!({"chat_id": chat_id})))
        }
        "telegram_send_location" => {
            let chat_id = parse_chat_id(&args)?;
            let latitude = require_f64(&args, "latitude")?;
            let longitude = require_f64(&args, "longitude")?;
            let resp = dispatch_custom(
                &bot,
                "send_location",
                json!({"chat_id": chat_id, "latitude": latitude, "longitude": longitude}),
            )
            .await
            .map_err(|e| ToolInvocationError::ExecutionFailed(e.to_string()))?;
            Ok(response_to_value(resp, json!({"chat_id": chat_id})))
        }
        "telegram_send_media" => {
            // Media tools come through with the operator's verbatim
            // payload; the existing dispatch_custom branches handle
            // photo/audio/voice/video/document/animation. Tool name
            // disambiguates via the `kind` field.
            let chat_id = parse_chat_id(&args)?;
            let kind = require_str(&args, "kind")?;
            let custom_name = match kind {
                "photo" => "send_photo",
                "audio" => "send_audio",
                "voice" => "send_voice",
                "video" => "send_video",
                "document" => "send_document",
                "animation" => "send_animation",
                other => {
                    return Err(ToolInvocationError::ArgumentInvalid(format!(
                        "unknown media kind: {other:?}"
                    )))
                }
            };
            let mut payload = args.clone();
            // Custom branches expect `chat_id: i64`, ensure normalisation.
            payload["chat_id"] = json!(chat_id);
            let resp = dispatch_custom(&bot, custom_name, payload)
                .await
                .map_err(|e| ToolInvocationError::ExecutionFailed(e.to_string()))?;
            Ok(response_to_value(resp, json!({"chat_id": chat_id})))
        }
        other => Err(ToolInvocationError::NotFound(other.to_string())),
    }
}

async fn run_command(bot: &Arc<BotClient>, cmd: Command) -> Result<Response, ToolInvocationError> {
    // Replicate the matching block in `TelegramPlugin::send_command`
    // without holding a borrow on the plugin so callers can keep a
    // reference live across the await.
    match cmd {
        Command::SendMessage { to, text } => {
            let chat_id: i64 = to.parse().map_err(|_| {
                ToolInvocationError::ArgumentInvalid("`to` must be a chat id (integer)".to_string())
            })?;
            let sent = crate::plugin::send_text_chunked(bot, chat_id, &text, None, None)
                .await
                .map_err(|e| ToolInvocationError::ExecutionFailed(e.to_string()))?;
            Ok(Response::MessageSent { message_id: sent })
        }
        Command::SendMedia { .. } => Err(ToolInvocationError::ArgumentInvalid(
            "use telegram_send_media instead of generic SendMedia".into(),
        )),
        Command::Custom { name, payload } => dispatch_custom(bot, &name, payload)
            .await
            .map_err(|e| ToolInvocationError::ExecutionFailed(e.to_string())),
    }
}

fn parse_chat_id(args: &Value) -> Result<i64, ToolInvocationError> {
    if let Some(i) = args.get("chat_id").and_then(|v| v.as_i64()) {
        return Ok(i);
    }
    if let Some(s) = args.get("chat_id").and_then(|v| v.as_str()) {
        return s.parse::<i64>().map_err(|e| {
            ToolInvocationError::ArgumentInvalid(format!("chat_id not parseable: {e}"))
        });
    }
    Err(ToolInvocationError::ArgumentInvalid(
        "`chat_id` is required (integer or numeric string)".into(),
    ))
}

fn parse_int_field(args: &Value, field: &str) -> Result<i64, ToolInvocationError> {
    if let Some(i) = args.get(field).and_then(|v| v.as_i64()) {
        return Ok(i);
    }
    if let Some(s) = args.get(field).and_then(|v| v.as_str()) {
        return s.parse::<i64>().map_err(|e| {
            ToolInvocationError::ArgumentInvalid(format!("{field} not parseable: {e}"))
        });
    }
    Err(ToolInvocationError::ArgumentInvalid(format!(
        "`{field}` is required (integer or numeric string)"
    )))
}

fn require_str<'a>(args: &'a Value, field: &str) -> Result<&'a str, ToolInvocationError> {
    args.get(field).and_then(|v| v.as_str()).ok_or_else(|| {
        ToolInvocationError::ArgumentInvalid(format!("`{field}` is required (string)"))
    })
}

fn require_f64(args: &Value, field: &str) -> Result<f64, ToolInvocationError> {
    args.get(field).and_then(|v| v.as_f64()).ok_or_else(|| {
        ToolInvocationError::ArgumentInvalid(format!("`{field}` is required (number)"))
    })
}

fn response_to_value(resp: Response, extra: Value) -> Value {
    match resp {
        Response::MessageSent { message_id } => {
            let mut v = json!({"ok": true, "message_id": message_id});
            merge_into(&mut v, extra);
            v
        }
        Response::Ok => {
            let mut v = json!({"ok": true});
            merge_into(&mut v, extra);
            v
        }
        Response::Error { message } => json!({"ok": false, "error": message}),
        // `Custom` exists for plugin-defined response shapes; the
        // telegram tools never produce one today, but the match
        // must cover every variant. Surface as-is so the caller
        // can interpret without losing fidelity.
        Response::Custom { payload } => payload,
    }
}

fn merge_into(target: &mut Value, extra: Value) {
    if let (Some(t), Value::Object(map)) = (target.as_object_mut(), extra) {
        for (k, v) in map {
            t.insert(k, v);
        }
    }
}
