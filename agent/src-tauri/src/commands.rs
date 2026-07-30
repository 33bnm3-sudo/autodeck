use crate::icon::{extract_icon_data_url, label_from_path};
use crate::state::{AppState, LaunchKind};
use crate::ws::PORT;
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, State};

#[derive(Serialize)]
pub struct SlotView {
    id: String,
    target: Option<String>,
    label: Option<String>,
    icon: Option<String>,
}

#[tauri::command]
pub async fn get_slots(state: State<'_, Arc<AppState>>) -> Result<Vec<SlotView>, String> {
    let slots = state.slots.lock().await;
    Ok(slots
        .iter()
        .map(|s| SlotView {
            id: s.id.clone(),
            target: s.target.clone(),
            label: s.label.clone(),
            icon: s.icon.clone(),
        })
        .collect())
}

#[tauri::command]
pub async fn assign_button(
    state: State<'_, Arc<AppState>>,
    slot_id: String,
    path: String,
) -> Result<(), String> {
    let is_shortcut = path.to_lowercase().ends_with(".lnk");
    // 바로가기면 대상 경로로 풀고, 아니면 그 경로 그대로 쓴다(폴더·파일·exe 모두 허용).
    let resolved = if is_shortcut {
        crate::shortcut::resolve_target(&path)
            .ok_or_else(|| "Could not resolve shortcut target".to_string())?
    } else {
        path.clone()
    };
    // exe는 직접 실행, 그 외(폴더·문서 등)는 셸로 연다.
    let kind = if resolved.to_lowercase().ends_with(".exe") {
        LaunchKind::Exe
    } else {
        LaunchKind::Open
    };
    let icon = extract_icon_data_url(&resolved);
    // 라벨은 바로가기면 바로가기 이름, 아니면 대상 파일/폴더 이름.
    let label = if is_shortcut {
        label_from_path(&path)
    } else {
        label_from_path(&resolved)
    };
    {
        let mut slots = state.slots.lock().await;
        let slot = slots
            .iter_mut()
            .find(|s| s.id == slot_id)
            .ok_or_else(|| format!("Unknown slot: {slot_id}"))?;
        slot.target = Some(resolved);
        slot.kind = kind;
        slot.label = Some(label);
        slot.icon = icon;
    }
    state.save().await;
    let layout = state.layout_json().await;
    state.broadcast_layout(layout);
    Ok(())
}

#[tauri::command]
pub async fn clear_button(state: State<'_, Arc<AppState>>, slot_id: String) -> Result<(), String> {
    {
        let mut slots = state.slots.lock().await;
        if let Some(slot) = slots.iter_mut().find(|s| s.id == slot_id) {
            slot.target = None;
            slot.kind = LaunchKind::Exe;
            slot.label = None;
            slot.icon = None;
        }
    }
    state.save().await;
    let layout = state.layout_json().await;
    state.broadcast_layout(layout);
    Ok(())
}

#[derive(Serialize, Clone)]
pub struct SpecialLocation {
    shell: String,
    label: String,
    icon: Option<String>,
}

/// 드래그로는 잡히지 않는 셸 특수 위치·자주 쓰는 앱 목록(라벨, `explorer`에 넘길 대상).
fn special_defs() -> [(&'static str, &'static str); 9] {
    [
        ("This PC", "shell:MyComputerFolder"),
        ("Recycle Bin", "shell:RecycleBinFolder"),
        ("Downloads", "shell:Downloads"),
        ("Desktop", "shell:Desktop"),
        ("Documents", "shell:Personal"),
        ("Control Panel", "shell:ControlPanelFolder"),
        ("Network", "shell:NetworkPlacesFolder"),
        ("Notepad", r"C:\Windows\System32\notepad.exe"),
        ("Calculator", r"C:\Windows\System32\calc.exe"),
    ]
}

#[tauri::command]
pub fn list_special_locations() -> Vec<SpecialLocation> {
    special_defs()
        .into_iter()
        .map(|(label, shell)| SpecialLocation {
            shell: shell.to_string(),
            label: label.to_string(),
            icon: crate::icon::extract_icon_data_url_shell(shell),
        })
        .collect()
}

#[tauri::command]
pub async fn assign_special(
    state: State<'_, Arc<AppState>>,
    slot_id: String,
    shell: String,
    label: String,
) -> Result<(), String> {
    let icon = crate::icon::extract_icon_data_url_shell(&shell);
    {
        let mut slots = state.slots.lock().await;
        let slot = slots
            .iter_mut()
            .find(|s| s.id == slot_id)
            .ok_or_else(|| format!("Unknown slot: {slot_id}"))?;
        slot.target = Some(shell);
        slot.kind = LaunchKind::Open;
        slot.label = Some(label);
        slot.icon = icon;
    }
    state.save().await;
    let layout = state.layout_json().await;
    state.broadcast_layout(layout);
    Ok(())
}

#[tauri::command]
pub async fn press_slot(
    state: State<'_, Arc<AppState>>,
    app: AppHandle,
    slot_id: String,
) -> Result<(), String> {
    crate::ws::launch_for(&state, &app, &slot_id).await;
    Ok(())
}

#[derive(Serialize)]
pub struct PcInfo {
    ip: String,
    port: u16,
}

#[tauri::command]
pub fn get_pc_info() -> PcInfo {
    let ip = local_ip_address::local_ip()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|_| "Unknown".to_string());
    PcInfo { ip, port: PORT }
}

#[tauri::command]
pub fn get_autostart_enabled(app: AppHandle) -> bool {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
pub fn set_autostart_enabled(app: AppHandle, enabled: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let autolaunch = app.autolaunch();
    let result = if enabled {
        autolaunch.enable()
    } else {
        autolaunch.disable()
    };
    result.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn approve_pair(state: State<'_, Arc<AppState>>, request_id: String) -> Result<(), String> {
    state.pairing.resolve_pending(&request_id, true).await;
    Ok(())
}

#[tauri::command]
pub async fn deny_pair(state: State<'_, Arc<AppState>>, request_id: String) -> Result<(), String> {
    state.pairing.resolve_pending(&request_id, false).await;
    Ok(())
}

#[derive(Serialize)]
pub struct PairedDeviceView {
    device_id: String,
    device_name: String,
    paired_at: u64,
}

#[tauri::command]
pub async fn list_paired_devices(state: State<'_, Arc<AppState>>) -> Result<Vec<PairedDeviceView>, String> {
    Ok(state
        .pairing
        .list()
        .await
        .into_iter()
        .map(|d| PairedDeviceView {
            device_id: d.device_id,
            device_name: d.device_name,
            paired_at: d.paired_at,
        })
        .collect())
}

#[tauri::command]
pub async fn unpair_device(state: State<'_, Arc<AppState>>, device_id: String) -> Result<(), String> {
    state.pairing.unpair(&device_id).await;
    Ok(())
}

#[tauri::command]
pub fn open_sound_settings() {
    let _ = std::process::Command::new("explorer.exe")
        .arg("ms-settings:sound")
        .spawn();
}
