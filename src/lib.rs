//! `nexo-plugin-telegram` — Telegram Bot channel plugin.
//!
//! Uses the HTTP Bot API (long polling). Complements the WhatsApp
//! plugin for multi-channel deployments: one agent, two surfaces,
//! same session semantics via `UUIDv5(chat_id)`.
//!
//! ## Two consumers
//!
//! 1. **Subprocess (default)** — `src/main.rs` wraps
//!    [`TelegramPlugin`] in
//!    [`nexo_microapp_sdk::plugin::PluginAdapter`] and runs the
//!    JSON-RPC dispatch loop over stdio. The daemon spawns this
//!    binary and seeds it with env vars (token, instance, allowlist,
//!    offset path, …); per-instance state is fully owned by the
//!    subprocess, no cross-process coordination.
//!
//! 2. **Embedded (Phase 90)** — a host process (Android Flutter
//!    bridge, future single-process operator UI) imports the lib
//!    directly and instantiates [`TelegramPlugin`] in-process via
//!    [`telegram_plugin_factory`]. The `embedded` cargo feature
//!    drops subprocess code paths so the resulting binary stays
//!    lean.

pub mod bot;
pub mod env_config;
pub mod events;
pub mod pairing_adapter;
pub mod plugin;
pub mod session_id;
#[cfg(not(feature = "embedded"))]
pub mod subprocess_dispatch;
pub mod tool;

pub use env_config::telegram_config_from_env;
#[cfg(not(feature = "embedded"))]
pub use subprocess_dispatch::{dispatch_telegram_tool, telegram_tool_defs};

pub use events::InboundEvent;
pub use pairing_adapter::TelegramPairingAdapter;
pub use plugin::{TelegramPlugin, TOPIC_INBOUND, TOPIC_OUTBOUND};
pub use session_id::session_id_for_chat;
pub use tool::register_telegram_tools;

use std::sync::Arc;

use nexo_config::types::plugins::TelegramPluginConfig;
use nexo_core::agent::nexo_plugin_registry::PluginFactory;
use nexo_core::agent::plugin_host::NexoPlugin;

/// Factory builder for one telegram plugin instance, used by the
/// in-process embedded path. Multi-bot operators call this once per
/// [`TelegramPluginConfig`] (one per bot token / instance label) and
/// register each result in a `PluginFactoryRegistry` under a
/// distinct manifest name; `wire_plugin_registry(..., Some(&factory))`
/// instantiates them on boot.
///
/// Subprocess consumers (the daemon's default path) construct
/// [`TelegramPlugin`] directly inside `main.rs` from env-derived
/// config and never touch this helper.
pub fn telegram_plugin_factory(cfg: TelegramPluginConfig) -> PluginFactory {
    Box::new(move |_manifest| {
        let plugin: Arc<dyn NexoPlugin> = Arc::new(TelegramPlugin::new(cfg.clone()));
        Ok(plugin)
    })
}
