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
//! 2. **Embedded** — a host process (Android Flutter
//!    bridge, future single-process operator UI) imports the lib
//!    directly and instantiates [`TelegramPlugin`] in-process via
//!    [`telegram_plugin_factory`]. The `embedded` cargo feature
//!    drops subprocess code paths so the resulting binary stays
//!    lean.

pub mod bot;
pub mod config;
pub mod configured_state;
pub mod env_config;
pub mod events;
pub mod pairing_adapter;
pub mod plugin;
pub mod session_id;
#[cfg(not(feature = "embedded"))]
pub mod subprocess_dispatch;
pub mod tool;

pub use config::{
    TelegramAllowlistConfig, TelegramAutoTranscribeConfig, TelegramPluginConfig,
    TelegramPollingConfig,
};
pub use configured_state::configured_state;
pub use env_config::telegram_config_from_env;
#[cfg(not(feature = "embedded"))]
pub use subprocess_dispatch::{dispatch_telegram_tool, telegram_tool_defs};

pub use events::InboundEvent;
pub use pairing_adapter::TelegramPairingAdapter;
pub use plugin::{TelegramPlugin, TOPIC_INBOUND, TOPIC_OUTBOUND};
pub use session_id::session_id_for_chat;
pub use tool::register_telegram_tools;
