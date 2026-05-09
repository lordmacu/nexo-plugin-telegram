//! Subprocess entrypoint for `nexo-plugin-telegram` (Phase 81.18).
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
//!   * `NEXO_BROKER_URL`

use std::sync::Arc;

use nexo_broker::AnyBroker;
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

async fn shared_plugin() -> Result<Arc<TelegramPlugin>, ToolInvocationError> {
    PLUGIN
        .get_or_try_init(|| async {
            let cfg = telegram_config_from_env()
                .map_err(|e| ToolInvocationError::ArgumentInvalid(format!("env config: {e}")))?;

            let broker_url = std::env::var("NEXO_BROKER_URL").map_err(|_| {
                ToolInvocationError::Unavailable(
                    "NEXO_BROKER_URL not set — daemon must seed it before tool.invoke".into(),
                )
            })?;

            // Build a `BrokerInner` from the seeded URL. Auth /
            // persistence / limits / fallback all default — the
            // daemon already chose those for the parent process and
            // the subprocess just needs the connection URL to reach
            // the same NATS server.
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

            let broker = AnyBroker::from_config(&broker_inner).await.map_err(|e| {
                ToolInvocationError::Unavailable(format!("broker connect failed: {e}"))
            })?;

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
        .on_tool(|invocation: ToolInvocation| async move {
            let plugin = shared_plugin().await?;
            dispatch_telegram_tool(plugin.as_ref(), invocation).await
        });

    adapter.run_stdio().await?;
    Ok(())
}
