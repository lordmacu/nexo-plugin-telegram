//! Phase 81.33.b.real Stages 1+2+4+5 — auto-discovery broker
//! handlers.
//!
//! Pure functions that take a JSON request payload + return a
//! JSON response payload. Wired in `src/main.rs` via the
//! `with_stdio_bridge_broker` subscription loop: incoming
//! broker.event for each of the registered topics →
//! `serde_json::from_value::<Message>(event.payload)` →
//! invoke handler → publish reply to `msg.reply_to`.
//!
//! Contract docs live in the daemon repo:
//! - Pairing adapter — `proyecto/docs/src/plugins/manifest-pairing-adapter.md`
//! - HTTP routes      — `proyecto/docs/src/plugins/manifest-http.md`
//! - Admin RPC        — `proyecto/docs/src/plugins/manifest-admin.md`
//! - Metrics scrape   — `proyecto/docs/src/plugins/manifest-metrics.md`

use base64::Engine;
use serde_json::{json, Value};

// ── Stage 1 — pairing adapter ──────────────────────────────────

/// Canonicalise an inbound Telegram sender id.
///
/// Telegram callers arrive as either `@handle` or numeric chat
/// id. The legacy `TelegramPairingAdapter::normalize_sender`
/// (in `pairing_adapter.rs`) lowercases the `@handle` form and
/// passes numerics through; this handler mirrors that logic so
/// the daemon's `GenericBrokerPairingAdapter` returns the same
/// canonical form via broker RPC.
///
/// Request: `{ "raw": "<raw-sender>" }`
/// Reply:   `{ "normalized": "<canonical>" }` or
///          `{ "normalized": null }` to reject.
pub fn pairing_normalize_sender(request: &Value) -> Value {
    let raw = request.get("raw").and_then(|v| v.as_str()).unwrap_or("");
    if raw.is_empty() {
        return json!({ "normalized": null });
    }
    // Numeric chat_id passes through unchanged.
    if raw.parse::<i64>().is_ok() {
        return json!({ "normalized": raw });
    }
    // Handle form: must start with `@`, lowercase, ASCII alnum +
    // underscore, length 5-32 per Telegram username rules.
    let normalized = if let Some(rest) = raw.strip_prefix('@') {
        let lower = rest.to_lowercase();
        if lower.len() < 5
            || lower.len() > 32
            || !lower.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return json!({ "normalized": null });
        }
        format!("@{lower}")
    } else {
        return json!({ "normalized": null });
    };
    json!({ "normalized": normalized })
}

/// Deliver a plain-text reply to a Telegram sender. The legacy
/// telegram outbound path issues a `sendMessage` Bot API call;
/// this handler ack-only stub returns OK so the
/// `GenericBrokerPairingAdapter::send_reply` round-trip
/// completes. A future iteration wires this into the real bot
/// outbound dispatcher (Phase 81.33.b.real Stage 7 cleanup
/// retires the legacy direct-call path).
pub fn pairing_send_reply(request: &Value) -> Value {
    let account = request.get("account").and_then(|v| v.as_str()).unwrap_or("");
    let to = request.get("to").and_then(|v| v.as_str()).unwrap_or("");
    if account.is_empty() || to.is_empty() {
        return json!({ "ok": false, "error": "account and to required" });
    }
    // TODO(Stage-7): route via existing telegram bot dispatcher
    // (see `bot.rs` `send_text`). For now ack so the broker
    // round-trip completes; the legacy
    // `PairingChannelAdapter::send_reply` path on the daemon
    // is still active for unmigrated runs.
    tracing::info!(
        account, to, "telegram auto-discovery pairing.send_reply ack",
    );
    json!({ "ok": true })
}

/// Send a QR PNG. Telegram supports image messages but the
/// pairing flow normally delivers a 6-digit code (text). The
/// handler validates base64 + acks; real send-photo wiring is
/// a Stage 7 follow-up.
pub fn pairing_send_qr_image(request: &Value) -> Value {
    let png_b64 = request
        .get("png_base64")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    match base64::engine::general_purpose::STANDARD.decode(png_b64.as_bytes()) {
        Ok(bytes) if !bytes.is_empty() => {
            let _: Vec<u8> = bytes;
            json!({ "ok": true })
        }
        Ok(_) => json!({ "ok": false, "error": "png_base64 decoded to empty" }),
        Err(e) => json!({ "ok": false, "error": format!("invalid base64: {e}") }),
    }
}

// ── Stage 2 — HTTP routes ──────────────────────────────────────

/// Handle an HTTP request the daemon proxied to the plugin under
/// `/telegram/*`.
///
/// Routes today:
/// - `GET /telegram/health` — plain-text health probe.
/// - `GET /telegram/status` — JSON status snapshot.
/// - anything else → 404.
///
/// Plugin owns its internal router; daemon doesn't need to know
/// the route table.
pub fn http_request(request: &Value) -> Value {
    let path = request.get("path").and_then(|v| v.as_str()).unwrap_or("/");
    let method = request.get("method").and_then(|v| v.as_str()).unwrap_or("GET");
    match (method, path) {
        ("GET", "/telegram/health") => respond(
            200,
            "text/plain; charset=utf-8",
            b"telegram plugin ok\n",
        ),
        ("GET", "/telegram/status") => respond(
            200,
            "application/json; charset=utf-8",
            br#"{"status":"ok","plugin":"telegram"}"#,
        ),
        _ => respond(
            404,
            "application/json; charset=utf-8",
            br#"{"error":"not found"}"#,
        ),
    }
}

