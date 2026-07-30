# AutoDeck 출시 준비 현황 (2026-07-30 세션)

이 프로젝트에 대한 작업 현황, 출시 전 사용자가 직접 확인/처리해야 할 것들, 앞으로 업데이트하면 좋을 것들을 정리한 문서.

## 완료한 것

### 보안
- **페어링 핸드셰이크**: 첫 연결 시 PC가 승인 팝업(허용/거부)을 띄우고, 승인하면 토큰을 발급해 재연결 시엔 다시 안 물어봄. PROTOCOL.md에 `hello`/`hello-ack`/`hello-deny` 문서화. `paired_devices.json`에 원자적 쓰기로 영속화.
- **PC 설정 화면에 페어링된 기기 관리 UI 추가**: 승인한 기기 목록(이름, 페어링한 날짜)을 보고 개별로 "Unpair"(연결 해제) 가능. `list_paired_devices`/`unpair_device` 명령 추가.
- **폰↔PC는 1:1 연결만 지원**: 처음엔 여러 PC가 보이면 고르는 UI를 만들었었는데, 사용자 판단으로 오히려 혼란스럽다고 판단해 되돌림 — 폰 하나는 항상 PC 하나에만 연결(맨 처음 찾은 PC를 기억해두고, 그 이후엔 다른 PC가 보여도 무시).

### 자동 업데이트
- `tauri-plugin-updater` + `tauri-plugin-process` 적용. 앱 시작 시 자동으로 최신 버전을 확인하고, 있으면 화면 위쪽에 "Update vX.X.X available — Install & Restart" 배너가 뜸. 클릭하면 다운로드→설치→재시작까지 자동.
- 업데이트 확인 주소(`tauri.conf.json`의 `plugins.updater.endpoints`): `https://github.com/33bnm3-sudo/autodeck/releases/latest/download/latest.json` — **GitHub 저장소가 만들어지고 릴리즈가 올라가야 실제로 동작함**(그 전까진 조용히 실패하고 배너 안 뜸, 앱엔 문제 없음).
- 업데이트 서명키를 생성해뒀음(`C:\Users\33bnm\.tauri\autodeck-updater.key`, 비밀번호 없음) — **이 파일 잃어버리면 이후 버전에 서명 못 해서 자동업데이트가 완전히 끊김. 백업 필수.** 공개키는 이미 `tauri.conf.json`에 박혀있음.
- 새 버전 낼 때마다: `tauri build`가 만드는 `.sig` 파일들을 이용해 `latest.json` 매니페스트를 만들어 릴리즈에 같이 올려야 함(지금은 수동, 나중에 GitHub Actions로 자동화 가능 — 아래 "앞으로 하면 좋을 것" 참고).

### 로고/브랜딩
- 검정 배경 + 주황 글로우, 가운데는 검정 원 + 주황 테두리 링(앱 안의 시계 디자인과 통일감) 로고로 최종 확정. PC(.ico/.icns)·안드로이드(adaptive icon) 아이콘 세트 전부 적용.

### 안드로이드
- 아이콘 동기화 시 fade+scale-in 애니메이션.
- 연결 상태 메시지 세분화: "Searching for PC...", "No PC found on this network", "Waiting for approval on PC…", "Connection denied on PC", "Approval timed out", "Lost connection to `<이름>`. Retrying…"
- 배터리 최적화 예외 요청(기존 구현 확인).

### PC
- 메모장/계산기 트레이, 폴더/파일 열기 속도 개선, 중복 실행 방지, 이미 실행 중인 앱 최전면 이동, 드래그앤드랍 저장 원자적 쓰기.
- 프로덕션 빌드 성공: 포터블 exe(설치 없이 바로 실행, `agent/dist/`)를 기본 배포 방식으로 채택. MSI + NSIS 인스톨러도 x64/ARM64 둘 다 만들어짐(원하는 사람용, 방화벽 규칙 자동 추가 NSIS 훅 `windows/installer.nsh` 포함).

