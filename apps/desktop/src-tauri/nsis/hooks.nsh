; Buildship 的 NSIS 安装/卸载钩子。
;
; 背景：Tauri 模板自带的「删除应用数据」只清 $APPDATA\<bundleId>
; （com.buildship.desktop），而 Buildship 按方案 5.1 把数据放在
; %APPDATA%\Buildship。两者路径不同，导致卸载后仍留一堆数据库与发布包。
;
; 这里在卸载收尾阶段补一次真实数据目录清理：
;   %APPDATA%\Buildship\{buildship.db*,artifacts,backups,logs,work}
;
; 需要保留数据时，请在应用「设置 → 重置与卸载」勾选「先导出备份到桌面」。

!macro NSIS_HOOK_POSTUNINSTALL
  SetShellVarContext current
  RmDir /r "$APPDATA\Buildship"
  RmDir /r "$LOCALAPPDATA\Buildship"
!macroend
