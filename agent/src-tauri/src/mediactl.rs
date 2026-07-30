use crate::state::AppState;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, OnceLock};
use tauri::State;
use windows::core::Result as WinResult;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
use windows::Win32::System::Com::{
    CLSCTX_ALL, CoCreateInstance, CoInitializeEx, COINIT_APARTMENTTHREADED,
};

fn endpoint_volume() -> WinResult<IAudioEndpointVolume> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
        device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)
    }
}

pub fn get_volume_raw() -> f32 {
    unsafe {
        endpoint_volume()
            .and_then(|v| v.GetMasterVolumeLevelScalar())
            .unwrap_or(0.0)
    }
}

// 드래그 중엔 "volume" WS 메시지가 초당 수십 번 날아온다. 매번 CoCreateInstance부터
// 새로 하면(COM 객체 생성 자체가 몇 ms씩 걸림) 그 지연이 쌓여서 폰 화면보다 실제
// 윈도우 볼륨이 계속 뒤처지는 것처럼 느껴진다. 인터페이스를 딱 한 번만 만들어 전용
// 스레드가 계속 들고 있게 하고(COM은 만든 스레드에서만 써야 하므로 채널로 값만
// 넘긴다), 밀린 값이 쌓이면 가장 최신 값만 적용해서 항상 "지금" 값으로 바로 수렴한다.
static VOLUME_TX: OnceLock<Sender<f32>> = OnceLock::new();

fn volume_worker() -> &'static Sender<f32> {
    VOLUME_TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<f32>();
        std::thread::spawn(move || {
            let endpoint = endpoint_volume();
            let ep = match endpoint {
                Ok(ep) => ep,
                Err(_) => return,
            };
            while let Ok(mut level) = rx.recv() {
                // 처리하는 사이 더 쌓인 값이 있으면 가장 최근 것만 남기고 건너뛴다.
                while let Ok(newer) = rx.try_recv() {
                    level = newer;
                }
                unsafe {
                    let _ = ep.SetMasterVolumeLevelScalar(level.clamp(0.0, 1.0), std::ptr::null());
                    // Windows는 음소거를 볼륨 값과 별개 플래그로 관리한다 - 다이얼을
                    // 움직이는 건 "지금 소리 나게 하고 싶다"는 뜻이니 매번 같이 풀어준다.
                    let _ = ep.SetMute(false, std::ptr::null());
                }
            }
        });
        tx
    })
}

pub fn set_volume_raw(level: f32) {
    let _ = volume_worker().send(level);
}

#[tauri::command]
pub fn get_volume() -> f32 {
    get_volume_raw()
}

#[tauri::command]
pub async fn set_volume(state: State<'_, Arc<AppState>>, level: f32) -> Result<(), String> {
    set_volume_raw(level);
    let layout = state.layout_json().await;
    state.broadcast_layout(layout);
    Ok(())
}
