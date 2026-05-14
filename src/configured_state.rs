//! Phase 93.4.a — operator config slice delivered via
//! `plugin.configure` JSON-RPC (Phase 93.2). The handler in
//! `main.rs::main` records the deserialised `Vec<TelegramPluginConfig>`
//! here; `shared_plugin()` reads it before falling back to the
//! legacy env-var path during the deprecation window.

use std::sync::Arc;
use std::sync::OnceLock;
use tokio::sync::RwLock;

use crate::config::TelegramPluginConfig;

static CONFIGURED: OnceLock<Arc<RwLock<Option<Vec<TelegramPluginConfig>>>>> =
    OnceLock::new();

/// Returns the process-wide configured-state cell, initialising
/// it on first access. The inner `Option` is `None` until the host
/// sends `plugin.configure`; legacy env-var-only daemons leave it
/// `None` forever and the plugin falls through to env reading.
pub fn configured_state() -> &'static Arc<RwLock<Option<Vec<TelegramPluginConfig>>>> {
    CONFIGURED.get_or_init(|| Arc::new(RwLock::new(None)))
}
