# Changelog

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
