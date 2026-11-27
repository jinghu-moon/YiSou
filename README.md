# 易搜 YiSou

易搜是一个轻量的 Windows 11 桌面搜索工具，支持全局快捷键、划词搜索、剪贴板搜索、自定义搜索引擎和浏览器。应用使用 Rust、Win32、Direct2D 和 DirectWrite。

## 下载

从发布页下载 Windows 11 x64 单文件 EXE。Lite 包含文本搜索和截图搜图，Full 另外支持本地截图 OCR。[发布页](https://github.com/jinghu-moon/YiSou/releases) · [使用说明](docs/distribution.md)

## 构建与测试

安装 Rust stable 的 x64 MSVC 工具链、Visual Studio Build Tools 的 MSVC 链接器、Windows SDK 和 PowerShell 7。在仓库根目录运行以下命令。

```powershell
cargo build --locked --features ocr
.\target\debug\YiSou.exe
```

如需同时生成 Full 和 Lite 的 Release 产物，在仓库根目录执行以下命令。

```powershell
.\tools\build-rust-editions.ps1
.\target\editions\full\YiSou.exe
```

Full 版提供截图 OCR。模型由用户在设置页指定，下载入口也在同一页。Lite 版位于 `target/editions/lite/YiSou.exe`，包含核心搜索功能。两版共用 JSON 配置，默认目录为 `%LOCALAPPDATA%\YiSouRust`。

Full 和 Lite 都可从托盘菜单或自定义快捷键打开「截图搜图」。框选后按 Enter 会执行快捷键设置页中配置的动作，截图工具栏中的搜索按钮可以临时选择引擎。「同时搜图」会把同一张截图并行提交给 Yandex Images 和 Google Lens，每个结果返回时都用已配置的浏览器打开。部分失败时，可以从托盘重试失败的引擎。

## 发布包

退出易搜后，用 PowerShell 7 从仓库根目录运行 `./tools/package-rust-release.ps1`。脚本会在 `target/release/<版本>/` 生成两个可直接运行的 Windows x64 EXE，以及 SHA-256 校验值和发布说明。Full 将 ONNX Runtime 静态链接到 EXE。发布构建需要 Rust 工具链的 `rust-docs` 组件以获取标准库许可证，许可证嵌入 EXE。[使用说明](docs/distribution.md) · [0.1.0 发布说明](docs/release-notes/0.1.0.md)

## 回归测试

退出运行中的易搜后，可以在 Windows 桌面会话验证两个版本。

```powershell
cargo test --locked --workspace --no-default-features
cargo test --locked --workspace --features ocr
```

## 许可证

易搜原创代码采用 MIT 许可证。第三方图标的来源和条款见 [素材说明](assets/THIRD_PARTY.md)。[MIT 许可证](LICENSE)
