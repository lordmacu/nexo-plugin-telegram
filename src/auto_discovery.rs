//! Phase 81.33.b.real Stages 1+2+4+5 — auto-discovery broker
//! handlers (v0.4 — full real wiring, no ack-only stubs).
//!
//! Functions that take a JSON request payload + an optional
//! broker handle and return a JSON response payload. Wired in
//! `src/main.rs` via the broker subscription loop: incoming
//! `broker.event` → `serde_json::from_value::<Message>(payload)`
//! → invoke handler → publish reply to `msg.reply_to`.
//!
//! Contract docs in the daemon repo:
//! - Pairing adapter — `proyecto/docs/src/plugins/manifest-pairing-adapter.md`
//! - HTTP routes     — `proyecto/docs/src/plugins/manifest-http.md`
//! - Admin RPC       — `proyecto/docs/src/plugins/manifest-admin.md`
//! - Metrics scrape  — `proyecto/docs/src/plugins/manifest-metrics.md`

use base64::Engine;
use nexo_broker::{AnyBroker, BrokerHandle, Event};
use serde_json::{json, Value};

use crate::configured_state;

// ── Stage 1 — pairing adapter ──────────────────────────────────

/// Canonicalise an inbound Telegram sender id.
///
/// Numeric chat_ids pass through; `@handle` form is lower-cased
/// and validated against Telegram's username rules (5-32 chars,
/// ASCII alnum + underscore). Mirrors
/// `TelegramPairingAdapter::normalize_sender`.
///
/// Pure — no broker round-trip.
///
/// Request: `{ "raw": "<raw-sender>" }`
/// Reply:   `{ "normalized": "<canonical>" }` or
///          `{ "normalized": null }` to reject.
pub fn pairing_normalize_sender(request: &Value) -> Value {
    let raw = request.get("raw").and_then(|v| v.as_str()).unwrap_or("");
    if raw.is_empty() {
        return json!({ "normalized": null });
    }
    if raw.parse::<i64>().is_ok() {
        return json!({ "normalized": raw });
    }
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

fn outbound_topic(account: &str) -> String {
    if account.is_empty() {
        "plugin.outbound.telegram".to_string()
    } else {
        format!("plugin.outbound.telegram.{account}")
    }
}

/// Deliver a plain-text reply to a Telegram sender by publishing
/// to the plugin's outbound topic. The outbound dispatcher
/// (`plugin.rs::handle_outbound`) consumes the event + issues the
/// `sendMessage` Bot API call.
///
/// Reuses the same payload shape `TelegramPairingAdapter::send_reply`
/// emits, so legacy lib-linked + broker-dispatch paths converge on
/// one outbound flow.
///
/// Request: `{ "account": "<instance>", "to": "<chat_id>", "text": "<msg>" }`
/// Reply:   `{ "ok": true }` on publish; `{ "ok": false, "error": "..." }` otherwise.
pub async fn pairing_send_reply(broker: &AnyBroker, request: &Value) -> Value {
    let account = request
        .get("account")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let to = request.get("to").and_then(|v| v.as_str()).unwrap_or("");
    let text = request.get("text").and_then(|v| v.as_str()).unwrap_or("");
    if to.is_empty() || text.is_empty() {
        return json!({ "ok": false, "error": "to and text required" });
    }
    let topic = outbound_topic(account);
    let payload = json!({
        "kind": "text",
        "to": to,
        "text": text,
        "parse_mode": "MarkdownV2",
    });
    let evt = Event::new(&topic, "core.pairing", payload);
    match broker.publish(&topic, evt).await {
        Ok(()) => json!({ "ok": true }),
        Err(e) => json!({ "ok": false, "error": format!("publish failed: {e}") }),
    }
}

/// Send a QR PNG to the Telegram sender. Validates base64,
/// publishes a `kind="photo"` event on the outbound topic; the
/// outbound dispatcher handles the `sendPhoto` Bot API call.
///
/// Request: `{ "account": "<instance>", "to": "<chat_id>",
///            "png_base64": "<base64>", "caption": "<optional>" }`
/// Reply:   `{ "ok": true }` / `{ "ok": false, "error": "..." }`.
pub async fn pairing_send_qr_image(broker: &AnyBroker, request: &Value) -> Value {
    let account = request
        .get("account")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let to = request.get("to").and_then(|v| v.as_str()).unwrap_or("");
    let png_b64 = request
        .get("png_base64")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let caption = request
        .get("caption")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if to.is_empty() || png_b64.is_empty() {
        return json!({ "ok": false, "error": "to and png_base64 required" });
    }
    if base64::engine::general_purpose::STANDARD
        .decode(png_b64.as_bytes())
        .map(|b| b.is_empty())
        .unwrap_or(true)
    {
        return json!({ "ok": false, "error": "invalid or empty base64" });
    }
    let topic = outbound_topic(account);
    let payload = json!({
        "kind": "photo",
        "to": to,
        "png_base64": png_b64,
        "caption": caption,
    });
    let evt = Event::new(&topic, "core.pairing", payload);
    match broker.publish(&topic, evt).await {
        Ok(()) => json!({ "ok": true }),
        Err(e) => json!({ "ok": false, "error": format!("publish failed: {e}") }),
    }
}

// ── Stage 2 — HTTP routes ──────────────────────────────────────

/// Handle an HTTP request the daemon proxied under `/telegram/*`.
///
/// Routes:
/// - `GET /telegram/health` — plain-text health probe.
/// - `GET /telegram/status` — JSON snapshot of plugin + configured instances.
/// - anything else → 404.
pub async fn http_request(request: &Value) -> Value {
    let path = request.get("path").and_then(|v| v.as_str()).unwrap_or("/");
    let method = request
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("GET");
    match (method, path) {
        ("GET", "/telegram/health") => respond(
            200,
            "text/plain; charset=utf-8",
            b"telegram plugin ok\n",
        ),
        ("GET", "/telegram/status") => {
            let instances = configured_instances().await;
            let body = json!({
                "status": "ok",
                "plugin": "telegram",
                "version": env!("CARGO_PKG_VERSION"),
                "configured_instances": instances,
            });
            respond(
                200,
                "application/json; charset=utf-8",
                body.to_string().as_bytes(),
            )
        }
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

/// Handle a daemon-forwarded admin RPC.
///
/// Methods:
/// - `nexo/admin/telegram/bot_info` — plugin metadata + configured instance count.
/// - `nexo/admin/telegram/list_instances` — declared instance ids.
pub async fn admin_handle(request: &Value) -> Value {
    let method = request
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    match method {
        "nexo/admin/telegram/bot_info" => {
            let instances = configured_instances().await;
            json!({
                "ok": true,
                "result": {
                    "plugin": "telegram",
                    "version": env!("CARGO_PKG_VERSION"),
                    "configured_instances": instances,
                },
            })
        }
        "nexo/admin/telegram/list_instances" => {
            let instances = configured_instances().await;
            json!({ "ok": true, "result": { "instances": instances } })
        }
        other => json!({
            "ok": false,
            "error": format!("unknown admin method: {other}"),
        }),
    }
}

// ── Stage 5 — metrics scrape ───────────────────────────────────

/// Emit Prometheus text for the daemon's `/metrics` aggregator.
/// Series names prefixed with `telegram_` to avoid collisions.
pub async fn metrics_scrape(_request: &Value) -> Value {
    let instance_count = configured_instances().await.len();
    let version = env!("CARGO_PKG_VERSION");
    let text = format!(
        "# HELP telegram_plugin_ready Whether the telegram plugin is up.\n\
         # TYPE telegram_plugin_ready gauge\n\
         telegram_plugin_ready 1\n\
         # HELP telegram_plugin_version_info Plugin version label.\n\
         # TYPE telegram_plugin_version_info gauge\n\
         telegram_plugin_version_info{{version=\"{version}\"}} 1\n\
         # HELP telegram_plugin_instances_configured Configured instance count.\n\
         # TYPE telegram_plugin_instances_configured gauge\n\
         telegram_plugin_instances_configured {instance_count}\n",
    );
    json!({ "text": text })
}

// ── helpers ────────────────────────────────────────────────────

async fn configured_instances() -> Vec<String> {
    let guard = configured_state().read().await;
    guard
        .as_ref()
        .map(|cfgs| {
            cfgs.iter()
                .filter_map(|c| c.instance.clone())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexo_broker::{AnyBroker, BrokerHandle};

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
    fn pairing_normalize_passes_negative_chat_id() {
        let r = pairing_normalize_sender(&json!({ "raw": "-1001234567890" }));
        assert_eq!(r["normalized"].as_str(), Some("-1001234567890"));
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
    fn pairing_normalize_rejects_empty() {
        let r = pairing_normalize_sender(&json!({ "raw": "" }));
        assert!(r["normalized"].is_null());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pairing_send_reply_publishes_to_outbound_topic_when_account_empty() {
        let broker = AnyBroker::local();
        let mut sub = broker.subscribe("plugin.outbound.telegram").await.unwrap();
        let r = pairing_send_reply(
            &broker,
            &json!({ "account": "", "to": "123", "text": "hello" }),
        )
        .await;
        assert_eq!(r["ok"].as_bool(), Some(true));
        let evt = sub.next().await.expect("event published");
        assert_eq!(evt.payload["kind"].as_str(), Some("text"));
        assert_eq!(evt.payload["to"].as_str(), Some("123"));
        assert_eq!(evt.payload["text"].as_str(), Some("hello"));
        assert_eq!(evt.payload["parse_mode"].as_str(), Some("MarkdownV2"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pairing_send_reply_uses_per_instance_topic_when_account_set() {
        let broker = AnyBroker::local();
        let mut sub = broker
            .subscribe("plugin.outbound.telegram.sales")
            .await
            .unwrap();
        let r = pairing_send_reply(
            &broker,
            &json!({ "account": "sales", "to": "@user", "text": "code: 9999" }),
        )
        .await;
        assert_eq!(r["ok"].as_bool(), Some(true));
        let evt = sub.next().await.expect("event published");
        assert_eq!(evt.payload["to"].as_str(), Some("@user"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pairing_send_reply_rejects_missing_fields() {
        let broker = AnyBroker::local();
        let r = pairing_send_reply(
            &broker,
            &json!({ "account": "default", "to": "", "text": "x" }),
        )
        .await;
        assert_eq!(r["ok"].as_bool(), Some(false));
        let r = pairing_send_reply(
            &broker,
            &json!({ "account": "default", "to": "123", "text": "" }),
        )
        .await;
        assert_eq!(r["ok"].as_bool(), Some(false));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pairing_send_qr_image_publishes_photo_event() {
        let broker = AnyBroker::local();
        let mut sub = broker.subscribe("plugin.outbound.telegram").await.unwrap();
        let png_b64 = base64::engine::general_purpose::STANDARD.encode(b"\x89PNG\r\n\x1a\n");
        let r = pairing_send_qr_image(
            &broker,
            &json!({
                "account": "",
                "to": "123",
                "png_base64": png_b64,
                "caption": "pair me",
            }),
        )
        .await;
        assert_eq!(r["ok"].as_bool(), Some(true));
        let evt = sub.next().await.expect("event published");
        assert_eq!(evt.payload["kind"].as_str(), Some("photo"));
        assert_eq!(evt.payload["caption"].as_str(), Some("pair me"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pairing_send_qr_image_rejects_invalid_base64() {
        let broker = AnyBroker::local();
        let r = pairing_send_qr_image(
            &broker,
            &json!({ "account": "", "to": "123", "png_base64": "!!not-base64!!" }),
        )
        .await;
        assert_eq!(r["ok"].as_bool(), Some(false));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pairing_send_qr_image_rejects_empty_base64() {
        let broker = AnyBroker::local();
        let r = pairing_send_qr_image(
            &broker,
            &json!({ "account": "", "to": "123", "png_base64": "" }),
        )
        .await;
        assert_eq!(r["ok"].as_bool(), Some(false));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn http_get_health_serves_200() {
        let r = http_request(&json!({ "method": "GET", "path": "/telegram/health" })).await;
        assert_eq!(r["status"].as_u64(), Some(200));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn http_get_status_returns_plugin_metadata() {
        let r = http_request(&json!({ "method": "GET", "path": "/telegram/status" })).await;
        assert_eq!(r["status"].as_u64(), Some(200));
        let body_b64 = r["body_base64"].as_str().unwrap();
        let body = base64::engine::general_purpose::STANDARD
            .decode(body_b64)
            .unwrap();
        let body_str = String::from_utf8(body).unwrap();
        assert!(body_str.contains("\"plugin\":\"telegram\""));
        assert!(body_str.contains("\"version\""));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn http_unknown_returns_404() {
        let r = http_request(&json!({ "method": "GET", "path": "/telegram/missing" })).await;
        assert_eq!(r["status"].as_u64(), Some(404));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn admin_bot_info_returns_plugin_metadata() {
        let r = admin_handle(&json!({
            "method": "nexo/admin/telegram/bot_info",
            "params": {},
        }))
        .await;
        assert_eq!(r["ok"].as_bool(), Some(true));
        assert_eq!(r["result"]["plugin"].as_str(), Some("telegram"));
        assert!(r["result"]["version"].is_string());
        assert!(r["result"]["configured_instances"].is_array());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn admin_list_instances_returns_array() {
        let r = admin_handle(&json!({
            "method": "nexo/admin/telegram/list_instances",
            "params": {},
        }))
        .await;
        assert_eq!(r["ok"].as_bool(), Some(true));
        assert!(r["result"]["instances"].is_array());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn admin_unknown_method_returns_err() {
        let r = admin_handle(&json!({
            "method": "nexo/admin/telegram/nonexistent",
            "params": {},
        }))
        .await;
        assert_eq!(r["ok"].as_bool(), Some(false));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn metrics_scrape_returns_telegram_namespaced_metrics() {
        let r = metrics_scrape(&json!({})).await;
        let text = r["text"].as_str().expect("text");
        assert!(text.contains("telegram_plugin_ready 1"));
        assert!(text.contains("telegram_plugin_version_info"));
        assert!(text.contains("telegram_plugin_instances_configured"));
    }
}
