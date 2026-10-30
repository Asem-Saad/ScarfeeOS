# shell

Desktop shell (Tauri UI + Wayland integration). Must support full RTL mirroring and runtime language switching. See ../docs/i18n-and-arabic.md.

## Layout

| Path | Purpose |
|------|---------|
| `ui/` | The assistant panel (HTML, CSS, JavaScript). Shared by the dev bridge and the Tauri window. |
| `dev-bridge.mjs` | Development only: serves `ui/` and `../locales/` over loopback and runs the core. |
| `src-tauri/` | The Tauri window. It starts `scarfeeos-core serve` and passes requests to it. |

## Tauri window

The window loads `ui/` and `../locales/` (gathered into `src-tauri/dist/` by `build.rs`, which is not committed). The panel and the calendar page post `/chat`; `src-tauri/chat-shim.js` answers that call through one Tauri command, `ask`, which takes a request line and returns the response line with the same `id` (the JSON lines in `../core/src/ipc.rs`). The shim accepts the same six requests as the dev bridge (a turn, `confirm`, `cancel`, `upcoming`, `delete_event`, `hardware`) and gives every request its own `id`. The shell owns one `scarfeeos-core serve` child, runs requests one at a time, and starts the child again if it stops or does not answer within 5 minutes. Nothing is served over HTTP.

The window has a strict Content Security Policy (`default-src 'none'`, scripts and styles from the app only, no remote content) and no bundled fonts.

The core is `$SCARFEEOS_CORE`, else `scarfeeos-core` next to the shell program, else `scarfeeos-core` on the `PATH`. The core reads its own settings (`SCARFEEOS_COMMAND_URL`, `SCARFEEOS_WRITER_URL`) from the environment.

### Build and run

On Fedora: `sudo dnf install webkit2gtk4.1-devel gtk3-devel`. On Debian and Ubuntu: `sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev pkg-config`.

```
. ~/.cargo/env
cargo build --manifest-path core/Cargo.toml
SCARFEEOS_CORE=core/target/debug/scarfeeos-core cargo run --manifest-path shell/src-tauri/Cargo.toml
```

It needs a display (Wayland or X11) and exits with a message when there is none. The window has no browser toolbar, so Alt+Left goes back from the calendar page to the panel.

### Without a screen

The window was opened under Xvfb (an X server with no screen) on WSL2 with webkit2gtk 2.52, with a fake core script that logged each request line. A text turn, a confirm and the calendar's `upcoming` all reached the core and came back, in English and in Arabic (mirrored). On Xvfb, WebKit prints two `libEGL warning: DRI3` lines; they are harmless. If Xvfb is not installed and you have no root, `apt-get download xvfb xserver-common libxfont2 x11-xkb-utils libfontenc1 libxkbfile1 libxcvt0 xauth`, unpack the files with `dpkg -x` and run Xvfb with `LD_LIBRARY_PATH` set to the unpacked `usr/lib/x86_64-linux-gnu`. Xvfb runs `/usr/bin/xkbcomp`, so inside `unshare -rm` mount an overlay of the unpacked `usr/bin` over `/usr/bin`. Run it with `-fbdir <dir>`: the screen is then the XWD file `<dir>/Xvfb_screen0`, which a short script can turn into a PNG, and `xdotool` types and clicks.

### Tests

```
cargo test --manifest-path shell/src-tauri/Cargo.toml   # child-process handling, with a fake core script
node --test shell/src-tauri/chat-shim.test.mjs          # the /chat to ask mapping
```

Licenses: `cargo deny --manifest-path shell/src-tauri/Cargo.toml --config core/deny.toml check licenses`.
