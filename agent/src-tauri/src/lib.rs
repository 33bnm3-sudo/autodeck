mod commands;
mod icon;
mod mdns;
mod mediactl;
mod pairing;
mod shortcut;
mod state;
mod tray;
mod winfocus;
mod ws;

use state::AppState;
use std::sync::Arc;
use tauri::Manager;
use tauri_plugin_autostart::MacosLauncher;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // 같은 앱이 두 개 이상 뜨면 WS 포트 충돌·상태 불일치가 생기니, 두 번째
        // 실행은 곧바로 종료시키고 기존 창을 앞으로 가져온다. 다른 플러그인보다
        // 먼저 등록해야 두 번째 인스턴스가 나머지 setup을 타지 않고 바로 빠진다.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let data_file = data_dir.join("buttons.json");
            let pairing_file = data_dir.join("paired_devices.json");
            let pc_name = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".to_string());
            let state = Arc::new(AppState::new(data_file, pairing_file, pc_name));
            app.manage(state.clone());
            app.manage(mdns::register(&state.pc_name, ws::PORT));

            tray::setup(app.handle())?;

            // 시작 프로그램 등록 여부는 설정 화면의 토글이 사용자 선택으로 관리한다
            // (여기서 매번 강제로 켜면 사용자가 꺼도 재시작할 때마다 도로 켜진다).

            tauri::async_runtime::spawn(ws::run(state, app.handle().clone()));
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_slots,
            commands::assign_button,
            commands::clear_button,
            commands::get_pc_info,
            commands::list_special_locations,
            commands::assign_special,
            commands::press_slot,
            commands::get_autostart_enabled,
            commands::set_autostart_enabled,
            commands::open_sound_settings,
            commands::approve_pair,
            commands::deny_pair,
            commands::list_paired_devices,
            commands::unpair_device,
            mediactl::get_volume,
            mediactl::set_volume
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
