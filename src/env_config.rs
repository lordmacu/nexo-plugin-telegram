//! Env-var → `TelegramPluginConfig` reader.
//!
//! The daemon seeds these vars before spawning the subprocess; the
//! plugin never reads YAML directly. Mirrors the
//! `browser_config_from_env` shape used by `nexo-plugin-browser`.

use anyhow::{Context, Result};

use crate::config::{
    TelegramAllowlistConfig, TelegramAutoTranscribeConfig, TelegramPluginConfig,
    TelegramPollingConfig,
};

const ENV_TOKEN: &str = "NEXO_PLUGIN_TELEGRAM_TOKEN";
const ENV_INSTANCE: &str = "NEXO_PLUGIN_TELEGRAM_INSTANCE";
const ENV_ALLOWLIST: &str = "NEXO_PLUGIN_TELEGRAM_ALLOWLIST";
const ENV_INTERVAL_MS: &str = "NEXO_PLUGIN_TELEGRAM_INTERVAL_MS";
const ENV_BRIDGE_TIMEOUT_MS: &str = "NEXO_PLUGIN_TELEGRAM_BRIDGE_TIMEOUT_MS";
const ENV_OFFSET_PATH: &str = "NEXO_PLUGIN_TELEGRAM_OFFSET_PATH";
const ENV_AUTO_TRANSCRIBE: &str = "NEXO_PLUGIN_TELEGRAM_AUTO_TRANSCRIBE";
const ENV_WHISPER_CMD: &str = "NEXO_PLUGIN_TELEGRAM_WHISPER_COMMAND";
const ENV_WHISPER_TIMEOUT_MS: &str = "NEXO_PLUGIN_TELEGRAM_WHISPER_TIMEOUT_MS";
const ENV_WHISPER_LANG: &str = "NEXO_PLUGIN_TELEGRAM_WHISPER_LANGUAGE";

/// Build a [`TelegramPluginConfig`] from the daemon-supplied env
/// vars. Fails with an operator-readable hint on the first missing
/// or malformed value so subprocess boot logs name the offender.
pub fn telegram_config_from_env() -> Result<TelegramPluginConfig> {
    let token = std::env::var(ENV_TOKEN)
        .with_context(|| format!("{ENV_TOKEN} missing — daemon must seed the bot token"))?;
    if token.trim().is_empty() {
        anyhow::bail!("{ENV_TOKEN} is empty — bot token must be a non-empty string");
    }

    let instance = match std::env::var(ENV_INSTANCE) {
        Ok(s) if !s.trim().is_empty() => Some(s),
        _ => None,
    };

    let allowlist = parse_allowlist()?;

    let polling_interval_ms = parse_u64(ENV_INTERVAL_MS, 1500)?;
    let offset_path = std::env::var(ENV_OFFSET_PATH)
        .with_context(|| format!("{ENV_OFFSET_PATH} missing — long-poll cursor file required"))?;
    if offset_path.trim().is_empty() {
        anyhow::bail!("{ENV_OFFSET_PATH} is empty — supply a writable file path");
    }

    let bridge_timeout_ms = parse_u64(ENV_BRIDGE_TIMEOUT_MS, 30_000)?;

    let auto_transcribe = parse_auto_transcribe()?;

    Ok(TelegramPluginConfig {
        token,
        polling: TelegramPollingConfig {
            enabled: true,
            interval_ms: polling_interval_ms,
            offset_path: Some(offset_path),
        },
        allowlist,
        auto_transcribe,
        bridge_timeout_ms,
        instance,
        // Subprocess plugins enforce the agent allowlist via the
        // resolver's `credentials.telegram` binding upstream (daemon
        // side). Leaving the per-plugin override empty keeps that
        // single-source-of-truth.
        allow_agents: Vec::new(),
    })
}

fn parse_u64(var: &str, default: u64) -> Result<u64> {
    match std::env::var(var) {
        Ok(s) if !s.trim().is_empty() => s
            .trim()
            .parse::<u64>()
            .with_context(|| format!("{var}={s:?} is not a non-negative integer")),
        _ => Ok(default),
    }
}