fn respond(status: u16, content_type: &str, body: &[u8]) -> Value {
    json!({
        "status": status,
        "headers": [["Content-Type", content_type]],
        "body_base64": base64::engine::general_purpose::STANDARD.encode(body),
    })
}

// ── Stage 4 — admin RPC ────────────────────────────────────────

/// Handle a daemon-forwarded admin RPC method. The daemon's
/// `PluginAdminRouter` maps `nexo/admin/telegram/<verb>` to
/// `plugin.telegram.admin.<verb>` (slash → dot translation).
///
/// Methods today (mirror the legacy hardcoded daemon paths
/// `nexo/admin/whatsapp/bot/*` for telegram analogs):
/// - `nexo/admin/telegram/bot_info` — return bot metadata.
/// - `nexo/admin/telegram/list_instances` — declared instances.
pub fn admin_handle(request: &Value) -> Value {
    let method = request
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let _params = request.get("params").cloned().unwrap_or(Value::Null);
    match method {
        "nexo/admin/telegram/bot_info" => json!({
            "ok": true,
            "result": { "plugin": "telegram", "version": env!("CARGO_PKG_VERSION") },
        }),
        "nexo/admin/telegram/list_instances" => json!({
            "ok": true,
            "result": { "instances": [] },
        }),
        other => json!({
            "ok": false,
            "error": format!("unknown admin method: {other}"),
        }),
    }
}

// ── Stage 5 — metrics scrape ───────────────────────────────────

/// Emit Prometheus text for the daemon's `/metrics` aggregator
/// to concatenate. Metric names MUST be prefixed with `telegram_`
/// to avoid collisions with daemon-internal series.
pub fn metrics_scrape(_request: &Value) -> Value {
    let text = "\
# HELP telegram_plugin_ready Whether the telegram plugin is up.\n\
# TYPE telegram_plugin_ready gauge\n\
telegram_plugin_ready 1\n\
# HELP telegram_plugin_version_info Plugin version label.\n\
# TYPE telegram_plugin_version_info gauge\n\
telegram_plugin_version_info{version=\"0.3.0\"} 1\n";
    json!({ "text": text })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_normalize_lowercases_handle() {
        let r = pairing_normalize_sender(&json!({ "raw": "@User_Name" }));
        assert_eq!(r["normalized"].as_str(), Some("@user_name"));
    }

    #[test]
    fn pairing_normalize_passes_numeric_through() {
        let r = pairing_normalize_sender(&json!({ "raw": "123456789" }));
        assert_eq!(r["normalized"].as_str(), Some("123456789"));
    }

    #[test]
    fn pairing_normalize_rejects_short_handle() {
        let r = pairing_normalize_sender(&json!({ "raw": "@abcd" }));
        assert!(r["normalized"].is_null());
    }

    #[test]
    fn pairing_normalize_rejects_non_alphanum() {
        let r = pairing_normalize_sender(&json!({ "raw": "@user-name" }));
        assert!(r["normalized"].is_null());
    }

    #[test]
    fn pairing_send_reply_acks_valid() {
        let r = pairing_send_reply(&json!({
            "account": "default",
            "to": "@user",
            "text": "code: 9999",
        }));
        assert_eq!(r["ok"].as_bool(), Some(true));
    }

    #[test]
    fn pairing_send_reply_rejects_missing_to() {
        let r = pairing_send_reply(&json!({ "account": "default", "to": "", "text": "x" }));
        assert_eq!(r["ok"].as_bool(), Some(false));
    }

    #[test]
    fn http_get_health_serves_200() {
        let r = http_request(&json!({ "method": "GET", "path": "/telegram/health" }));
        assert_eq!(r["status"].as_u64(), Some(200));
    }

    #[test]
    fn http_unknown_returns_404() {
        let r = http_request(&json!({ "method": "GET", "path": "/telegram/missing" }));
        assert_eq!(r["status"].as_u64(), Some(404));
    }

    #[test]
    fn admin_bot_info_returns_plugin_metadata() {
        let r = admin_handle(&json!({
            "method": "nexo/admin/telegram/bot_info",
            "params": {},
        }));
        assert_eq!(r["ok"].as_bool(), Some(true));
        assert_eq!(r["result"]["plugin"].as_str(), Some("telegram"));
    }

    #[test]
    fn admin_unknown_method_returns_err() {
        let r = admin_handle(&json!({
            "method": "nexo/admin/telegram/nonexistent",
            "params": {},
        }));
        assert_eq!(r["ok"].as_bool(), Some(false));
    }

    #[test]
    fn metrics_scrape_returns_telegram_namespaced_metrics() {
        let r = metrics_scrape(&json!({}));
        let text = r["text"].as_str().expect("text");
        assert!(text.contains("telegram_plugin_ready 1"));
        assert!(text.contains("telegram_plugin_version_info"));
    }
}
