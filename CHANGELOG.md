# Changelog

## 0.3.0 — 2026-05-15

### Added

- Manifest declares `[plugin.credentials_schema] enabled = true,
  accounts_shape = "array"` (Phase 93.8.a-daemon). Daemon-side
  `SubprocessNexoPlugin::credential_store()` now constructs a
  `RemoteCredentialStore` for this plugin and registers it into
  `bundle.stores_v2["telegram"]` via the Phase 93.7 init-loop
  helper.
- `src/main.rs` registers the four Phase 93.8.a-sdk handlers:
  - `on_credentials_list` — returns configured `instance`
    labels from `configured_state()`.
  - `on_credentials_issue` — allow-list check against the
    matching `TelegramPluginConfig.allow_agents`. Empty list
    accepts any agent; explicit non-match returns
    `not_permitted`; unknown account returns `not_found`.
  - `on_credentials_resolve_bytes` — serialises the full
    `TelegramPluginConfig` (including `token`) via
    `serde_json::to_vec`. Daemon-side consumers (Phase 93.8.b+)
    deserialise with `serde_json::from_slice::<TelegramPluginConfig>(&bytes)`.
    Bytes flow only through the resolver + breaker chain.
  - `on_credentials_reload` — no-op + Ok. Telegram has no
    live-reload path; the operator's YAML re-delivers via
    `plugin.configure` on file-watcher fire (Phase 93.2).
- `TelegramPluginConfig` + its sub-structs gain `Serialize`
  derive (needed by `resolve_bytes` handler's
  `serde_json::to_vec`).
- 5 new integration tests in `tests/credentials_path.rs` —
  list / issue allow-list / issue not-found / issue
  not-permitted / resolve_bytes round-trip.
- `nexo-microapp-sdk` path-dep pin bumped 0.1.12 → 0.1.15 to
  pick up the `PluginAdapter::on_credentials_*` builders + the
  `CredentialsListReply` struct (Phase 93.8.a-sdk, proyecto
  commit `e59ba0be`).

### Backward compatibility

- Daemon-side typed `bundle.stores.telegram` keeps serving
  consumers through the Phase 93.9 deprecation window. The new
  `bundle.stores_v2["telegram"]` runs in parallel; no consumer
  reads it yet.

## 0.2.0 — 2026-05-14

### Breaking

- Plugin owns its config types. `nexo_config::types::plugins::TelegramPluginConfig`
  and sub-structs (`TelegramPollingConfig`,
  `TelegramAllowlistConfig`, `TelegramAutoTranscribeConfig`) no
  longer come from `nexo-config`; the equivalent definitions
  live in the new `nexo_plugin_telegram::config` module. Field
  shapes byte-for-byte identical so operator YAML keeps working.
- Public `telegram_plugin_factory(cfg: TelegramPluginConfig)`
  factory function removed. The binary entrypoint
  (`PluginAdapter::run_stdio`) is unaffected.
- Manifest version bumped `"0.1.4" → "0.2.0"` to match crate.

### Added

- Manifest declares `[plugin.config_schema]` (Phase 93.1) with
  `shape = "array"` + JSON Schema for required `token` +
  optional polling / allowlist / auto-transcribe / allow_agents.
- SDK `on_configure(...)` handler receives the operator YAML
  slice via the new `plugin.configure` JSON-RPC method (Phase
  93.2 host + Phase 93.4.a-sdk plugin-adapter hook). Handler
  deserialises the sequence into `Vec<TelegramPluginConfig>` and
  caches the value via `configured_state()`.
- `shared_plugin()` now prefers configured state; falls back to
  `telegram_config_from_env()` when state is empty.
- 5 new integration tests in `tests/configure_path.rs` —
  deserialise / missing-token error / hot-reload re-call /
  legacy env-var fallback / precedence.

### Removed

- `nexo-config` direct dep (still pulled transitively for
  unrelated broker types; not a plugin-config coupling any more).
- `pub use telegram_plugin_factory` from `lib.rs`.

### Backward compatibility

- Env-var fallback (`NEXO_PLUGIN_TELEGRAM_*` vars) keeps working
  when the daemon does NOT deliver `plugin.configure`. Removed
  in a future 0.3.0 once Phase 93.5 closes the daemon-side
  typed-fields deprecation window.

## 0.1.1 — 2026-05-09

Initial extract from `proyecto/crates/plugins/telegram/` per Phase
81.18. Sources copied verbatim; no behavioural change. Surface
re-exports from `lib.rs`:

- `TelegramPlugin`, `TOPIC_INBOUND`, `TOPIC_OUTBOUND`
- `InboundEvent`
- `register_telegram_tools`
- `session_id_for_chat`
- `TelegramPairingAdapter` (lib-only; daemon-side adapter ships
  inline in `proyecto/src/telegram_pairing_adapter.rs`)

`main.rs` is new: wraps the plugin in
`nexo_microapp_sdk::plugin::PluginAdapter` and runs the JSON-RPC
loop over stdio. Manifest is bundled at compile-time via
`include_str!("../nexo-plugin.toml")`.
