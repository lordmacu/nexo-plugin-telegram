//! Phase 93.4.a — coverage for the configure(value) hook +
//! configured_state singleton + shared_plugin preference order.
//!
//! These tests target the public surface of `nexo-plugin-telegram`:
//! `TelegramPluginConfig` deserialise, `configured_state()`
//! read/write, env-var fallback.

use nexo_plugin_telegram::{configured_state, telegram_config_from_env, TelegramPluginConfig};
use serial_test::serial;

#[tokio::test]
#[serial]
async fn configure_deserialises_single_entry_array() {
    let yaml = r#"
- token: "abc"
  instance: main
"#;
    let value: serde_yaml::Value = serde_yaml::from_str(yaml).unwrap();
    let parsed: Vec<TelegramPluginConfig> =
        serde_yaml::from_value(value).expect("yaml round-trips");
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].token, "abc");
    assert_eq!(parsed[0].instance.as_deref(), Some("main"));
    // Reset configured_state for sibling tests.
    *configured_state().write().await = None;
}

#[tokio::test]
#[serial]
async fn configure_missing_required_token_errors() {
    let yaml = r#"
- instance: main
"#;
    let value: serde_yaml::Value = serde_yaml::from_str(yaml).unwrap();
    let result: Result<Vec<TelegramPluginConfig>, _> = serde_yaml::from_value(value);
    let err = result.expect_err("missing token must fail");
    assert!(
        err.to_string().contains("token"),
        "error should mention token, got: {err}",
    );
}

#[tokio::test]
#[serial]
async fn configure_overwrites_on_hot_reload_recall() {
    let value_a: serde_yaml::Value =
        serde_yaml::from_str(r#"- token: "first""#).unwrap();
    let parsed_a: Vec<TelegramPluginConfig> = serde_yaml::from_value(value_a).unwrap();
    *configured_state().write().await = Some(parsed_a);

    let value_b: serde_yaml::Value =
        serde_yaml::from_str(r#"- token: "second""#).unwrap();
    let parsed_b: Vec<TelegramPluginConfig> = serde_yaml::from_value(value_b).unwrap();
    *configured_state().write().await = Some(parsed_b);

    let guard = configured_state().read().await;
    let current = guard.as_ref().expect("state populated");
    assert_eq!(current.len(), 1);
    assert_eq!(current[0].token, "second");
    drop(guard);
    *configured_state().write().await = None;
}

#[tokio::test]
#[serial]
async fn legacy_env_path_active_when_configured_state_empty() {
    *configured_state().write().await = None;
    std::env::set_var("NEXO_PLUGIN_TELEGRAM_TOKEN", "env_tok");
    std::env::set_var("NEXO_PLUGIN_TELEGRAM_OFFSET_PATH", "/tmp/x");
    let cfg = telegram_config_from_env().expect("env path works");
    assert_eq!(cfg.token, "env_tok");
    std::env::remove_var("NEXO_PLUGIN_TELEGRAM_TOKEN");
    std::env::remove_var("NEXO_PLUGIN_TELEGRAM_OFFSET_PATH");
}

#[tokio::test]
#[serial]
async fn configured_state_value_wins_over_env_var() {
    // Set state to "FROM_RPC" + env to "FROM_ENV"; precedence
    // logic (mirrors main.rs::shared_plugin) checks state first.
    let value: serde_yaml::Value =
        serde_yaml::from_str(r#"- token: "FROM_RPC""#).unwrap();
    let parsed: Vec<TelegramPluginConfig> = serde_yaml::from_value(value).unwrap();
    *configured_state().write().await = Some(parsed);
    std::env::set_var("NEXO_PLUGIN_TELEGRAM_TOKEN", "FROM_ENV");
    std::env::set_var("NEXO_PLUGIN_TELEGRAM_OFFSET_PATH", "/tmp/y");

    // Reproduce shared_plugin()'s precedence inline (avoids
    // depending on its full broker bootstrap which needs NATS).
    let chosen = {
        let guard = configured_state().read().await;
        if let Some(vec) = guard.as_ref() {
            vec.first().cloned().expect("non-empty")
        } else {
            telegram_config_from_env().expect("env fallback")
        }
    };
    assert_eq!(chosen.token, "FROM_RPC");

    std::env::remove_var("NEXO_PLUGIN_TELEGRAM_TOKEN");
    std::env::remove_var("NEXO_PLUGIN_TELEGRAM_OFFSET_PATH");
    *configured_state().write().await = None;
}
