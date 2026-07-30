use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};
use tokio::sync::{oneshot, Mutex};

pub const APPROVAL_TIMEOUT_SECS: u64 = 60;

#[derive(Clone, Serialize, Deserialize)]
pub struct PairedDevice {
    pub device_id: String,
    pub device_name: String,
    pub token: String,
    pub paired_at: u64,
}

pub struct PairingState {
    file: PathBuf,
    devices: Mutex<Vec<PairedDevice>>,
    pending: Mutex<HashMap<String, oneshot::Sender<bool>>>,
}

pub fn generate_token() -> String {
    random_hex(24)
}

fn random_hex(len_bytes: usize) -> String {
    let mut bytes = vec![0u8; len_bytes];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl PairingState {
    pub fn new(file: PathBuf) -> Self {
        let devices = std::fs::read_to_string(&file)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self {
            file,
            devices: Mutex::new(devices),
            pending: Mutex::new(HashMap::new()),
        }
    }

    async fn save(&self) {
        let devices = self.devices.lock().await;
        if let Ok(json) = serde_json::to_string_pretty(&*devices) {
            // buttons.json과 같은 이유로 임시 파일 + rename으로 원자적으로 교체한다.
            let tmp_path = self.file.with_extension("json.tmp");
            if tokio::fs::write(&tmp_path, json).await.is_ok() {
                let _ = tokio::fs::rename(&tmp_path, &self.file).await;
            }
        }
    }

    pub async fn list(&self) -> Vec<PairedDevice> {
        self.devices.lock().await.clone()
    }

    pub async fn unpair(&self, device_id: &str) {
        {
            let mut devices = self.devices.lock().await;
            devices.retain(|d| d.device_id != device_id);
        }
        self.save().await;
    }

    pub async fn matching_token(&self, device_id: &str) -> Option<String> {
        let devices = self.devices.lock().await;
        devices
            .iter()
            .find(|d| d.device_id == device_id)
            .map(|d| d.token.clone())
    }

    pub async fn upsert(&self, device_id: &str, device_name: &str, token: &str) {
        {
            let mut devices = self.devices.lock().await;
            if let Some(existing) = devices.iter_mut().find(|d| d.device_id == device_id) {
                existing.device_name = device_name.to_string();
                existing.token = token.to_string();
                existing.paired_at = now_epoch_secs();
            } else {
                devices.push(PairedDevice {
                    device_id: device_id.to_string(),
                    device_name: device_name.to_string(),
                    token: token.to_string(),
                    paired_at: now_epoch_secs(),
                });
            }
        }
        self.save().await;
    }

    // 승인 대기 항목을 등록하고 PC 창을 앞으로 띄운 뒤, 사용자의 허용/거부를
    // 기다릴 수 있는 (request_id, receiver)를 돌려준다. 실제 대기(select! 등)는
    // 호출부(ws.rs)가 소켓 읽기와 함께 동시에 처리한다 - 여기서 그냥 await해버리면
    // 그동안 클라이언트가 보내는 WS ping에 응답을 못 해서 OkHttp가 타임아웃으로
    // 알아서 끊어버리고 재연결을 반복하며 승인 대기 항목이 계속 쌓이는 버그가 있었다.
    pub async fn register_pending(
        &self,
        app: &AppHandle,
        device_name: &str,
    ) -> (String, oneshot::Receiver<bool>) {
        let request_id = random_hex(16);
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().await;
            pending.insert(request_id.clone(), tx);
        }

        crate::winfocus::bring_own_window_to_front(app);
        let _ = app.emit(
            "pair-request",
            serde_json::json!({ "request_id": request_id, "device_name": device_name }),
        );

        (request_id, rx)
    }

    // 이미 응답이 왔으면(resolve_pending이 먼저 remove) 아무 일도 안 하는 no-op.
    // 연결이 끊기거나 타임아웃으로 대기를 그만둘 때 남은 항목을 정리하는 용도.
    pub async fn unregister_pending(&self, request_id: &str) {
        let mut pending = self.pending.lock().await;
        pending.remove(request_id);
    }

    pub async fn resolve_pending(&self, request_id: &str, approved: bool) {
        let mut pending = self.pending.lock().await;
        if let Some(tx) = pending.remove(request_id) {
            let _ = tx.send(approved);
        }
    }
}
