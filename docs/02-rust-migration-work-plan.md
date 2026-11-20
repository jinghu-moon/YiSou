# 易搜 Rust 重构工作文档

## 基线与范围

易搜是一个 Windows 11 桌面搜索工具。它在托盘常驻，通过浮动搜索框、划词或剪贴板取得查询词，再交给用户选定的搜索引擎和浏览器。当前 C++ 版使用 Win32、Direct2D、DirectWrite、DirectComposition、WIC 和 UI Automation。

Rust 重构分支为 `refactor/rust`，从本地 `main` 的 `6e3054a` 分出。该提交包含 resvg 16× 图标优化，是迁移前的 C++ 基线。`d133c49` 用于追溯引入 resvg 前的结果。OCR 属于后续设计，详见 [OCR 搜索设计](01-ocr-search-design.md)。界面实现和截图验收参照 [UI 设计语言](03-ui-design-language.md)。

项目尚未正式发布。Rust 版可以重做内部结构、持久化格式和渲染流程。每个迁移阶段都要先记录 C++ 的运行结果，再用相同输入验证 Rust，并为计划中的行为变化列出新的预期结果。是否需要读取旧 INI、注册表和旧版引擎包，可在确定新文件格式时单独决策。用户在 Rust 版内保存、备份和恢复配置的流程需要完整验收。

## C++ 功能清单

| 编号 | 功能 | 可观察行为与边界 |
| --- | --- | --- |
| F01 | 启动与单实例 | 区分手动启动、`--startup`、`--settings` 和 `--configuration`。`--config` 指定配置路径。验证重复启动对已有实例的行为。`--icon-preview` 打开开发用图标对比窗口。 |
| F02 | 托盘与窗口生命周期 | 托盘菜单可打开搜索框、打开设置和退出。Explorer 重启后恢复托盘图标。可配置手动启动是否弹框、关闭搜索框是否退到托盘、是否随 Windows 登录启动。 |
| F03 | 搜索与打开链接 | `Enter` 提交查询，`Ctrl+Enter` 打开通过验证的 HTTP(S) 链接。合法链接出现提示。空输入时 `↑` 可召回最近一次查询。浏览器启动失败时保留输入与来源，允许重试。成功提交后清空输入。 |
| F04 | 引擎选择与别名 | 搜索框按钮打开引擎网格。可临时切换当前查询的引擎，默认引擎从设置管理。输入开头的 `#alias` 支持提示与补全，ASCII 不区分大小写。已知别名选择对应引擎，未知别名保留在查询正文。`Alt+1…5` 选择置顶顺序的前五项。置顶列表可超过五项。 |
| F05 | 搜索引擎管理 | 可添加、编辑、删除、向上或向下排序、设为默认、置顶或取消置顶、测试搜索。自定义引擎支持名称、别名、URL 模板、颜色、SVG 或位图图标。删除、改名和重排后检查默认、置顶和快捷键引用。 |
| F06 | 全局快捷键 | 默认双击左 Alt，支持改成带修饰键的全局组合键。设置报告冲突，支持双 Alt 的应用排除名单，以及单独绑定划词、剪贴板操作。验证改键失败后原绑定仍可用。 |
| F07 | 划词搜索 | 主快捷键取得选区后搜索，无选区时打开搜索框。专用划词快捷键可以指定引擎，可直接搜索或把选区预填到浮窗。选区读取覆盖原生编辑控件、UI Automation、浏览器文档和 Windows Terminal。组合快捷键路径在符合条件时可模拟复制并恢复原剪贴板。 |
| F08 | 剪贴板搜索 | 专用快捷键读取剪贴板文本并使用默认引擎。文字按原文进入查询。选区和剪贴板中的 `#alias` 要与搜索框主动键入别名分别验收。 |
| F09 | 浏览器 | 支持系统默认浏览器、从已注册浏览器中选择、手动选择浏览器 EXE。搜索和直接打开链接都使用该选择。验证含空格、引号和非 ASCII 字符的启动参数。 |
| F10 | 搜索历史 | 默认关闭，成功搜索后按设置写入。历史页支持按关键词和来源筛选、分页、删除和再次搜索。记录区分键入、选区、历史和剪贴板来源。C++ 版将历史保存在用户注册表。 |
| F11 | 配置管理 | 支持指定并记忆配置路径、EXE 同目录便携 INI、`%LOCALAPPDATA%\YiSou\settings.ini`、首次迁移旧注册表设置。保存时通过临时文件替换，配置内的路径支持相对解析。支持整份配置及仅引擎的导入导出。 |
| F12 | 外观与语言 | 搜索字体可用默认、系统字体或本地字体文件，可预览和恢复。引擎网格可切换显示名称。语言支持跟随系统、中文和英文，修改语言后重启应用。 |
| F13 | 多显示器与输入 | 搜索框按目标显示器放置，可拖动并按显示器记忆位置。窗口采用 Per-Monitor DPI Awareness V2。支持 IME 组合输入、键盘焦点、菜单导航和关闭后的焦点返回。 |

