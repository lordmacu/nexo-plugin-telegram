# nexo-plugin-telegram

Telegram bot channel plugin for the [Nexo agent framework][nexo]. Runs
as a subprocess of the daemon, exchanging JSON-RPC frames over stdio
and routing user messages through NATS broker topics.

Out-of-tree per **Phase 81.18**: extracted from
`proyecto/crates/plugins/telegram/` so the plugin can ship and
upgrade independently of the framework, and so a future embedded
build (Phase 90 — Android) can pull `TelegramPlugin` straight out of
the lib surface without dragging the subprocess loop along.

## Layout

```
nexo-rs-plugin-telegram/
├── Cargo.toml             # lib + [[bin]], path-deps interim
├── nexo-plugin.toml       # manifest, [capabilities.broker] declared
├── src/
│   ├── lib.rs             # re-exports for embedded consumers
│   ├── main.rs            # subprocess entrypoint (PluginAdapter loop)
│   ├── plugin.rs          # TelegramPlugin: long-poll loop + dispatch
│   ├── bot.rs             # BotClient: HTTP API wrapper
│   ├── events.rs          # InboundEvent payload types
│   ├── tool.rs            # tool defs + per-tool dispatch
│   ├── session_id.rs      # deterministic session id from chat_id
│   └── pairing_adapter.rs # PairingChannelAdapter impl (lib-only)
└── tests/
    ├── tool_invoke_round_trip.rs  # subprocess end-to-end
    └── ...                         # ported from in-tree crate
```

## Build

```bash
cargo build --release
```

`Cargo.lock` is committed — binary repo convention, reproducible
builds from `git checkout v0.1.1 && cargo install --path .`.

## Daemon wiring

The daemon spawns this binary per `plugin.telegram[]` config entry
and seeds it with the env vars below. None of these are read from
disk; the daemon is the single source of truth for runtime config.

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `NEXO_PLUGIN_TELEGRAM_TOKEN`            | yes | — | bot token (resolved from `secrets/` by the daemon) |
| `NEXO_PLUGIN_TELEGRAM_INSTANCE`         | no  | `""` | topic suffix; empty = legacy single-bot |
| `NEXO_PLUGIN_TELEGRAM_ALLOWLIST`        | no  | `[]` | JSON array of chat ids; empty = no allowlist |
| `NEXO_PLUGIN_TELEGRAM_INTERVAL_MS`      | no  | `1500` | long-poll cadence in ms |
| `NEXO_PLUGIN_TELEGRAM_BRIDGE_TIMEOUT_MS`| no  | `30000` | poller wait for matched reply in ms |
| `NEXO_PLUGIN_TELEGRAM_OFFSET_PATH`      | yes | — | persistent `getUpdates` cursor file |
| `NEXO_PLUGIN_TELEGRAM_MEDIA_DIR`        | yes | — | dedup cache for `(chat_id, msg_id, file_id)` |
| `NEXO_BROKER_URL`                       | yes | — | NATS endpoint (already set globally) |
| `RUST_LOG`                              | no  | `info` | tracing filter |

Multi-bot config: spawn one binary per instance. Topics, offset path
and media dir are scoped per `INSTANCE` so the binaries don't
contend on shared state.

## Topics

- `plugin.inbound.telegram.<instance>` — `InboundEvent` payload
  (Telegram → agent)
- `plugin.outbound.telegram.<instance>` — `Command` payload
  (agent → Telegram)
- Legacy single-bot (no instance): `plugin.inbound.telegram` /
  `plugin.outbound.telegram`

## Running standalone (debug)

```bash
NEXO_PLUGIN_TELEGRAM_TOKEN=12345:abcdef \
NEXO_PLUGIN_TELEGRAM_OFFSET_PATH=/tmp/tg.offset \
NEXO_PLUGIN_TELEGRAM_MEDIA_DIR=/tmp/tg.media \
NEXO_BROKER_URL=nats://127.0.0.1:4222 \
RUST_LOG=debug,nexo_plugin_telegram=trace \
cargo run --bin nexo-plugin-telegram
```

The binary speaks JSON-RPC on stdin/stdout. Pipe the daemon's
handshake into it manually, or run via the daemon's discovery
walker which seeds the env automatically.

## Path-dep disclaimer

Until the proyecto-side crates land on crates.io, every `cargo
build` of this repo expects the layout

```
~/chat/
├── nexo-rs-plugin-telegram/   ← this repo
└── proyecto/                  ← Nexo framework workspace
    └── crates/{microapp-sdk,broker,core,config,llm,auth,pairing,resilience,plugin-manifest}/
```

If `proyecto/` isn't adjacent, override the path-deps in your
local `Cargo.toml` or wait for the Phase 81.18.c crates.io publish
wave.

[nexo]: https://github.com/lordmacu/nexo-rs
