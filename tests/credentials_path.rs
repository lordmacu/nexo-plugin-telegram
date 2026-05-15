//! Phase 93.8.a-telegram — coverage for the on_credentials_*
//! handler logic. The handlers themselves live inside `main.rs`
//! and aren't directly callable from integration tests, so these
//! tests exercise the same `configured_state()`-backed lookup
//! logic by inlining the handler bodies.

use nexo_plugin_telegram::{configured_state, TelegramPluginConfig};
use serial_test::serial;

fn parse_cfg(yaml: &str) -> Vec<TelegramPluginConfig> {
    let value: serde_yaml::Value = serde_yaml::from_str(yaml).unwrap();
    serde_yaml::from_value(value).unwrap()
}

/// Returns `accounts` per `on_credentials_list` handler logic.
async fn list_handler() -> Vec<String> {
    let guard = configured_state().read().await;
    guard
        .as_ref()
        .map(|v| {
            v.iter()
                .filter_map(|c| c.instance.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// Mirrors `on_credentials_issue` handler.
async fn issue_handler(account_id: &str, agent_id: &str) -> Result<(), String> {
    let guard = configured_state().read().await;
    let Some(cfgs) = guard.as_ref() else {
        return Err("not_found".to_string());
    };
    let cfg = cfgs
        .iter()
        .find(|c| c.instance.as_deref() == Some(account_id));
    match cfg {
        None => Err("not_found".to_string()),
        Some(c) if c.allow_agents.is_empty() || c.allow_agents.contains(&agent_id.to_string()) => {
            Ok(())
        }
        Some(_) => Err("not_permitted".to_string()),
    }
}

/// Mirrors `on_credentials_resolve_bytes`.
async fn resolve_bytes_handler(account_id: &str) -> Result<Vec<u8>, String> {
    let guard = configured_state().read().await;
    let Some(cfgs) = guard.as_ref() else {
        return Err("not_found".to_string());
    };
    let cfg = cfgs
        .iter()
        .find(|c| c.instance.as_deref() == Some(account_id))
        .ok_or_else(|| "not_found".to_string())?;
    serde_json::to_vec(cfg).map_err(|e| format!("serialize failed: {e}"))
}

#[tokio::test]
#[serial]
async fn list_returns_configured_instance_names() {
    let cfgs = parse_cfg(
        r#"
- token: "abc"
  instance: main
- token: "def"
  instance: secondary
"#,
    );
    *configured_state().write().await = Some(cfgs);
    let accounts = list_handler().await;
    assert_eq!(accounts.len(), 2);
    assert!(accounts.contains(&"main".to_string()));
    assert!(accounts.contains(&"secondary".to_string()));
    *configured_state().write().await = None;
}

#[tokio::test]
#[serial]
async fn issue_permits_when_allow_agents_empty() {
    let cfgs = parse_cfg(
        r#"
- token: "abc"
  instance: main
"#,
    );
    *configured_state().write().await = Some(cfgs);
    // Empty allow_agents → accept any.
    issue_handler("main", "alice").await.expect("accepted");
    *configured_state().write().await = None;
}

#[tokio::test]
#[serial]
async fn issue_rejects_when_account_not_found() {
    *configured_state().write().await = None;
    let err = issue_handler("nonexistent", "alice")
        .await
        .expect_err("expected not_found");
    assert_eq!(err, "not_found");
}

#[tokio::test]
#[serial]
async fn issue_rejects_when_allow_agents_excludes() {
    let cfgs = parse_cfg(
        r#"
- token: "abc"
  instance: main
  allow_agents: ["bob"]
"#,
    );
    *configured_state().write().await = Some(cfgs);
    let err = issue_handler("main", "alice")
        .await
        .expect_err("expected not_permitted");
    assert_eq!(err, "not_permitted");
    *configured_state().write().await = None;
}

#[tokio::test]
#[serial]
async fn resolve_bytes_returns_serde_json_encoded_config() {
    let cfgs = parse_cfg(
        r#"
- token: "secret_token"
  instance: main
"#,
    );
    *configured_state().write().await = Some(cfgs);
    let bytes = resolve_bytes_handler("main")
        .await
        .expect("resolve ok");
    let decoded: TelegramPluginConfig =
        serde_json::from_slice(&bytes).expect("round-trip");
    assert_eq!(decoded.token, "secret_token");
    assert_eq!(decoded.instance.as_deref(), Some("main"));
    *configured_state().write().await = None;
}
