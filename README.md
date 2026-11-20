# 易搜 YiSou

易搜是一个轻量的 Windows 11 桌面搜索工具，支持全局快捷键、划词搜索、剪贴板搜索、自定义搜索引擎和浏览器。应用使用 Rust、Win32、Direct2D 和 DirectWrite。

## 构建与测试

安装 Rust stable 的 x64 MSVC 工具链、Visual Studio Build Tools 的 MSVC 链接器和 Windows SDK。在仓库根目录运行以下命令。

```powershell
.\tools\build-rust-editions.ps1
.\build\editions\full\YiSou.exe
```

Full 版提供截图 OCR。模型由用户在设置页指定，下载入口也在同一页。Lite 版位于 `build/editions/lite/YiSou.exe`，包含核心搜索功能。两版共用 JSON 配置，默认目录为 `%LOCALAPPDATA%\YiSouRust`。

退出运行中的易搜后，可以在 Windows 桌面会话验证两个版本。

```powershell
cargo test --locked --workspace --no-default-features
cargo test --locked --workspace --features ocr
```

## 许可证

易搜原创代码采用 MIT 许可证。第三方图标的来源和条款见 [素材说明](assets/THIRD_PARTY.md)。[MIT 许可证](LICENSE)
