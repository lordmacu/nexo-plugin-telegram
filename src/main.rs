//! Subprocess entrypoint for `nexo-plugin-telegram`.
//!
//! Wires:
//!   - [`PluginAdapter`] — child-side JSON-RPC dispatch loop.
//!   - [`telegram_tool_defs`] — 6 `telegram_*` tool defs advertised
//!     in the initialize reply.
//!   - [`dispatch_telegram_tool`] — per-tool routing for the
//!     resolved [`TelegramPlugin`].
//!   - one [`TelegramPlugin`] per process, lazy-booted on the first
//!     `tool.invoke` from the daemon-supplied env vars.
//!
//! Configuration flows from the daemon via env vars set by
//! `proyecto/src/main.rs::seed_telegram_subprocess_env`:
//!   * `NEXO_PLUGIN_TELEGRAM_TOKEN`
//!   * `NEXO_PLUGIN_TELEGRAM_INSTANCE`        (optional)
//!   * `NEXO_PLUGIN_TELEGRAM_ALLOWLIST`       (JSON array, optional)
//!   * `NEXO_PLUGIN_TELEGRAM_INTERVAL_MS`     (default 1500)
//!   * `NEXO_PLUGIN_TELEGRAM_BRIDGE_TIMEOUT_MS` (default 30000)
//!   * `NEXO_PLUGIN_TELEGRAM_OFFSET_PATH`
//!   * `NEXO_PLUGIN_TELEGRAM_AUTO_TRANSCRIBE` (default false)
//!   * `NEXO_PLUGIN_TELEGRAM_WHISPER_*`       (optional)
//!   * `NEXO_BROKER_KIND`  (`nats` or `stdio_bridge`;
//!     defaults to `nats` for backwards compat)
//!   * `NEXO_BROKER_URL`   (required when KIND=nats)

use std::sync::Arc;

use nexo_broker::{AnyBroker, StdioBridgeBroker};
use nexo_core::agent::plugin::Plugin;
use nexo_microapp_sdk::plugin::{PluginAdapter, ToolInvocation, ToolInvocationError};
use nexo_plugin_telegram::{
    dispatch_telegram_tool, telegram_config_from_env, telegram_tool_defs, TelegramPlugin,
};
use once_cell::sync::Lazy;
use tokio::sync::OnceCell;

const MANIFEST: &str = include_str!("../nexo-plugin.toml");

/// Process-wide [`TelegramPlugin`]. Boot is gated behind the first
/// `tool.invoke` so the JSON-RPC `initialize` handshake can complete
/// even when the broker is unreachable at startup. Daemon supervisor
/// retries broker / Telegram outages on its own cadence.
static PLUGIN: Lazy<OnceCell<Arc<TelegramPlugin>>> = Lazy::new(OnceCell::new);

/// Populated in `main()` when the daemon stamps
/// `NEXO_BROKER_KIND=stdio_bridge`. Mirrors the BRIDGE cell in
/// the whatsapp plugin; same role, same wiring.
static BRIDGE: Lazy<OnceCell<Arc<StdioBridgeBroker>>> = Lazy::new(OnceCell::new);

/// Construct the broker based on `NEXO_BROKER_KIND`.
/// `stdio_bridge` clones from [`BRIDGE`]; anything else (default
/// + explicit `nats`) connects via `NEXO_BROKER_URL`.
async fn build_broker() -> Result<AnyBroker, ToolInvocationError> {
    let kind = std::env::var("NEXO_BROKER_KIND").unwrap_or_else(|_| "nats".to_string());
    if kind == "stdio_bridge" {
        let bridge = BRIDGE.get().ok_or_else(|| {
            ToolInvocationError::Unavailable(
                "stdio_bridge mode: BRIDGE not initialized — main() must call \
                 PluginAdapter::with_stdio_bridge_broker before tool.invoke"
                    .into(),
            )
        })?;
        return Ok(AnyBroker::stdio_bridge((**bridge).clone()));
    }
    let broker_url = std::env::var("NEXO_BROKER_URL").map_err(|_| {
        ToolInvocationError::Unavailable(
            "NEXO_BROKER_URL not set — daemon must seed it before tool.invoke".into(),
        )
    })?;
    let broker_inner = nexo_config::types::broker::BrokerInner {
        kind: if broker_url.starts_with("nats://") {
            nexo_config::types::broker::BrokerKind::Nats
        } else {
            nexo_config::types::broker::BrokerKind::Local
        },
        url: broker_url,
        auth: nexo_config::types::broker::BrokerAuthConfig::default(),
        persistence: nexo_config::types::broker::BrokerPersistenceConfig::default(),
        limits: nexo_config::types::broker::BrokerLimitsConfig::default(),
        fallback: nexo_config::types::broker::BrokerFallbackConfig::default(),
    };
    AnyBroker::from_config(&broker_inner)
        .await
        .map_err(|e| ToolInvocationError::Unavailable(format!("broker connect failed: {e}")))
}

