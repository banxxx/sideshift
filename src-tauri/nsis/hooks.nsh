; 卸载钩子：由 tauri.conf.json 的 bundle.windows.nsis.installerHooks 指进来。
;
; 为什么挂 POSTUNINSTALL 而不是 PREUNINSTALL：模板把 PRE 宏插在 `CheckIfAppIsRunning` **之前**
; （生成脚本 :748 vs :751），那一刻应用很可能还开着，`appdata\webview\EBWebView` 整棵都被
; WebView2 锁着，删了等于没删。POST 在所有删除动作之后，那时进程早退了。
;
; 为什么必须判 $UpdateMode：升级走的是「新安装包调起老卸载器并传 /UPDATE」，
; 少了这道闸门就是每次升级把用户的设置清一遍。
;
; 为什么绝不写 `RMDir /r "$INSTDIR"`：安装目录和数据根可以是同一个目录（用户把它选成
; `E:\SideShift`），那句话会连 output\ 里他转好的整合包一起带走。所以只删我们认识的那一个子目录。

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    SetShellVarContext current

    ; 卸载壳自己。它是安装那一刻由安装壳投进来的、注册表 UninstallString 指的就是它，
    ; 而 NSIS 的删除清单里没有它（它不是 ${FILE} 装进去的）——不删就是卸载完留在安装目录里
    ; 一个打不开的 exe，且 `RMDir "$INSTDIR"` 永远失败。
    ; 正常路径下它此刻已经不在原地：壳跑卸载前会把自己复制到 %TEMP%，所以这句删得动。
    ; 名字和 `data_root::UNINSTALL_SHELL_NAME` 是同一个，改那里要改这里（钩子编不进 Rust 常量）
    Delete "$INSTDIR\SideShift-Uninstall.exe"

    ; 这一版的应用数据：设置 / 任务存档 / 鸣谢快照与皮肤副本 / WebView2 的 profile
    ; （那几百 MB 里 99% 是一次性缓存，跟着安装目录走才有人清得动）
    RMDir /r /REBOOTOK "$INSTDIR\appdata"

    ; 老布局：identifier 命名的那两个目录。模板自己只在勾了「Delete app data」时才删它们，
    ; 这里不看那个勾选框——同一份数据留一半清一半，比留两份还难查
    RMDir /r "$APPDATA\${BUNDLEID}"
    RMDir /r "$LOCALAPPDATA\${BUNDLEID}"

    ; 清完才轮得到这句：模板自己那句 `RMDir "$INSTDIR"` 跑得太早，那时 appdata 还在里面
    RMDir "$INSTDIR"
  ${EndIf}
!macroend
