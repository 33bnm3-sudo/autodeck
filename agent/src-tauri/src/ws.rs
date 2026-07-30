use crate::state::{AppState, LaunchKind};
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;

pub const PORT: u16 = 9999;

pub async fn run(state: Arc<AppState>, app: AppHandle) {
    let listener = match TcpListener::bind(("0.0.0.0", PORT)).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("autodeck: failed to bind port {PORT}: {e}");
            return;
        }
    };
    loop {
        let (stream, _addr) = match listener.accept().await {
            Ok(pair) => pair,
            Err(_) => continue,
        };
        // Nagle 알고리즘을 꺼서 press 같은 작은 메시지가 지연 없이 바로 나가게 한다.
        let _ = stream.set_nodelay(true);
        let state = state.clone();
        let app = app.clone();
        tauri::async_runtime::spawn(handle_connection(stream, state, app));
    }
}

async fn handle_connection(stream: TcpStream, state: Arc<AppState>, app: AppHandle) {
    let ws_stream = match tokio_tungstenite::accept_async(stream).await {
        Ok(ws) => ws,
        Err(_) => return,
    };
    let (mut write, mut read) = ws_stream.split();

    // 페어링 핸드셰이크: hello를 받고 승인되기 전엔 layout을 포함해 아무것도
    // 내보내지 않는다 - 같은 네트워크의 아무 기기나 곧바로 프로그램을 실행시킬
    // 수 있던 구멍을 막기 위함.
    let hello_text = match read.next().await {
        Some(Ok(Message::Text(text))) => text,
        _ => return,
    };
    let hello: serde_json::Value = match serde_json::from_str::<serde_json::Value>(hello_text.as_str()) {
        Ok(v) if v.get("type").and_then(|t| t.as_str()) == Some("hello") => v,
        _ => return,
    };
    let device_id = match hello.get("device_id").and_then(|d| d.as_str()) {
        Some(id) if !id.is_empty() => id.to_string(),
        _ => return,
    };
    let device_name = hello
        .get("device")
        .and_then(|d| d.as_str())
        .unwrap_or("Unknown device")
        .to_string();
    let provided_token = hello.get("token").and_then(|t| t.as_str()).map(str::to_string);

    let already_known = state.pairing.matching_token(&device_id).await;
    let token = if already_known.is_some() && already_known.as_deref() == provided_token.as_deref() {
        already_known.unwrap()
    } else {
        // 모르는 기기거나 토큰이 안 맞음 - 사용자 승인을 기다린다. 그동안에도
        // read를 계속 폴링해야 클라이언트가 보내는 WS ping에 응답이 나가서
        // 연결이 안 죽는다(안 그러면 OkHttp가 15초 뒤 죽었다고 판단해 재연결을
        // 반복하며 승인 대기 항목이 계속 쌓이는 버그가 있었다).
        let (request_id, mut approval_rx) =
            state.pairing.register_pending(&app, &device_name).await;
        let deadline = tokio::time::sleep(std::time::Duration::from_secs(
            crate::pairing::APPROVAL_TIMEOUT_SECS,
        ));
        tokio::pin!(deadline);

        let approved = loop {
            tokio::select! {
                result = &mut approval_rx => {
                    break result.unwrap_or(false);
                }
                _ = &mut deadline => {
                    break false;
                }
                msg = read.next() => {
                    match msg {
                        Some(Ok(_)) => continue,
                        _ => break false,
                    }
                }
            }
        };
        state.pairing.unregister_pending(&request_id).await;
        // 사용자가 누르지 않고 타임아웃/연결끊김으로 끝난 경우에도 프런트엔드
        // 승인 대기열에서 이 항목을 지워야 한다 - 안 그러면 이미 죽은 연결의
        // 요청이 큐에 유령처럼 계속 쌓여서, 실제로 살아있는 요청은 뒤로 밀리고
        // 사용자가 몇 번을 눌러도 큐 앞쪽의 죽은 항목만 소모하는 것처럼 보인다.
        let _ = app.emit("pair-resolved", &request_id);

        if !approved {
            let deny = serde_json::json!({ "type": "hello-deny", "reason": "denied" }).to_string();
            let _ = write.send(Message::text(deny)).await;
            return;
        }

        let new_token = crate::pairing::generate_token();
        state.pairing.upsert(&device_id, &device_name, &new_token).await;
        new_token
    };

    let ack = serde_json::json!({ "type": "hello-ack", "token": token }).to_string();
    if write.send(Message::text(ack)).await.is_err() {
        return;
    }

    let mut layout_rx = state.layout_tx.subscribe();

    let initial = state.layout_json().await;
    if write.send(Message::text(initial)).await.is_err() {
        return;
    }

    loop {
        tokio::select! {
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(value) = serde_json::from_str::<serde_json::Value>(text.as_str()) {
                            match value.get("type").and_then(|t| t.as_str()) {
                                Some("press") => {
                                    if let Some(id) = value.get("id").and_then(|i| i.as_str()) {
                                        launch_for(&state, &app, id).await;
                                    }
                                }
                                Some("volume") => {
                                    if let Some(level) = value.get("level").and_then(|l| l.as_f64()) {
                                        crate::mediactl::set_volume_raw(level as f32);
                                        let layout = state.layout_json().await;
                                        // PC's own window isn't a WS client of itself, so
                                        // broadcast_layout alone never reaches it - a phone
                                        // changing volume would otherwise leave the PC dial's
                                        // local `volume` stale until it starts its own drag.
                                        let _ = app.emit("remote-layout", &layout);
                                        state.broadcast_layout(layout);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(_)) => break,
                    _ => {}
                }
            }
            layout = layout_rx.recv() => {
                if let Ok(json) = layout {
                    if write.send(Message::text(json)).await.is_err() {
                        break;
                    }
                }
            }
        }
    }
}

pub async fn launch_for(state: &Arc<AppState>, app: &AppHandle, id: &str) {
    if let Some((path, kind)) = state.target_for(id).await {
        let launched = match kind {
            // exe는 프로세스로 직접 실행.
            LaunchKind::Exe => match std::process::Command::new(&path).spawn() {
                Ok(child) => {
                    // 이미 떠있는 단일 인스턴스 앱(크롬 등)은 방금 spawn한 프로세스가
                    // 기존 인스턴스에 인자만 넘기고 곧장 죽어버리므로, 실행 파일명으로도
                    // 찾을 수 있게 힌트를 같이 넘긴다.
                    let exe_hint = std::path::Path::new(&path)
                        .file_name()
                        .and_then(|s| s.to_str())
                        .map(|s| s.to_lowercase());
                    crate::winfocus::foreground_after_spawn(child.id(), exe_hint);
                    true
                }
                Err(_) => false,
            },
            // 폴더·문서 등은 셸에 직접 "open"을 요청한다(폴더는 열고, 파일은 연결된
            // 기본 프로그램으로 실행) - explorer.exe를 새로 spawn해서 그게 다시 진짜
            // 탐색기로 IPC 전달하는 중간 단계를 건너뛰어 더 빨리 열린다.
            LaunchKind::Open => crate::winfocus::shell_open(&path),
        };
        if launched {
            let _ = app.emit("launched", id);
        }
    }
}
