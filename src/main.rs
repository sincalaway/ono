#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
fn main() {
    ono::builder()
        .build(tauri::generate_context!())
        .expect("无法初始化 ono")
        .run(|_, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                ono::shutdown();
            }
        })
}
