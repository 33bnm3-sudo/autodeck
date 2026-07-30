use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::sync::{broadcast, Mutex};

pub const SLOT_COUNT: usize = 16;

/// 버튼을 눌렀을 때 PC가 대상을 어떻게 여는지.
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum LaunchKind {
    /// 실행 파일: 프로세스로 직접 spawn.
    #[default]
    Exe,
    /// 폴더·문서 등: 셸로 열기(탐색기/연결된 기본 프로그램).
    Open,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ButtonSlot {
    pub id: String,
    // 예전 buttons.json은 "exe_path" 키를 썼으므로 alias로 계속 읽어들인다.
    #[serde(alias = "exe_path")]
    pub target: Option<String>,
    #[serde(default)]
    pub kind: LaunchKind,
    pub label: Option<String>,
    pub icon: Option<String>,
}

impl ButtonSlot {
    fn empty(id: &str) -> Self {
        Self {
            id: id.to_string(),
            target: None,
            kind: LaunchKind::Exe,
            label: None,
            icon: None,
        }
    }
}

#[derive(Serialize)]
struct WireButton {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    icon: Option<String>,
}

#[derive(Serialize)]
struct WireLayout {
    #[serde(rename = "type")]
    kind: &'static str,
    pc: String,
    volume: f32,
    buttons: Vec<WireButton>,
}

pub fn default_slots() -> Vec<ButtonSlot> {
    (0..SLOT_COUNT)
        .map(|i| ButtonSlot::empty(&format!("s{i}")))
        .collect()
}

fn load_slots(path: &PathBuf) -> Option<Vec<ButtonSlot>> {
    let data = std::fs::read_to_string(path).ok()?;
    let slots: Vec<ButtonSlot> = serde_json::from_str(&data).ok()?;
    if slots.len() != SLOT_COUNT {
        return None;
    }
    Some(slots)
}

pub struct AppState {
    pub slots: Mutex<Vec<ButtonSlot>>,
    pub layout_tx: broadcast::Sender<String>,
    pub data_file: PathBuf,
    pub pc_name: String,
    pub pairing: crate::pairing::PairingState,
}

impl AppState {
    pub fn new(data_file: PathBuf, pairing_file: PathBuf, pc_name: String) -> Self {
        let mut slots = load_slots(&data_file).unwrap_or_else(default_slots);
        for slot in slots.iter_mut() {
            if let Some(path) = &slot.target {
                if let Some(icon) = crate::icon::extract_icon_data_url(path) {
                    slot.icon = Some(icon);
                }
            }
        }
        let (layout_tx, _rx) = broadcast::channel(16);
        Self {
            slots: Mutex::new(slots),
            layout_tx,
            data_file,
            pc_name,
            pairing: crate::pairing::PairingState::new(pairing_file),
        }
    }

    pub async fn save(&self) {
        let slots = self.slots.lock().await;
        if let Ok(json) = serde_json::to_string_pretty(&*slots) {
            // 임시 파일에 먼저 쓰고 rename으로 교체한다 - 중간에 죽거나(또는
            // 예전 인스턴스가 겹쳐 뜬 상태에서 두 프로세스가 동시에 쓰다) 파일이
            // 반쯤만 써지면, 다음 시작 시 load_slots의 JSON 파싱이 조용히 실패해
            // 드래그앤드랍한 배치를 통째로 잃어버린 것처럼 보이는 문제를 막는다.
            let tmp_path = self.data_file.with_extension("json.tmp");
            if tokio::fs::write(&tmp_path, json).await.is_ok() {
                let _ = tokio::fs::rename(&tmp_path, &self.data_file).await;
            }
        }
    }

    pub async fn layout_json(&self) -> String {
        let slots = self.slots.lock().await;
        let buttons = slots
            .iter()
            .map(|s| WireButton {
                id: s.id.clone(),
                label: s.label.clone(),
                icon: s.icon.clone(),
            })
            .collect();
        let layout = WireLayout {
            kind: "layout",
            pc: self.pc_name.clone(),
            volume: crate::mediactl::get_volume_raw(),
            buttons,
        };
        serde_json::to_string(&layout).unwrap_or_default()
    }

    pub fn broadcast_layout(&self, json: String) {
        let _ = self.layout_tx.send(json);
    }

    pub async fn target_for(&self, id: &str) -> Option<(String, LaunchKind)> {
        let slots = self.slots.lock().await;
        slots
            .iter()
            .find(|s| s.id == id)
            .and_then(|s| s.target.clone().map(|p| (p, s.kind)))
    }
}