## ⚠️ 사용자가 직접 해야 하는 것

### 1. 방화벽 규칙 정리 (이 PC에서 테스트하면서 생긴 것, 한 번만)
내가 이 PC에서 자동화로 반복 테스트하다가 release exe 경로에 Windows가 실수로 **차단** 규칙을 만들어버렸어(진짜 사용자는 이런 일 안 생김 — 정상적으로 실행하면 Windows가 알아서 허용/차단을 물어보는 팝업을 띄워줌). 이 PC에서만 정리하면 됨, 관리자 권한 PowerShell에서 한 번:
```powershell
Remove-NetFirewallRule -DisplayName "autodeck" -ErrorAction SilentlyContinue
```

### 2. 배포 방식을 포터블 exe 위주로 바꿈
설치 마법사(UAC 필요)가 낯선 사람한텐 겁먹기 쉽다고 판단해서, **그냥 다운받아 바로 실행하는 포터블 exe**를 기본 배포 방식으로 바꿨어 — `agent/dist/AutoDeck-x64.exe` / `AutoDeck-arm64.exe`(설치 절차 없음, 관리자 권한도 필요 없음). 방화벽은 Windows 자체의 표준 "이 앱의 네트워크 통신을 허용하시겠습니까?" 팝업이 첫 실행 때 자동으로 뜨는데(이건 어떤 정상적인 로컬 네트워크 앱이든 다 뜨는 흔한 절차라 오히려 신뢰감 있음), 여기서 "액세스 허용" 누르면 끝 — 방화벽 규칙을 몰래 심는 설치 스크립트보다 이 편이 더 투명하고 안전해 보임.

기존 NSIS/MSI 인스톨러도 그대로 만들어지긴 함(Start Menu 바로가기·제거 프로그램 원하는 사람용, 방화벽 규칙도 설치 시 자동 추가됨) — 필요하면 아래 위치에 있음:
- x64: `D:\.cargo-target\x86_64-pc-windows-msvc\release\bundle\nsis\autodeck_0.1.0_x64-setup.exe`
- ARM64: `D:\.cargo-target\release\bundle\nsis\autodeck_0.1.0_arm64-setup.exe`

**주의**: 포터블 exe든 인스톨러든, 코드서명이 없는 이상 SmartScreen "알 수 없는 게시자" 경고는 똑같이 뜸(패키징 방식이 아니라 서명 여부 문제라서) — 아래 3번 참고.
**아직 못 한 것**: 아주 오래된 Windows 10(WebView2 런타임이 없는 경우)에서 포터블 exe가 빈 화면으로 뜨는지는 확인 못 함 — 인스톨러는 WebView2를 자동 설치해주지만 포터블은 그 단계가 없음. Windows 11과 최신 Windows 10은 기본 내장이라 대부분은 문제없음.

### 3. 코드 서명 (구매는 내가 못 함 — 결제라서)
서명 안 하면 설치 시 "알 수 없는 게시자" SmartScreen 경고가 뜸(막히진 않고 "추가 정보 → 실행"으로 넘어갈 수 있지만 낯선 사람 상대로는 이탈률 커짐).