fn parse_allowlist() -> Result<TelegramAllowlistConfig> {
    match std::env::var(ENV_ALLOWLIST) {
        Ok(s) if !s.trim().is_empty() => {
            let chat_ids: Vec<i64> = serde_json::from_str(&s).with_context(|| {
                format!("{ENV_ALLOWLIST}={s:?} must be a JSON array of i64 chat ids")
            })?;
            Ok(TelegramAllowlistConfig { chat_ids })
        }
        _ => Ok(TelegramAllowlistConfig::default()),
    }
}

fn parse_auto_transcribe() -> Result<TelegramAutoTranscribeConfig> {
    let enabled = match std::env::var(ENV_AUTO_TRANSCRIBE) {
        Ok(s) => matches!(s.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"),
        Err(_) => false,
    };
    let command = std::env::var(ENV_WHISPER_CMD).unwrap_or_else(|_| {
        "./extensions/openai-whisper/target/release/openai-whisper".to_string()
    });
    let timeout_ms = parse_u64(ENV_WHISPER_TIMEOUT_MS, 60_000)?;
    let language = match std::env::var(ENV_WHISPER_LANG) {
        Ok(s) if !s.trim().is_empty() => Some(s),
        _ => None,
    };
    Ok(TelegramAutoTranscribeConfig {
        enabled,
        command,
        timeout_ms,
        language,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    fn clear_all() {
        for var in [
            ENV_TOKEN,
            ENV_INSTANCE,
            ENV_ALLOWLIST,
            ENV_INTERVAL_MS,
            ENV_BRIDGE_TIMEOUT_MS,
            ENV_OFFSET_PATH,
            ENV_AUTO_TRANSCRIBE,
            ENV_WHISPER_CMD,
            ENV_WHISPER_TIMEOUT_MS,
            ENV_WHISPER_LANG,
        ] {
            std::env::remove_var(var);
        }
    }

    #[test]
    #[serial]
    fn config_happy_path() {
        clear_all();
        std::env::set_var(ENV_TOKEN, "12345:abcdef");
        std::env::set_var(ENV_OFFSET_PATH, "/tmp/tg.offset");
        std::env::set_var(ENV_INSTANCE, "boss");
        std::env::set_var(ENV_ALLOWLIST, "[100, 200]");
        std::env::set_var(ENV_INTERVAL_MS, "2000");

        let cfg = telegram_config_from_env().expect("happy path");
        assert_eq!(cfg.token, "12345:abcdef");
        assert_eq!(cfg.instance.as_deref(), Some("boss"));
        assert_eq!(cfg.allowlist.chat_ids, vec![100, 200]);
        assert_eq!(cfg.polling.interval_ms, 2000);
        assert_eq!(cfg.polling.offset_path.as_deref(), Some("/tmp/tg.offset"));
        assert!(!cfg.auto_transcribe.enabled);
        clear_all();
    }

    #[test]
    #[serial]
    fn config_missing_token_errors() {
        clear_all();
        std::env::set_var(ENV_OFFSET_PATH, "/tmp/tg.offset");
        let err = telegram_config_from_env().unwrap_err();
        assert!(
            err.to_string().contains(ENV_TOKEN),
            "error must name the missing var, got: {err}"
        );
        clear_all();
    }

    #[test]
    #[serial]
    fn config_invalid_allowlist_json_errors() {
        clear_all();
        std::env::set_var(ENV_TOKEN, "12345:abcdef");
        std::env::set_var(ENV_OFFSET_PATH, "/tmp/tg.offset");
        std::env::set_var(ENV_ALLOWLIST, "[not, json");
        let err = telegram_config_from_env().unwrap_err();
        assert!(
            err.to_string().contains(ENV_ALLOWLIST),
            "error must name the offending var, got: {err}"
        );
        clear_all();
    }

    #[test]
    #[serial]
    fn config_empty_token_errors() {
        clear_all();
        std::env::set_var(ENV_TOKEN, "   ");
        std::env::set_var(ENV_OFFSET_PATH, "/tmp/tg.offset");
        let err = telegram_config_from_env().unwrap_err();
        assert!(err.to_string().contains("empty"), "got: {err}");
        clear_all();
    }
}
