# 易搜 YiSou

易搜是一个轻量的 Windows 11 桌面搜索工具，支持全局快捷键、划词搜索、剪贴板搜索、自定义搜索引擎和浏览器。界面由 C++、Win32、Direct2D 和 DirectWrite 实现。

## 构建与测试

使用 Visual Studio 2022 的「使用 C++ 的桌面开发」工作负载、Windows SDK 和 CMake 3.20 或以上版本。在仓库根目录运行以下命令。

```powershell
cmake -S . -B build -G "Visual Studio 17 2022" -A x64
cmake --build build --config Release
ctest --test-dir build -C Release --output-on-failure
.\build\Release\YiSou.exe
```

窗口测试默认使用 Windows 系统字体。如果本地已有 HarmonyOS Sans SC Regular，可将其放在 `assets/fonts/HarmonyOS_SansSC_Regular.ttf`，窗口测试会额外验证自定义字体的加载与恢复。

## 许可证

易搜原创代码采用 MIT 许可证。第三方图标的来源和条款见 [素材说明](assets/THIRD_PARTY.md)。[MIT 许可证](LICENSE)
