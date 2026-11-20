; Qbird NSIS hooks.
;
; The installer marks the install as self-contained (qbird.portable next to the
; executable), so the app keeps identity, database, attachments, plugins and
; stickers in <install dir>\data — the chosen install path is also the data path.
;
; Uninstall rules:
; - Updates and silent installs never touch user data.
; - Interactive uninstall asks whether to back the data up to 「文档\Qbird 数据」.
; - Then it asks between a complete uninstall (delete every local data file) and
;   keeping user data next to the removed program so a reinstall can resume.

Var QbirdRemoveData

!macro NSIS_HOOK_POSTINSTALL
  CreateDirectory "$INSTDIR\data"
  FileOpen $0 "$INSTDIR\qbird.portable" w
  FileWrite $0 "Qbird stores local data in the data folder next to this file."
  FileClose $0
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  StrCpy $QbirdRemoveData 0

  ; An update replaces the program, it must never drop the data folder.
  ${If} $UpdateMode = 1
    Goto qbird_uninstall_done
  ${EndIf}
  ; Silent/passive uninstalls cannot ask, so they keep user data.
  ${If} $PassiveMode = 1
    Goto qbird_uninstall_done
  ${EndIf}
  ${If} ${Silent}
    Goto qbird_uninstall_done
  ${EndIf}

  ; Never delete files the app still holds open.
  !insertmacro CheckIfAppIsRunning "$INSTDIR\${MAINBINARYNAME}.exe" "${PRODUCTNAME}"

  ; Is there any local data next to the program?
  StrCpy $0 0
  FindFirst $1 $2 "$INSTDIR\data\*.*"
  ${If} $2 != ""
    StrCpy $0 1
  ${EndIf}
  FindClose $1
  ${If} $0 = 0
    Goto qbird_uninstall_done
  ${EndIf}

  ; 1) Offer a backup before anything is deleted. Cancel aborts the uninstall.
  MessageBox MB_YESNOCANCEL|MB_ICONQUESTION \
    "卸载前是否把本地聊天数据备份到「文档\Qbird 数据」？$\r$\n$\r$\n是：立即备份（推荐）$\r$\n否：不备份，继续卸载$\r$\n取消：中止卸载" \
    /SD IDYES IDYES qbird_uninstall_backup IDNO qbird_uninstall_scope
  Abort

  qbird_uninstall_backup:
    CreateDirectory "$DOCUMENTS\Qbird 数据"
    nsExec::ExecToLog '"$SYSDIR\robocopy.exe" "$INSTDIR\data" "$DOCUMENTS\Qbird 数据" /E /R:2 /W:2 /NFL /NDL /NJH /NJS'
    Pop $0
    ${If} $0 < 8
      MessageBox MB_OK|MB_ICONINFORMATION "聊天数据已备份到：$\r$\n$DOCUMENTS\Qbird 数据"
      Goto qbird_uninstall_scope
    ${EndIf}
    MessageBox MB_YESNO|MB_ICONEXCLAMATION \
      "备份到「文档\Qbird 数据」失败（robocopy 返回 $0）。$\r$\n$\r$\n是：中止卸载，先启动 Qbird 手动导出数据$\r$\n否：忽略备份，继续卸载" \
      /SD IDNO IDYES qbird_uninstall_abort
    Goto qbird_uninstall_scope

  qbird_uninstall_abort:
    Abort

  ; 2) Complete uninstall vs keep user data. Keeping is the safe default.
  qbird_uninstall_scope:
    MessageBox MB_YESNO|MB_ICONQUESTION|MB_DEFBUTTON2 \
      "是否彻底卸载 Qbird？$\r$\n$\r$\n是：完全卸载，删除联系人、消息、设置和已接收的文件$\r$\n否：保留用户信息，只删除程序，重新安装到同一目录后可继续使用" \
      /SD IDNO IDYES qbird_uninstall_purge
    Goto qbird_uninstall_done

  qbird_uninstall_purge:
    StrCpy $QbirdRemoveData 1
    RmDir /r "$INSTDIR\data"
    ; The uninstaller's own "Delete the application data" checkbox covers the
    ; legacy %APPDATA%/%LOCALAPPDATA% folders, so force it to match this choice.
    StrCpy $DeleteAppDataCheckboxState 1
    ; Report files that survived the purge instead of leaving them silently.
    StrCpy $0 0
    FindFirst $1 $2 "$INSTDIR\data\*.*"
    ${If} $2 != ""
      StrCpy $0 1
    ${EndIf}
    FindClose $1
    ${If} $0 = 1
      MessageBox MB_OK|MB_ICONEXCLAMATION "部分数据文件未能删除，可能仍被 Qbird 占用。$\r$\n请关闭 Qbird 后手动删除：$\r$\n$INSTDIR\data"
    ${EndIf}
    Goto qbird_uninstall_done

  qbird_uninstall_done:
    ; Keeping user data must also keep the legacy per-user folders it may use.
    ${If} $QbirdRemoveData = 0
      StrCpy $DeleteAppDataCheckboxState 0
    ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; After a complete uninstall the data purge leaves an empty install folder
  ; behind, because the template's RMDir ran while the data was still there.
  ${If} $QbirdRemoveData = 1
    RMDir "$INSTDIR"
  ${EndIf}
!macroend
