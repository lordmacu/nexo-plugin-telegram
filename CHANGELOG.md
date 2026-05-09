# Changelog

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
