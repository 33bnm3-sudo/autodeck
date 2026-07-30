; 설치 직후 방화벽에 인바운드 규칙을 추가한다 - 안 그러면 대부분의 사용자가
; 첫 실행 때 Windows Defender 방화벽에 막혀 폰에서 연결이 안 되는데, 그걸
; 스스로 해결하는 사람은 거의 없어서 "연결이 안 돼요" 문의의 제일 큰 원인이 된다.
!macro NSIS_HOOK_POSTINSTALL
  nsExec::ExecToLog 'netsh advfirewall firewall add rule name="AutoDeck" dir=in action=allow protocol=TCP localport=9999 program="$INSTDIR\autodeck.exe" enable=yes'
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::ExecToLog 'netsh advfirewall firewall delete rule name="AutoDeck"'
!macroend
