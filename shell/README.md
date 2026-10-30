# shell

Desktop shell (Tauri UI + Wayland integration). Must support full RTL mirroring and runtime language switching. See ../docs/i18n-and-arabic.md.

## Layout

| Path | Purpose |
|------|---------|
| `ui/` | The assistant panel (HTML, CSS, JavaScript). Shared by the dev bridge and the Tauri window. |
| `dev-bridge.mjs` | Development only: serves `ui/` and `../locales/` over loopback and runs the core. |
| `src-tauri/` | The Tauri window. It starts `scarfeeos-core serve` and passes requests to it. |

## Tauri window

The window loads `ui/` and `../locales/` (gathered into `src-tauri/dist/` by `build.rs`, which is not committed). The panel posts `/chat`; `src-tauri/chat-shim.js` answers that call through one Tauri command, `ask`, which takes a request line and returns the response line with the same `id` (the JSON lines in `../core/src/ipc.rs`). The shell owns one `scarfeeos-core serve` child, runs requests one at a time, and starts the child again if it stops or does not answer within 5 minutes. Nothing is served over HTTP.

The window has a strict Content Security Policy (`default-src 'none'`, scripts and styles from the app only, no remote content) and no bundled fonts.

The core is `$SCARFEEOS_CORE`, else `scarfeeos-core` next to the shell program, else `scarfeeos-core` on the `PATH`. The core reads its own settings (`SCARFEEOS_COMMAND_URL`, `SCARFEEOS_WRITER_URL`) from the environment.

### Build and run

On Fedora: `sudo dnf install webkit2gtk4.1-devel gtk3-devel`. On Debian and Ubuntu: `sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev pkg-config`.

```
. ~/.cargo/env
cargo build --manifest-path core/Cargo.toml
SCARFEEOS_CORE=core/target/debug/scarfeeos-core cargo run --manifest-path shell/src-tauri/Cargo.toml
```

It needs a display (Wayland or X11) and exits with a message when there is none.

### Tests

```
cargo test --manifest-path shell/src-tauri/Cargo.toml   # child-process handling, with a fake core script
node --test shell/src-tauri/chat-shim.test.mjs          # the /chat to ask mapping
```

Licenses: `cargo deny --manifest-path shell/src-tauri/Cargo.toml --config core/deny.toml check licenses`.
