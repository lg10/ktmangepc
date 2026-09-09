; 安装/卸载前置钩子：释放内置 adb server 常驻守护
;
; 背景：adb server 启动后为常驻守护进程，长期持有安装目录内
; adb.exe、AdbWinApi.dll、AdbWinUsbApi.dll 句柄；NSIS 覆盖安装/卸载
; 写删这些文件时会弹“无法写入：终止/重试/忽略”对话框。
; 应用侧已在 退出清理 / 升级 install 前 主动 kill-server（见 adbshell.rs），
; 本钩子兜底应用崩溃、任务管理器强杀等未走正常退出路径的残留场景。
;
; 策略：先 kill-server 让守护经 TCP 优雅自退；再 taskkill 强杀兜底
; （server 卡死不应答 kill-server 时）。adb server 按需自动重启，
; 强杀无副作用。

!macro NSIS_HOOK_PREINSTALL
  Call KTKillAdbServer
!macroend

; NSIS 规则：卸载段内 Call 的函数名必须以 un. 开头，
; 故安装/卸载各定义一份入口函数，主体用宏复用
!macro NSIS_HOOK_PREUNINSTALL
  Call un.KTKillAdbServer
!macroend

!macro KTKillAdbServerBody
  ; Windows 下 tauri 资源目录即 exe 同目录（tauri-utils platform::resource_dir），
  ; 映射 "resources/adb": "adb" → $INSTDIR\adb
  IfFileExists "$INSTDIR\adb\adb.exe" 0 +3
    nsExec::ExecToLog '"$INSTDIR\adb\adb.exe" kill-server'
    Pop $0
  ; 兜底强杀残留守护（含 kill-server 无应答的卡死 server）
  nsExec::ExecToLog 'taskkill /F /IM adb.exe /T'
  Pop $0
!macroend

Function KTKillAdbServer
  !insertmacro KTKillAdbServerBody
FunctionEnd

Function un.KTKillAdbServer
  !insertmacro KTKillAdbServerBody
FunctionEnd
