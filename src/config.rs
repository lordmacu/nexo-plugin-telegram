//! Phase 93.4.a — plugin-owned config types.
//!
//! Until 0.1.4 this plugin re-exported `nexo_config::types::plugins::*`
//! to keep the daemon-side typed struct as the single source of
//! truth. Phase 93 inverts that: each plugin owns its config
//! contract (manifest's `[plugin.config_schema]` + this module's
//! Rust definitions), and the daemon delivers the operator YAML
//! opaquely via `plugin.configure` JSON-RPC. Dropping the
//! `nexo-config` dep cuts the framework→plugin coupling Phase 93
//! targets.
//!
//! Field shapes mirror the historical `nexo_config::types::plugins`
//! definitions verbatim — operators keep the same YAML, plugin
//! authors don't need a `nexo-config` Cargo dep.

use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct TelegramPluginConfig {
    pub token: String,
    #[serde(default)]
    pub polling: TelegramPollingConfig,
    #[serde(default)]
    pub allowlist: TelegramAllowlistConfig,
    #[serde(default)]
    pub auto_transcribe: TelegramAutoTranscribeConfig,
    /// How long the bridge waits for the agent's reply before firing
    /// a BridgeTimeout event. Agents with long tool chains (multi-step
    /// LLM + external APIs) can breach the old 30s default — bump this
    /// to cover the slowest realistic turn.
    #[serde(default = "default_bridge_timeout_ms")]
    pub bridge_timeout_ms: u64,
    /// Optional instance label for multi-bot routing. When set, events
    /// publish to `plugin.inbound.telegram.<instance>` instead of the
    /// default `plugin.inbound.telegram`. Empty / absent = legacy
    /// single-bot topic.
    #[serde(default)]
    pub instance: Option<String>,
    /// Agents permitted to publish from this bot. Empty = accept any
    /// agent holding a valid resolver handle (back-compat).
    #[serde(default)]
    pub allow_agents: Vec<String>,
}

fn default_bridge_timeout_ms() -> u64 {
    120_000
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct TelegramAutoTranscribeConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Path to the whisper extension binary (stdio JSON-RPC). Default is
    /// `./extensions/openai-whisper/target/release/openai-whisper`.
    #[serde(default = "default_whisper_command")]
    pub command: String,
    /// Hard cap on how long to wait for a transcription before giving up
    /// and publishing the message without text.
    #[serde(default = "default_whisper_timeout")]
    pub timeout_ms: u64,
    /// Forwarded verbatim to the whisper tool call (`language`, `prompt`).
    #[serde(default)]
    pub language: Option<String>,
}

fn default_whisper_command() -> String {
    "./extensions/openai-whisper/target/release/openai-whisper".to_string()
}
fn default_whisper_timeout() -> u64 {
    60_000
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct TelegramPollingConfig {
    #[serde(default = "default_polling_enabled")]
    pub enabled: bool,
    #[serde(default = "default_polling_interval")]
    pub interval_ms: u64,
    /// Path where the poller persists its `offset` between restarts so
    /// a restart doesn't replay the last 24h of updates.
    #[serde(default)]
    pub offset_path: Option<String>,
}

fn default_polling_enabled() -> bool {
    true
}
fn default_polling_interval() -> u64 {
    25_000
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct TelegramAllowlistConfig {
    #[serde(default)]
    pub chat_ids: Vec<i64>,
}