代码入口见 [启动](https://github.com/jinghu-moon/YiSou/blob/d133c492d67e568bf224e4a591596da210b88b6e/src/main.cpp)、[业务模型](https://github.com/jinghu-moon/YiSou/blob/d133c492d67e568bf224e4a591596da210b88b6e/src/core/application.cpp)、[搜索窗口](https://github.com/jinghu-moon/YiSou/blob/d133c492d67e568bf224e4a591596da210b88b6e/src/ui/search_window.cpp)、[设置](https://github.com/jinghu-moon/YiSou/blob/d133c492d67e568bf224e4a591596da210b88b6e/src/ui/settings.cpp)、[快捷键](https://github.com/jinghu-moon/YiSou/blob/d133c492d67e568bf224e4a591596da210b88b6e/src/platform/shortcut.cpp)、[选区](https://github.com/jinghu-moon/YiSou/blob/d133c492d67e568bf224e4a591596da210b88b6e/src/platform/selection.cpp)、[浏览器](https://github.com/jinghu-moon/YiSou/blob/d133c492d67e568bf224e4a591596da210b88b6e/src/platform/browser.cpp)、[配置](https://github.com/jinghu-moon/YiSou/blob/d133c492d67e568bf224e4a591596da210b88b6e/src/platform/config.cpp)、[Windows 服务](https://github.com/jinghu-moon/YiSou/blob/d133c492d67e568bf224e4a591596da210b88b6e/src/platform/windows.cpp)和 [本地化文案](https://github.com/jinghu-moon/YiSou/blob/d133c492d67e568bf224e4a591596da210b88b6e/src/platform/strings.inc)。

### 搜索和数据规则

- 默认引擎为 Bing。内置顺序和别名以 C++ 数据表为准，内置集合包括 Bing `bing`、百度 `bd`、Google `g`、哔哩哔哩 `bili`、YouTube `yt`、知乎 `zh`、Reddit `rd`、GitHub `gh`、Stack Overflow `so`、淘宝 `tb`、Amazon `amz`。
- 搜索模板须使用 HTTP(S) 并含 `%s`。查询按 UTF-8 字节百分号编码。需要覆盖中英文、全角标点、emoji、`%`、`&`、`#`、斜线、换行和首尾空白。别名应覆盖完整匹配、大小写、重复、未知别名、只有别名的输入及临时引擎的优先级。
- C++ 限制为名称 48 个 UTF-16 码元、别名 16 个字符、URL 2048 个 UTF-16 码元、SVG 64 KiB。WIC 导入位图边长上限为 4096 像素，新导入的位图保存为 32×32。Rust 需要明确其长度单位，测试非 BMP 字符和限值前后各一位。
- 全量导入解析并验证后写入活动配置。仅引擎导入按 ID 用导入记录替换同名记录，导入顺序在前，保留其余引擎。置顶按导入顺序在前，追加已有且未重复的项目。验证失败或写入失败时检查活动配置仍可读。涉及配置位置或语言变化的流程，检查重启后实际读取的位置和语言。
- 选区读取上限为 2048 个 UTF-16 码元。双 Alt 路径预算约 350 ms，允许复制回退的路径约 850 ms。验证超时、前台变化、后一次调用取消前一次、选区为空、剪贴板被其他应用更新和恢复失败等情况。过期结果不能启动浏览器。

规则定义见 [数据模型和限制](https://github.com/jinghu-moon/YiSou/blob/d133c492d67e568bf224e4a591596da210b88b6e/src/core/application.h)、[业务校验](https://github.com/jinghu-moon/YiSou/blob/d133c492d67e568bf224e4a591596da210b88b6e/src/core/application.cpp)、[配置读写](https://github.com/jinghu-moon/YiSou/blob/d133c492d67e568bf224e4a591596da210b88b6e/src/platform/config.cpp)和 [选区读取](https://github.com/jinghu-moon/YiSou/blob/d133c492d67e568bf224e4a591596da210b88b6e/src/platform/selection.cpp)。

## Rust 实施顺序

采用分阶段替换。两个版本在迁移期间可独立构建，Rust 使用自己的数据目录。达到全量验收后，在重构分支上切换默认构建，再处理 C++ 工程文件的退役。`main` 上的 `6e3054a` 可重建旧版用于复测和回滚。Release 二进制与资源独立打包，依赖使用当前稳定且符合项目需要的版本，以 `Cargo.lock` 固定解析结果。

### 执行清单

- [x] P0 提交图标优化，通过 Release 构建和四项 CTest，合入本地 `main` 并创建 `refactor/rust`。
- [x] P1 盘点 C++ 的功能、边界和 UI，建立迁移范围、预期和对照组。
- [x] P2 底层实现。独立 Cargo 构建已建立，包含搜索规则、引擎模型、数据存储、图标栅格化及 Windows 系统适配器。系统联测归入 P5。
- [x] P3 消费层实现。搜索、划词和剪贴板意图、历史、配置及窗口命令有自动化覆盖。划词的三层读取已通过 Windows 桌面回归，并经产品快捷键在 Edge 和 Windows Terminal 联测。
- [x] P4a UI 选型材料。提供 [三套 UI 方案](04-rust-ui-options.md)，由用户选择后进入 P4b。
- [ ] P4b UI 实现。接通搜索框、引擎菜单、七页设置、托盘与焦点交互。验证系统注册变更和设置写入的一致性。
- [ ] P5 完成升级前后实机联测、图标画质和性能对照，处理差异并记录验收证据。
- [ ] P6 切换默认构建并验证干净环境的 Release 分发及回滚路径。

| 阶段 | 工作 | 交付与验收 |
| --- | --- | --- |
| 0 | 固定 C++ 基线。保存构建标识、测试结果、性能采样与关键界面截图。准备含多引擎、自定义图标和历史的隔离配置。 | 能从 `d133c49` 重建并重复测试，测试数据与用户日常配置分开。 |
| 1 | 迁移搜索和引擎模型、URL 校验、别名、历史记录模型与配置校验。为浏览器启动、存储和时间建立可替换的边界。 | 同一组输入可对比搜索目标、URL、状态变化和错误。保存失败的测试覆盖内存状态与磁盘状态的一致性。 |
| 2 | 完成 Windows 系统适配器和消费层用例。验证浏览器、快捷键、选区、剪贴板、启动、配置、历史和失败恢复。 | 用例可独立于窗口和渲染调用。测试请求取消、无焦点、空输入、坏配置和系统调用失败。 |
| 3 | 给出原生 Win32、Rust 原生 UI 框架和系统合成控件方向的选项。 | 用户选择 UI 技术后进入界面实现。 |
| 4 | 迁移搜索框、别名提示、引擎网格、七页设置、托盘及本地化。实现图标渲染对照。 | UI 尺寸与状态按设计文档复核。F01–F13 都有 Rust 验收结果。 |
| 5 | 做 Release 构建、资源和依赖审计、安装或便携分发验证。对比性能与截图，更新文档。 | 记录 EXE 和分发大小、延迟、内存、资源数量以及可见差异。验证新功能与既有流程，记录经决定的行为变化。 |

底层的 Win32 系统集成和 SVG 栅格化可以用独立测试验证。消费层负责动作、查询意图、持久化和失败恢复，UI 层负责焦点、输入、布局与动画。选区等耗时工作需要可取消，并在完成后检查请求和前台状态。Win32 句柄、Hook、COM 和图形资源的生命周期由其所属模块管理。

## 升级前后验证

### 自动化与实机用例

| 组别 | 升级前 | Rust 验收 |
| --- | --- | --- |
| 业务 | 跑 `YiSou.Core` 并保留搜索用例 | 将 [搜索用例](https://github.com/jinghu-moon/YiSou/blob/d133c492d67e568bf224e4a591596da210b88b6e/tests/search_cases.h) 用于 Rust，比较 URL、引擎选择、历史和错误结果。新增功能补充正向、边界及失败用例。 |
| Windows 服务 | 跑 `YiSou.Platform`，保存测试配置 | 检查快捷键冲突、配置原子写、完整和仅引擎导入导出、无效路径、浏览器选择、选区取消和剪贴板恢复。 |
| 窗口 | 跑 `YiSou.Windows`，采集常规与失败状态 | 检查启动、托盘、显示和关闭、设置页、引擎切换、链接提示、键盘及 IME。分别验证模拟 DPI 消息和真实跨屏拖动。 |
| 系统实机 | 记录 Explorer、浏览器、终端与编辑器样本，以及 Windows DPI 和输入法 | 在同版本应用上跑划词、剪贴板、Explorer 重启、登录自启动、中文输入和跨屏。记录系统及第三方应用版本。 |
| 图标 | 记录 C++ 提交的 resvg 16× 生产路径及 Direct2D 参考结果 | 使用同一份原始 SVG，覆盖单色细线、填充、多色、透明边缘、规则及不规则曲线。按界面实际绘制的物理尺寸输出，对比已有 Direct2D 路径、resvg 至少 1× 与 16×，以及需要时的中间倍率。 |

图标画质复测记录参考栅格器、版本、各 SVG 尺寸和 viewBox、绘制位置、颜色空间、预乘 Alpha 和缩小滤波。每个目标尺寸从 SVG 独立生成参考图，在同一目标尺寸比较 Alpha 边缘及浅底、深底合成，保存差分图并查看实际界面。16× 的差异分数需要和冷渲染耗时、缓存命中耗时、峰值内存一起判断。小图标验收详见 [UI 设计语言](03-ui-design-language.md)。

### 性能基线与采样

`ctest --test-dir build/svg-before -C Release --output-on-failure` 已通过 `YiSou.Core`、`YiSou.Platform` 和 `YiSou.Windows`。合并前再对 `build/svg-after` 构建 Release 并运行 CTest，四项全部通过，新增项为 `YiSou.SvgFilter`。对应源码见 [业务测试](https://github.com/jinghu-moon/YiSou/blob/d133c492d67e568bf224e4a591596da210b88b6e/tests/core_tests.cpp)、[平台测试](https://github.com/jinghu-moon/YiSou/blob/d133c492d67e568bf224e4a591596da210b88b6e/tests/platform_tests.cpp)和 [窗口测试](https://github.com/jinghu-moon/YiSou/blob/d133c492d67e568bf224e4a591596da210b88b6e/tests/window_tests.cpp)。

| 指标 | 引入 resvg 前 `d133c49` | Rust 迁移前 `6e3054a` |
| --- | ---: | ---: |
| Release `YiSou.exe` | 2,447,360 字节 | 4,347,392 字节 |
| 启动到 ready | 63.385 ms | 59.439 ms |
| 首次打开引擎菜单的完整绘制 | 68.402 ms | 待核对历史记录 |
| 采样过程的峰值工作集 | 36.242 MiB | 36.40 MiB |

性能数字来自同机五轮测量的中位数，使用 `build/svg-timing/before-*` 与 `after-*` 的历史记录。体积只计 EXE。Rust 对比时同时记录 EXE、发布目录和压缩包大小，采用相同架构和 Release 配置，复用同一采样流程。分别记录冷启动、热打开、托盘空闲、开设置、翻历史与反复切换引擎菜单的耗时、工作集、私有字节、CPU 以及 GDI/USER 句柄数。历史记录中缺失的指标需对 C++ 补测，再给出前后差值。

### 当前 Rust 验证记录

从仓库根目录运行 `cargo test --locked --workspace`，Full 版增加 `--features ocr`。测试覆盖 C++ 查询基准、URL 边界、别名与失败重试、配置保存失败与导入导出、选择请求取消、双 Alt 和时间回绕、内置图标和曲线、独立配置路径、Windows 单实例、浏览器枚举及热键冲突。图标由 `yisou_svg` 渲染。配置使用版本化 JSON，默认目录为 `%LOCALAPPDATA%\YiSouRust`。

原生窗口保留现有底层与消费层。[`platform`](../rust/src/platform.rs) 负责 Windows 服务，[应用层](../rust/src/application.rs) 负责搜索、引擎、配置与历史。窗口入口只负责初始化、单实例唤醒和消息循环。搜索窗口、引擎弹窗、七页设置、托盘、划词任务和共享绘制各自放在 [原生窗口目录](../rust/src/native/)。

C++ 基线四项 CTest 已复跑通过。Rust 回归覆盖 [业务与迁移](../rust/tests/migration.rs)、[Windows 适配器](../rust/tests/windows.rs)和 [原生窗口](../rust/tests/native_ui.rs)。窗口测试启动实际进程，检查引擎菜单、别名提示、设置切页和保存、单实例唤醒、关闭到托盘和正常退出。

划词的 [交互式回归](../rust/src/platform/selection/tests.rs) 分别验证 Win32 Edit、RichEdit UI Automation，以及 Ctrl+C 未响应后的 Ctrl+Insert 和原剪贴板文本与自定义格式恢复。产品快捷键已通过 Edge 页面及工具栏焦点、Windows Terminal 单窗格和分屏验收。旧式控制台及更多第三方应用、自启动、Explorer 重启、IME、混合 DPI 和截图对照仍待验收。

### Rust 原生版性能优化

托盘启动推迟到首次唤醒才创建搜索控件。输入时按变化更新提示和引擎按钮，别名只计算一次。绘制按需创建 DirectWrite 文本格式，复用 UTF-16 缓冲区，并直接从图标缓存上传像素。图标继续使用 resvg 16×。

同机以优化前后的 Release 交替运行，预热一轮后取五轮中位数。启动、唤醒与菜单时间以 Win32 窗口可见及同步绘制完成为检测点。文本更新通过原生 Edit 的 `WM_SETTEXT` 测量。回到托盘后等待 1.65 秒再读取内存。

| 指标 | 优化前 | 优化后 |
| --- | ---: | ---: |
| 直接启动到搜索框绘制结束 | 82.85 ms | 88.39 ms |
| 托盘启动到后台窗口响应 | 32.27 ms | 30.51 ms |
| 首次从托盘打开搜索框 | 60.45 ms | 58.90 ms |
| 首次打开引擎菜单 | 27.96 ms | 28.80 ms |
| 热唤醒 | 26.89 ms | 27.09 ms |
| 普通文本更新 | 2.55 ms | 0.68 ms |
| 首次托盘空闲私有字节 | 2.13 MiB | 2.09 MiB |
| 使用后回到托盘的私有字节 | 7.34 MiB | 6.48 MiB |
| 使用后回到托盘的工作集 | 27.78 MiB | 27.09 MiB |

### 内存诊断与 C++ 对照

启动 Rust 版前可设置 `YISOU_MEMORY_TRACE` 为 CSV 路径，父目录需要已存在。退出正在运行的实例后重新启动，采样记录由常驻进程写入。CSV 含 PID、相对时间、工作集、私有字节、峰值工作集以及 GDI 和 USER 对象数量，采样点覆盖启动、搜索框首次绘制、菜单显示、托盘释放、历史页加载、历史缓存释放和退出。日常运行默认关闭。

```powershell
$env:YISOU_MEMORY_TRACE = Join-Path $PWD 'build/rust-memory.csv'
Start-Process build/rust-native/release/YiSou.exe
Remove-Item Env:YISOU_MEMORY_TRACE
```

离开历史页或关闭设置时释放历史缓存，在托盘空闲且设置已关闭时也释放缓存。用 12,000 条记录测试，关闭设置后私有内存中位数由 15.79 MiB 降至 5.91 MiB。内置图标由构建过程用 resvg 16× 独立生成 100% 至 200% 常用 DPI 的对应物理尺寸，压缩后随 EXE 分发。自定义 SVG 和其他尺寸在运行时栅格化。DirectWrite 使用实例独立的字体缓存，随绘制资源一起释放。

同机交替运行三个 Release，关闭诊断，忽略首轮预热，取五轮中位数。使用同等默认配置，菜单和搜索框的耗时记录到同步绘制返回。窗口显示 350 ms 后读取内存，关闭到托盘 1.65 秒后再读取。C++ 版使用 DirectComposition，Rust 版使用 Direct2D 软件渲染。

| 指标 | C++ Release | Rust 优化前 | Rust 优化后 |
| --- | ---: | ---: | ---: |
| EXE | 4.10 MiB | 3.21 MiB | 3.37 MiB |
| 托盘启动到后台窗口响应 | 35.82 ms | 29.78 ms | 30.02 ms |
| 首次从托盘打开搜索框 | 55.27 ms | 60.62 ms | 56.18 ms |
| 首次绘制引擎菜单 | 39.12 ms | 34.40 ms | 21.61 ms |
| 搜索框显示时的私有字节 | 7.58 MiB | 7.74 MiB | 7.88 MiB |
| 引擎菜单显示时的私有字节 | 12.51 MiB | 12.62 MiB | 12.72 MiB |
| 使用后回到托盘的私有字节 | 11.65 MiB | 6.43 MiB | 5.96 MiB |
| 使用后回到托盘的工作集 | 32.35 MiB | 26.66 MiB | 25.24 MiB |

### 搜索框材质与设置对标

Rust 搜索框使用 Windows Desktop Acrylic 与 DirectComposition 的半透明浅色层，保留原生输入和引擎按钮。高对比模式或合成路径不可用时使用实色背景。退到托盘并空闲后释放合成资源。屏幕像素测试用同一位置的两种纯色窗口作背景，验证搜索框和输入区的底色随背景变化。

设置页已接通历史关键词与来源筛选、本地字体文件及预览、划词引擎选择。已覆盖双条件筛选、清除筛选、划词绑定保存、私有字体加载和失败、托盘释放后唤醒。中英文切换、快捷键录制、跨会话显示器位置记忆和中文输入法等实机流程仍待完成。SVG/PNG 对比窗口属于开发工具，排除在产品功能对标范围外。

启用亚克力后，使用同机五轮 Release 中位数更新性能对照。首次唤醒和菜单首开测到同步绘制返回，返回托盘后等待 1.65 秒采样。

| 指标 | C++ Release | Rust 亚克力版 |
| --- | ---: | ---: |
| EXE | 4.10 MiB | 3.39 MiB |
| 首次唤醒 | 57.17 ms | 54.07 ms |
| 引擎菜单首开 | 46.65 ms | 27.81 ms |
| 搜索框可见时的私有字节 | 7.32 MiB | 10.58 MiB |
| 使用后回到托盘的私有字节 | 11.61 MiB | 9.20 MiB |

### 完成条件

- F01–F13 每项有升级前结果、升级后结果和可复现步骤。行为改动有明确的新期望和新测试。
- 保存失败、启动失败、重复启动、连续划词、导入错误、输入法和混合 DPI 有验证记录。图标对照包含实际 UI 截图。
- Release 产物通过体积、性能和资源对比，并能在目标 Windows 环境运行。每次修改后按项目流程退出当前易搜、构建并打开本次生成的 `YiSou.exe`。