async fn shared_plugin() -> Result<Arc<TelegramPlugin>, ToolInvocationError> {
    PLUGIN
        .get_or_try_init(|| async {
            // Phase 93.4.a — prefer the `plugin.configure`-delivered
            // slice (Phase 93.2) when present; legacy env-var path
            // stays as fallback during the 0.2.x deprecation window.
            let cfg = {
                let guard = nexo_plugin_telegram::configured_state().read().await;
                if let Some(vec) = guard.as_ref() {
                    vec.first().cloned().ok_or_else(|| {
                        ToolInvocationError::ArgumentInvalid(
                            "configured array delivered empty Vec".into(),
                        )
                    })?
                } else {
                    drop(guard);
                    telegram_config_from_env().map_err(|e| {
                        ToolInvocationError::ArgumentInvalid(format!("env config: {e}"))
                    })?
                }
            };

            let broker = build_broker().await?;

            let plugin = Arc::new(TelegramPlugin::new(cfg));

            // `start` performs `getMe` against the Telegram Bot API
            // (validates token + caches bot username), subscribes to
            // the outbound topic, and spawns the long-poll task. A
            // 401 / network outage here surfaces as Unavailable so
            // the daemon supervisor handles retry. Subsequent
            // `tool.invoke` calls find the cached `bot_handle`.
            plugin.start(broker).await.map_err(|e| {
                ToolInvocationError::Unavailable(format!("telegram plugin start failed: {e}"))
            })?;

            tracing::info!(
                target = "nexo_plugin_telegram",
                instance = plugin.config().instance.as_deref().unwrap_or(""),
                "telegram subprocess plugin ready"
            );
            Ok::<_, ToolInvocationError>(plugin)
        })
        .await
        .cloned()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    // rustls 0.23 requires an explicit process-wide CryptoProvider
    // before `ClientConfig::builder()` can return successfully.
    // Same dance as the proyecto daemon (see proyecto/src/main.rs).
    let _ = rustls::crypto::ring::default_provider().install_default();

    let adapter = PluginAdapter::new(MANIFEST)?
        .declare_tools(telegram_tool_defs())
        // Phase 93.4.a — receive the operator YAML slice via the
        // host's `plugin.configure` JSON-RPC (Phase 93.2). Sequence
        // shape per manifest `[plugin.config_schema] shape = "array"`.
        .on_configure(|value: serde_yaml::Value| async move {
            let parsed: Vec<nexo_plugin_telegram::TelegramPluginConfig> =
                serde_yaml::from_value(value)
                    .map_err(|e| format!("invalid telegram config: {e}"))?;
            *nexo_plugin_telegram::configured_state().write().await = Some(parsed);
            Ok(())
        })
        // Phase 93.8.a-telegram — register the 4 credential-store
        // handlers. Daemon-side `RemoteCredentialStore` (Phase
        // 93.8.a-daemon) round-trips each
        // `GenericCredentialStore` method via these RPCs.
        .on_credentials_list(|| async move {
            let guard = nexo_plugin_telegram::configured_state().read().await;
            let accounts: Vec<String> = guard
                .as_ref()
                .map(|v| {
                    v.iter()
                        .filter_map(|c| c.instance.clone())
                        .collect()
                })
                .unwrap_or_default();
            Ok(nexo_microapp_sdk::plugin::CredentialsListReply {
                accounts,
                warnings: Vec::new(),
            })
        })
        .on_credentials_issue(|account_id: String, agent_id: String| async move {
            // Allow-list check against the configured account's
            // `allow_agents`. Empty allow_agents ⇒ accept any.
            let guard = nexo_plugin_telegram::configured_state().read().await;
            let Some(cfgs) = guard.as_ref() else {
                return Err("not_found".to_string());
            };
            let cfg = cfgs
                .iter()
                .find(|c| c.instance.as_deref() == Some(account_id.as_str()));
            match cfg {
                None => Err("not_found".to_string()),
                Some(c) if c.allow_agents.is_empty() || c.allow_agents.contains(&agent_id) => {
                    Ok(())
                }
                Some(_) => Err("not_permitted".to_string()),
            }
        })
        .on_credentials_resolve_bytes(|account_id: String, _agent_id: String, _fingerprint: String| async move {
            // Return the full account config as JSON bytes. The
            // daemon's caller (Phase 93.8.b+ consumer migration)
            // deserialises with
            // `serde_json::from_slice::<TelegramPluginConfig>(&bytes)`.
            // Bytes carry the bot token — only flow to authorised
            // outbound paths (the existing resolver + breaker
            // chain enforces that).
            let guard = nexo_plugin_telegram::configured_state().read().await;
            let Some(cfgs) = guard.as_ref() else {
                return Err("not_found".to_string());
            };
            let cfg = cfgs
                .iter()
                .find(|c| c.instance.as_deref() == Some(account_id.as_str()))
                .ok_or_else(|| "not_found".to_string())?;
            serde_json::to_vec(cfg).map_err(|e| format!("serialize failed: {e}"))
        })
        .on_credentials_reload(|| async move {
            // Telegram has no live-reload path — the operator's
            // YAML re-delivers via `plugin.configure` on file-
            // watcher fire (Phase 93.2 hot-reload). This handler
            // is a no-op + ack.
            Ok(())
        })
        .on_tool(|invocation: ToolInvocation| async move {
            let plugin = shared_plugin().await?;
            dispatch_telegram_tool(plugin.as_ref(), invocation).await
        });

    // See whatsapp plugin's main.rs for the matching wiring
    // rationale. The bridge piggybacks on the adapter's
    // stdout writer; net: zero network broker dependency when
    // the daemon runs with `broker.yaml type: local`.
    let adapter = if std::env::var("NEXO_BROKER_KIND").as_deref() == Ok("stdio_bridge") {
        let (adapter, bridge) = adapter.with_stdio_bridge_broker();
        BRIDGE
            .set(bridge)
            .map_err(|_| anyhow::anyhow!("BRIDGE already initialized (this should not happen)"))?;
        tracing::info!(
            target = "nexo_plugin_telegram",
            "stdio_bridge broker wired (daemon broker = Local)"
        );
        adapter
    } else {
        adapter
    };

    adapter.run_stdio().await?;
    Ok(())
}
