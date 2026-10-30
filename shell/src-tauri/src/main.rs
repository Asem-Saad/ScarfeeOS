//! The ScarfeeOS shell: a Tauri window around the assistant panel in ../ui.
//! The panel's `/chat` request reaches the core through the `ask` command.

mod core_process;

use std::sync::Arc;

use core_process::Core;
use tauri::{Manager, RunEvent, State, WebviewUrl, WebviewWindowBuilder};

/// Sends one request line to the core and returns the response line with the same id.
#[tauri::command]
async fn ask(core: State<'_, Arc<Core>>, line: String) -> Result<String, String> {
    let core = Arc::clone(&core);
    tauri::async_runtime::spawn_blocking(move || core.ask(&line))
        .await
        .map_err(|e| format!("the request was interrupted: {e}"))?
}

fn main() {
    #[cfg(target_os = "linux")]
    if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
        eprintln!("scarfeeos-shell: no display (neither WAYLAND_DISPLAY nor DISPLAY is set), so no window can open");
        std::process::exit(1);
    }

    let app = tauri::Builder::default()
        .manage(Arc::new(Core::serve_command()))
        .invoke_handler(tauri::generate_handler![ask])
        .setup(|app| {
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("ScarfeeOS")
                .inner_size(1000.0, 700.0)
                .initialization_script(include_str!("../chat-shim.js"))
                .build()?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .unwrap_or_else(|e| {
            eprintln!("scarfeeos-shell: {e}");
            std::process::exit(1);
        });
    app.run(|app, event| {
        if let RunEvent::Exit = event {
            app.state::<Arc<Core>>().shutdown();
        }
    });
}