옵션:
- **OV(Organization Validation) 인증서**: 연 10~20만원대. 첫 배포 땐 그래도 경고가 뜰 수 있고, 다운로드 수가 쌓이면서 SmartScreen 평판이 서서히 좋아짐.
- **EV(Extended Validation) 인증서**: 연 40~60만원대. 즉시 신뢰 획득하지만 비싸고, 신원확인(사업자 등록증 등)이 더 까다로울 수 있음.
- 무료 대안 없음(TLS 인증서와 달리 코드서명은 Let's Encrypt 같은 무료 CA가 없음).

인증서를 사면(SectigoDigiCert 등에서 구매 가능) `.pfx` 파일을 받게 되는데, 그걸 `tauri.conf.json`의 `bundle.windows.certificateThumbprint`에 지문을 넣거나 `signtool` 경로를 지정하면 빌드 시 자동 서명됨 — 이 설정은 인증서 생기면 내가 바로 넣어줄 수 있음.

### 4. GitHub 저장소 (진행 중 — 아래 참고)

## 이번에 실제로 발견해서 고친 버그들

1. **페어링 승인 대기 중 keepalive 응답 안 함** — 대기 중 소켓을 안 읽어서 폰의 WS ping에 응답 못 함 → OkHttp가 15초 뒤 죽었다 판단, 재연결 반복, 승인 요청 폭증(실측 36개). 대기 중에도 소켓 계속 읽도록 수정.
2. **타임아웃/거부된 요청이 큐에서 안 지워짐** — 죽은 요청이 큐에 유령처럼 쌓여 진짜 요청이 뒤로 밀림. `pair-resolved` 이벤트로 해결.
3. **BambuStudio 같은 앱 아이콘이 작게 한구석에** — 256px 아이콘 없는 exe는 작은 아이콘을 확대 없이 캔버스 구석에 그대로 박음(실측: 256x256 캔버스, 내용물 48x48이 (0,0)에). 바운딩박스 검출→크롭→확대→중앙배치로 수정.
4. **우클릭으로 지울 때 동시에 왼클릭도 발동** — pointerdown에서 버튼 종류 먼저 확인하도록 수정.
5. **볼륨 100/0 근처에서 갑자기 튐** — 클램프 재계산 부호 오류.
6. **이미 켜진 프로그램이 뒤에 깔림** — AttachThreadInput 방식 + 실행파일명 기반 창 탐색 fallback.
7. **드래그앤드랍 배치가 가끔 초기화되는 것처럼 보임(추정)** — `buttons.json` 저장이 원자적이지 않아 중간에 죽으면 파싱 실패→초기화. 임시파일+rename으로 수정.
8. **앱이 2개 이상 뜨면 상태가 꼬임** — `tauri-plugin-single-instance` 추가.

## 앞으로 업데이트하면 좋을 것들

- **릴리즈 자동화(GitHub Actions)**: 태그 찍으면 자동으로 x64/ARM64 빌드 + `latest.json` 생성 + 릴리즈 업로드까지 되도록. 지금은 수동.
- **동시에 여러 페어링 요청이 뜰 때 UI가 투박함**: 큐에 여러 개 쌓이면 순서대로 하나씩만 보여줌 — "외 N건 대기 중" 카운트 표시하면 더 좋음.
- **회전 미세 끊김**: 낮은 우선순위, 근본 원인 못 찾음(물리 루프 자체엔 문제 없어 보임, OS/webview 컴포지터 레벨 가능성).
- **안드로이드는 실기기 1대(갤럭시 A36)로만 검증**: 배터리 최적화 예외는 표준 API라 대부분 커버되지만 제조사별(특히 삼성) 추가 정책까진 다 못 커버할 수 있음.

## 개발 환경 메모 (다음에 빌드할 때 참고)

- 이 PC는 ARM64라서 만든 설치파일 중 ARM64용은 이 PC 전용, 대부분의 윈도우 사용자(x64)에겐 x64용을 배포해야 함.
- `tauri-plugin-updater` 추가한 뒤로 `cargo build`가 `ring` 크레이트에서 clang을 못 찾아 실패할 수 있음 — PATH에 아래 두 경로를 추가하면 해결됨(Visual Studio Build Tools의 LLVM):
  ```
  C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\VC\Tools\Llvm\ARM64\bin
  C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\VC\Tools\Llvm\x64\bin
  ```
  (Community 버전 LLVM 경로엔 `clang.exe`가 없고 BuildTools 쪽에만 있음 — Community의 `clang-tidy.exe` 등과 헷갈리지 않게 주의.)
- PC exe를 프로젝트 밖 새 경로에서 실행하면 Windows 방화벽이 그 경로별로 새 규칙을 만듦 — 관리자 권한 없는 셸에선 규칙 추가/삭제 둘 다 안 됨.
