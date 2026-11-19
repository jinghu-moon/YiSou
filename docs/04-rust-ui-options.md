# 易搜 Rust UI 选型

同一套 Rust 搜索、配置、历史、快捷键及图标接口供三种 UI 方案共用。界面依据 [UI 设计语言](03-ui-design-language.md) 实现，并优先验证最常用的路径，从托盘或快捷键唤醒浮窗，输入中文，切换引擎，交给浏览器，再回到托盘。

| 方案 | 窗口和输入 | 视觉与动效 | 成本与待验证项 |
| --- | --- | --- | --- |
| A · Win32 + Direct2D + DirectWrite + DirectComposition | 原生窗口、系统文本控件及 IME。直接管理浮窗激活、锚点、托盘、可访问性和 DPI。 | 用 Direct2D 绘制浅色面板和品牌图标，由合成层处理短淡入和位移，可复用原有视觉规格。 | Windows 单平台，系统组件提供图形栈。需要自行管理布局、交互状态和绘制资源。重点测首帧、显隐后资源释放及设置页维护量。 |
| B · GPUI Kit | 通过 GPUI 管理窗口、输入与焦点，复用设置页控件。全局快捷键和托盘继续走 Win32。 | GPU 绘制与组件样式统一，适合重做设置、引擎网格和滚动交互。 | 需要实测 Windows 下的 IME、焦点回收、混合 DPI、Narrator 和长驻占用。控制依赖功能并统计 Release 体积。 |
| C · Slint + winit | 声明式界面与 Rust 状态绑定，使用 winit 窗口后端，托盘及热键仍使用 Win32。 | 布局、状态及过渡由 Slint 描述，零散动效收敛到一套规则。 | 需要实测候选框定位、浮窗激活及菜单贴边，固定渲染后端后测体积和首帧。分发前核对所选许可和披露要求。 |

A 适合优先控制易搜的秒开浮窗、Windows 焦点和常驻资源。B 适合希望设置和交互组件能更快迭代的路线。C 适合优先选择声明式布局及过渡的路线。三个方案的速度、内存和显示效果在原型中实测。

## 选定后的首轮原型

第一轮实现搜索框与引擎菜单，再接七页设置。原型复用同一组品牌 SVG 和目标物理尺寸，采用系统动画设置，默认仅对显隐与菜单展开使用 100–150 ms 的轻动效。单击、按键和 IME 在过渡结束前即可响应。

| 检查 | 统一测法 |
| --- | --- |
| 启动与响应 | 同机同配置记录冷启动 ready、热唤醒首帧及可输入时间、菜单首帧与完整绘制时间。 |
| 常驻与资源 | 记录 Release EXE、发布目录、空闲工作集、私有字节和 CPU。反复显示、隐藏并检查资源数量。 |
| 输入与定位 | 从托盘和全局快捷键进入，验证中文候选、方向键、Tab、Esc、鼠标点击与焦点返回。实测 100% 与 150%/200% 跨屏。 |
| 视觉和动效 | 用同一配置并排截图，观察品牌图标边缘、窗体落点、文本可读性和光标。开启及关闭 Windows 动画分别采样。 |

[DirectComposition](https://learn.microsoft.com/windows/win32/directcomp/directcomposition-portal) · [GPUI](https://github.com/zed-industries/zed/blob/main/crates/gpui/README.md) · [Slint winit 后端](https://docs.slint.dev/latest/docs/slint/guide/backends-and-renderers/backend_winit/) · [Slint 许可](https://github.com/slint-ui/slint/blob/master/LICENSE.md)
