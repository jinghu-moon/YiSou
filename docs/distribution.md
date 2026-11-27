# 易搜 YiSou · Windows 11 x64

- **Lite** 提供全局呼出、划词搜索、剪贴板搜索、搜索历史、自定义搜索引擎和浏览器，以及截图搜图。
- **Full** 具备 Lite 的功能，并提供截图 OCR。OCR 默认关闭，使用前需配置本地模型。

## 启动

1. 从发布页下载 Lite 或 Full 的 EXE，放在固定目录并直接运行。
2. 默认双击左 Alt 呼出搜索框。右键点按任务栏通知区域的易搜图标，选择「设置」可更改快捷键、搜索引擎和浏览器。
3. 如果 Windows 提示缺少 MSVC 运行库，请安装微软提供的最新 x64 版，再启动易搜。[Microsoft Visual C++ x64 Redistributable](https://aka.ms/vc14/vc_redist.x64.exe)

发布页附有 `SHA256SUMS.txt`。可用 PowerShell 的 `Get-FileHash` 计算下载文件的 SHA-256，并与对应文件名的校验值比对。

## 截图搜图

从托盘菜单选择「截图搜图」，或先在快捷键设置中绑定按键。框选后按 Enter 执行预设动作，也可以点击截图工具栏的搜索按钮临时选择 Yandex Images、Google Lens 或同时使用两者。确认后，所选区域的图片会发送给所选搜索服务，结果在浏览器中打开。

## 截图 OCR

在 Full 的「设置 → OCR」中打开 ModelScope 下载入口，将检测模型、识别模型和字典放在同一目录。选择该目录并通过验证后启用 OCR。默认截图快捷键为 Alt + Shift + O，识别在本机完成。

## 升级与移除

从托盘退出易搜后，运行新版 EXE。两版默认共用 `%LOCALAPPDATA%\YiSouRust` 中的 JSON 配置和历史记录。迁移或手动清理前，可在设置中导出配置。如果开启了随 Windows 启动且更换了 EXE 的位置或名称，请在新版本的设置中重新开启该选项以更新路径。

移除前先在设置中关闭随 Windows 启动，再从托盘退出并删除 EXE。如果也要移除用户配置和历史记录，可删除 `%LOCALAPPDATA%\YiSouRust`。

## 许可证

易搜源码采用 MIT 许可证。程序及第三方依赖的许可证和声明嵌入 EXE 的 `YISOU_LICENSES` 资源。[项目源码](https://github.com/jinghu-moon/YiSou)
