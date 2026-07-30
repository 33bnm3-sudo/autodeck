use windows::core::{BOOL, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM};
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentThreadId, GetProcessId, OpenProcess,
    QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, BringWindowToTop, EnumWindows, GetForegroundWindow, GetWindow,
    GetWindowThreadProcessId, IsIconic, IsWindowVisible, SetForegroundWindow, ShowWindow,
    GW_OWNER, SW_RESTORE, SW_SHOWNORMAL,
};

struct EnumData {
    target_pid: u32,
    found: Option<HWND>,
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let data = &mut *(lparam.0 as *mut EnumData);
    if !IsWindowVisible(hwnd).as_bool() {
        return true.into();
    }
    if GetWindow(hwnd, GW_OWNER).unwrap_or_default() != HWND(std::ptr::null_mut()) {
        return true.into();
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == data.target_pid {
        data.found = Some(hwnd);
        return false.into();
    }
    true.into()
}

fn find_top_window_for_pid(target_pid: u32) -> Option<HWND> {
    let mut data = EnumData {
        target_pid,
        found: None,
    };
    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut data as *mut _ as isize));
    }
    data.found
}

struct ExeEnumData {
    target_name: String,
    found: Option<HWND>,
}

unsafe extern "system" fn enum_proc_exe(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let data = &mut *(lparam.0 as *mut ExeEnumData);
    if !IsWindowVisible(hwnd).as_bool() {
        return true.into();
    }
    if GetWindow(hwnd, GW_OWNER).unwrap_or_default() != HWND(std::ptr::null_mut()) {
        return true.into();
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == 0 {
        return true.into();
    }
    if process_image_name(pid).as_deref() == Some(data.target_name.as_str()) {
        data.found = Some(hwnd);
        return false.into();
    }
    true.into()
}

// pid로 실행 이미지 파일명(소문자)을 알아낸다. 이미 떠있던 앱(예: 크롬)을 다시 누르면
// 우리가 spawn한 프로세스는 기존 인스턴스에 인자만 넘기고 곧바로 죽어버려서, pid 기준
// 검색으로는 그 창을 절대 못 찾는다 - 이럴 땐 실행 파일명으로 이미 떠있는 창을 찾아야 한다.
fn process_image_name(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let name = process_image_name_from_handle(handle);
        let _ = CloseHandle(handle);
        name
    }
}

unsafe fn process_image_name_from_handle(handle: HANDLE) -> Option<String> {
    let mut buf = [0u16; 260];
    let mut len = buf.len() as u32;
    unsafe { QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len) }.ok()?;
    let full = String::from_utf16_lossy(&buf[..len as usize]);
    std::path::Path::new(&full)
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase())
}

fn find_top_window_for_exe_name(exe_name: &str) -> Option<HWND> {
    let mut data = ExeEnumData {
        target_name: exe_name.to_lowercase(),
        found: None,
    };
    unsafe {
        let _ = EnumWindows(Some(enum_proc_exe), LPARAM(&mut data as *mut _ as isize));
    }
    data.found
}

// SetForegroundWindow는 호출한 프로세스가 이미 포그라운드 상태이거나 명시적으로 허용된
// 경우가 아니면 실제로는 안 먹고 그냥 작업표시줄만 깜빡이게 만든다(AllowSetForegroundWindow는
// "대상 프로세스 자신"이 나중에 스스로 호출할 때만 도와줄 뿐, 우리가 남의 창을 강제로
// 앞으로 가져오는 데는 별 소용이 없다). 현재 포그라운드 창의 입력 큐에 우리 스레드를
// 잠깐 붙여서(AttachThreadInput) 그 권한을 빌려쓰는 게 실질적으로 확실하게 먹히는 방법.
unsafe fn force_foreground(hwnd: HWND) {
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }

        let my_tid = GetCurrentThreadId();
        let cur_fg = GetForegroundWindow();
        let mut fg_tid = 0u32;
        if !cur_fg.0.is_null() && cur_fg != hwnd {
            fg_tid = GetWindowThreadProcessId(cur_fg, None);
        }

        let attached = fg_tid != 0 && fg_tid != my_tid && AttachThreadInput(my_tid, fg_tid, true).as_bool();

        let _ = SetForegroundWindow(hwnd);
        let _ = BringWindowToTop(hwnd);

        if attached {
            let _ = AttachThreadInput(my_tid, fg_tid, false);
        }
    }
}

// 새로 실행한(또는 이미 떠있던) 프로그램의 창을 최전면으로 가져온다. AutoDeck 자체가
// 포커스 없는 트레이 앱이라 그냥 spawn만 하면 새 창이 뒤에 깔리는 경우가 있어, 창이
// 나타날 때까지 짧게 폴링한 뒤 강제로 앞으로 가져온다. exe_name_hint가 있으면 pid
// 기준 검색이 실패했을 때(이미 떠있던 단일 인스턴스 앱에 인자만 넘기고 죽는 경우 등)
// 실행 파일명으로도 찾아본다.
pub fn foreground_after_spawn(pid: u32, exe_name_hint: Option<String>) {
    unsafe {
        let _ = AllowSetForegroundWindow(pid);
    }
    std::thread::spawn(move || {
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(75));
            let hwnd = find_top_window_for_pid(pid)
                .or_else(|| exe_name_hint.as_deref().and_then(find_top_window_for_exe_name));
            if let Some(hwnd) = hwnd {
                unsafe {
                    let _ = AllowSetForegroundWindow(pid);
                    force_foreground(hwnd);
                }
                return;
            }
        }
    });
}

// 페어링 승인 요청처럼, 우리 자신의 창을 (트레이에 최소화돼 있어도) 사용자
// 눈에 띄게 앞으로 가져와야 할 때 쓴다. show/set_focus만으로는 포그라운드
// 잠금 때문에 안 먹힐 수 있어서, 항상 먹히는 taskbar 깜빡임(request_user_attention)도
// 같이 걸어 최소한 사용자가 알아챌 수 있게 한다.
pub fn bring_own_window_to_front(app: &tauri::AppHandle) {
    use tauri::Manager;
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        let _ = window.request_user_attention(Some(tauri::UserAttentionType::Critical));
    }
}

// 폴더·문서를 열 때 기존엔 새 explorer.exe 프로세스를 spawn해서 그게 다시 진짜
// 탐색기 인스턴스에 IPC로 요청을 넘기는 식이었다(그 자체가 한 단계 더 걸림).
// ShellExecuteExW로 셸에 직접 "open"을 요청하면 그 중간 프로세스 hop이 없어져
// 체감상 더 빨리 열린다. NOCLOSEPROCESS로 프로세스 핸들을 받아 foreground 로직도
// 그대로 재사용한다.
pub fn shell_open(path: &str) -> bool {
    let wide_path: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let wide_verb: Vec<u16> = "open\0".encode_utf16().collect();
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: PCWSTR(wide_verb.as_ptr()),
        lpFile: PCWSTR(wide_path.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    unsafe {
        if ShellExecuteExW(&mut info).is_err() {
            return false;
        }
        if !info.hProcess.is_invalid() {
            // 핸들을 닫기 전에 실제로 실행된 이미지 이름을 알아둔다(예: 폴더 →
            // explorer.exe, 문서 → 연결된 프로그램) - 경로 문자열만으로는 알 수 없다.
            let exe_hint = process_image_name_from_handle(info.hProcess);
            let pid = GetProcessId(info.hProcess);
            let _ = CloseHandle(info.hProcess);
            if pid != 0 {
                foreground_after_spawn(pid, exe_hint);
            }
        }
    }
    true
}
