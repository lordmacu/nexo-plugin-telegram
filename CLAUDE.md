# nexo-plugin-telegram — operator guide

Standalone repo for the Telegram subprocess plugin extracted from
`proyecto/crates/plugins/telegram/` in Phase 81.18.

## Shape B reminder

This repo follows the same lib + bin pattern as
`nexo-rs-plugin-browser`:

- `src/lib.rs` re-exports `TelegramPlugin` and friends so a future
  embedded build (Phase 90) can drop the subprocess loop and use the
  plugin in-process.
- `src/main.rs` is the **only** subprocess-specific code. It wraps
  `TelegramPlugin` in `PluginAdapter`, runs the JSON-RPC loop over
  stdio, and seeds the plugin from the env vars the daemon set
  before spawn.

## Multi-instance

A daemon configured with two `[plugin.telegram]` entries spawns two
binaries — not one binary handling both bots. Per-instance state
(`offset` cursor, `media` dedup dir, `instance` topic suffix) lives
under paths scoped by the daemon-supplied env vars; the subprocess
never enumerates siblings.

## Long polling offset

`NEXO_PLUGIN_TELEGRAM_OFFSET_PATH` points at a file the subprocess
writes the latest `getUpdates` `update_id + 1` to on every
successful drain. On restart it loads the value, so a crash
between drain and ack at most replays the last batch. Operators
must NOT delete this file — Telegram will only re-deliver updates
within its own buffer (typically last 24h, capped at ~100 unread).

## Debugging tips

- Pipe the daemon's stdio handshake by hand:
  `cat handshake.json | nexo-plugin-telegram | jq .`
- `RUST_LOG=trace,nexo_plugin_telegram::plugin=trace` exposes every
  bridge wait, every chat_action heartbeat, every media download
  attempt.
- The binary panics on startup if `NEXO_BROKER_URL` is missing;
  that's intentional — running without a broker is silent failure.

## Pairing adapter

`PairingChannelAdapter` impl is part of the lib surface
(`lib.rs::pub use pairing_adapter::TelegramPairingAdapter`) so
embedded consumers can use it. The daemon, however, ships its own
inline copy in `proyecto/src/telegram_pairing_adapter.rs` — that
copy holds the production wiring (broker handle, session store).
The two MUST stay byte-equivalent on the message-format side
(MarkdownV2 escape, topic naming) to avoid pairing flow drift.

## Mining references

Phases that touched the Telegram surface: 81.12.b (NexoPlugin
trait dual-impl), 81.18 (extract), 82.15.bx (broker capability
manifest declaration). Per-step rationale lives in
`proyecto/PHASES.md` and `proyecto/PHASES-curated.md`.
