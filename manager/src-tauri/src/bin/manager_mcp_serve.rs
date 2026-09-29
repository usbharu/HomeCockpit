// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;

fn main() {
    let port = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "28765".to_string())
        .parse::<u16>()
        .expect("port must be a number 1..65535");
    if port == 0 {
        panic!("MCP port must be 1..65535.");
    }
    tauri::Builder::default()
        .setup(move |app| {
            let state = Arc::new(manager_lib::RuntimeState::new());
            let server = manager_lib::mcp::start(app.handle().clone(), state, port)
                .map_err(std::io::Error::other)?;
            let _keep_alive = Box::leak(Box::new(server));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running MCP server");
}
