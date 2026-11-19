# Buildship

跨平台桌面前端发布平台。把「本机构建 → 打包 → 上传 → 原子上线 → 健康检查 → 回滚」
收敛进一个 Windows / macOS 桌面应用里，每一步都有记录、可回滚、可审计。

- 技术栈：**Tauri 2 + Rust + Vue 3 + TypeScript + SQLite**
- 不使用 Electron；Windows 用 WebView2，macOS 用 WKWebView
- 桌面端约 4MB 级安装包依赖系统 WebView，不内嵌浏览器内核

## 它解决什么

前端项目发布长期靠"手工 npm run build + 拖文件到 FTP/宝塔"，问题是：
版本对不上、谁传的不知道、传坏了回不去。Buildship 把这条链路产品化：

| 能力 | 说明 |
| --- | --- |
| 项目纳管 | 只读扫描目录，识别框架、包管理器、建议构建脚本、git commit |
| 构建执行 | 参数数组启动子进程（不拼 shell），日志实时流式展示并落盘，可取消（整进程树） |
| 发布包 | ZIP + `manifest.json` + SHA-256 + 目录树哈希，排除 `.env*` / 密钥 / `node_modules` |
| 发布 | 上传（流式）→ 校验 → Agent 侧原子切换 `current` → 健康检查 |
| 回滚 | 一键回滚到任一历史健康版本，**必须填写原因**，原因进审计 |
| 审计 | 所有关键动作写 `audit_events`，数据库触发器禁止修改与删除 |
| 悬浮栏 | Aurora Loop 悬浮入口，悬停展开快捷操作，可拖到任意位置并记住，区域外鼠标穿透 |

## 目录

```text
crates/
  buildship-common     错误码、路径、时间、脱敏、日志
  buildship-domain     枚举、实体、状态机、DTO
  buildship-db         SQLite 迁移、仓储、备份
  buildship-build      工具链探测、项目扫描、子进程执行、构建运行器
  buildship-artifact   哈希、manifest、ZIP 打包与校验
  buildship-release    Agent 协议结构 + HTTP 客户端
apps/desktop/          Tauri 外壳（Rust）+ Vue 3 渲染层
deploy-agent/          目标服务器发布代理（独立二进制）
docs/                  架构 / 数据库 / IPC / 部署协议 / 安全 / 测试
```

## 环境要求

| 依赖 | 版本 | 说明 |
| --- | --- | --- |
| Rust | 1.77+（实测 1.100） | 目标 `x86_64-pc-windows-gnu` 或 `x86_64-apple-darwin` |
| Node.js | 20+（实测 22） | 前端构建；同时是被管理项目的构建运行时 |
| C 工具链 | Windows 需 MinGW-w64 gcc | `libsqlite3-sys` 需要编译 C 代码 |
| WebView2 | Windows 10/11 自带 | 缺失时安装包会自动引导 |

> **Windows 上必须能调用 `gcc`**。若使用 MSYS2：
> `pacman -S mingw-w64-x86_64-gcc`，并把 `<msys64>\mingw64\bin` 加入 `PATH`。

## 开发

```bash
# 1) 前端依赖
cd apps/desktop && npm ci

# 2) 开发模式（前端热更新 + Rust 热重启）
npm run tauri dev

# 3) 只跑前端类型检查与打包
npm run build

# 4) Rust 侧检查与测试
cargo check --workspace
cargo test --workspace
```

### 日志与控制台

- 桌面端声明了 `windows` 子系统（GUI）：**双击启动不会弹控制台黑框**，窗口只有主面板与悬浮条。
- 发布版是纯后台进程：不 attach 任何控制台，日志只写文件；
  由资源管理器双击启动时三个标准句柄都是空的，日志层直接丢弃控制台输出，不做无谓写入。
- 需要临时在终端看输出：`set BUILDSHIP_CONSOLE=1` 后启动，或直接用 debug 构建
  （`npm run tauri dev` 默认就会挂回父控制台）。`buildship.exe > a.log` 这类重定向会被识别并尊重。
- Windows 控制台默认不解析 ANSI：日志初始化时尝试开启 `ENABLE_VIRTUAL_TERMINAL_PROCESSING`，
  失败就自动关闭着色，避免终端里出现 `[32m`、`[2m` 这种裸露序列；设 `NO_COLOR=1` 可强制关闭。
- 控制台输出走 `WriteConsoleW`（UTF-16），不受控制台本地代码页（936/GBK）影响，
  中文不会变成 `鍚姩`；管道与文件场景仍写 UTF-8 字节。
- 持久日志在 `%APPDATA%\Buildship\logs\buildship.<date>.log`（保留 7 天、纯文本无转义码）。
  桌面端注册了 panic hook，崩溃信息会进日志，不会因为"没有 stderr"而消失。
- 判断手头的 exe 是哪种：`python -c "import struct;d=open('buildship.exe','rb').read();e=struct.unpack_from('<I',d,0x3c)[0];print(struct.unpack_from('<H',d,e+92)[0])"`
  —— 输出 `2` 是 GUI（无黑框），`3` 是控制台版（旧包）。

## 打包

```bash
cd apps/desktop
npm run tauri build
```

产物：

| 平台 | 路径（工作区根 `target/`） | 格式 |
| --- | --- | --- |
| Windows | `target/release/bundle/nsis/*.exe`、`target/release/bundle/msi/*.msi` | NSIS / MSI |
| macOS | `target/release/bundle/dmg/*.dmg`、`target/release/bundle/macos/*.app` | DMG / APP |

> **Windows GNU 工具链的两个坑（实测踩过，已处理）**
>
> 1. `webview2-com-sys` 在 GNU 目标下链接的是 `WebView2Loader.dll`（MSVC 目标才静态链接 `.lib`）。
>    仓库已固化 `apps/desktop/src-tauri/vendor/WebView2Loader.dll`：`build.rs` 会把它复制到
>    `target/<profile>/`、`deps/` 与测试产物目录，`bundle.resources` 会把它装到安装根目录。
>    缺了它启动直接 `0xC0000135 (STATUS_DLL_NOT_FOUND)`。
> 2. 打包需要 NSIS 工具链，`tauri build` 会自动从 GitHub 下载到 `%LOCALAPPDATA%\tauri\NSIS`。
>    内网/代理环境下载失败时，可离线预置：解压 [nsis-3.13.zip](https://github.com/tauri-apps/binary-releases/releases/download/nsis-3.13/nsis-3.13.zip)
>    到该目录（`makensis.exe` 在根层），再放好 `Plugins/x86-unicode/nsis_tauri_utils.dll`，
>    然后执行 `npx tauri build --bundles nsis`。

### 安装包语言（中文 / 英文）

`tauri.conf.json` 的 `bundle.windows.nsis.languages` 决定安装界面语言：

```json
"nsis": {
  "languages": ["English", "SimpChinese"],
  "displayLanguageSelector": false
}
```

行为（NSIS 规则，已在本机中文 Windows 上实测）：

- **默认按系统 UI 语言自动选择**：中文 Windows 直接出中文安装界面，英文系统出英文界面，不弹选择框；
- 系统语言不在列表里时回落到列表第一个（这里是 English）；
- Tauri 自带中文文案（`SimpChinese.nsh`），"已安装 / 正在运行 / 维护操作" 等自定义提示也是中文；
- 卸载程序同样按系统语言显示。

MSI 走另一套开关，每种语言各出一个包：

```json
"wix": {
  "language": {
    "en-US": {},
    "zh-CN": { "localePath": "wix/zh-CN.wxl" }
  }
}
```

WixUI 的对话框文案由 WiX 自带的中文语言段提供，`wix/zh-CN.wxl` 只需覆盖 Tauri 模板自己的几条文案
（"启动 Buildship"、功能描述等）。已实测：`_zh-CN.msi` 为中文界面，`_en-US.msi` 全英文、无中文残留。

> MSI 的字符串按 `Codepage`（936 = GBK）存放而非 UTF-16，用 `strings` / 十六进制搜 UTF-16 会误判为"没有中文"。

想改成"安装时自己选"：把 `displayLanguageSelector` 设为 `true`，会多出一个语言选择对话框（默认已选中系统语言），
此时命令行 `Buildship_..._setup.exe /LANG=2052` 也生效。
注意：**关闭选择器时 `/LANG=` 不生效** —— `/LANG` 由 NSIS 的 LangDLL 解析，只有插入语言选择器宏时才读取它。

图标来自设计素材包 `Buildship_白蓝液态玻璃图标_素材包`（白蓝液态玻璃光球，**透明背景版**）：

| 用途 | 文件 | 落地位置 |
| --- | --- | --- |
| 系统应用图标（Windows） | `Buildship_白蓝液态玻璃球.ico`（16~256 多尺寸） | `apps/desktop/src-tauri/icons/icon.ico` |
| 系统应用图标（macOS） | `Buildship_白蓝液态玻璃球.icns` | `icons/icon.icns` |
| 各尺寸 PNG / Store 图标 | `Buildship_白蓝液态玻璃球_透明背景_512.png` | PIL/LANCZOS 重采样生成全套 |
| 悬浮入口 | `Buildship_白蓝液态玻璃球_透明背景_紧凑512.png` | `src/assets/buildship-orb.png` |

悬浮图标的动态感由 CSS 实现（外围柔光呼吸 + 沿边缘流动的高光），不直接用素材包里的
`循环动画.gif` —— 那个 GIF **没有透明通道**（`transparency=None`），贴上去会带一块方形底，
与「必须透明」的要求冲突。

换图标的标准流程（在 `apps/desktop` 下）：

```bash
# 1) 复制 .ico / .icns
cp "路径/Buildship_白蓝液态玻璃球.ico"  src-tauri/icons/icon.ico
cp "路径/Buildship_白蓝液态玻璃球.icns" src-tauri/icons/icon.icns
# 2) 用 512 透明 PNG 重采样出各尺寸 PNG（含 Store 图标）
```

托盘的 `default_window_icon()` 与安装包图标都取自 `bundle.icon`，改完重新打包即可同步。
应用图标与悬浮图标同源（同一颗光球），区别只在留白：应用图标用 `_512`（球体约占画布 66%，
符合系统图标的视觉重量），悬浮入口用 `_紧凑512`（约占 90%，50px 下更清晰）。
绿色在线状态点只由悬浮窗的 HTML 叠加绘制，不并入图标文件。

### 卸载与数据目录

Buildship 的数据放在 `%APPDATA%\Buildship`（macOS 为 `~/Library/Application Support/Buildship`）：

```
buildship.db  buildship.db-wal  buildship.db-shm
artifacts/    backups/          work/logs/
```

两条卸载路径都会清干净：

1. **应用内**：设置 → 重置与卸载 →「一键卸载」。先停构建、删系统凭据、关连接池、删除数据目录，
   再以 `/S` 调用安装目录的 `uninstall.exe`，随后退出。
   同一区域还有「清理本机数据」（保留程序、重启为全新状态）。
2. **控制面板卸载**：`src-tauri/nsis/hooks.nsh` 在 `NSIS_HOOK_POSTUNINSTALL` 里补删
   `%APPDATA%\Buildship`。Tauri 模板自带的「删除应用数据」只清 `$APPDATA\<bundleId>`，
   而本项目的目录名是 `Buildship`，两者不同 —— 这是此前卸载后仍留一堆文件的原因。

需要留存数据时，在点卸载前勾选「先导出备份到桌面」：会先 `VACUUM INTO` 一份一致性备份再复制到桌面。

## 部署代理

目标服务器上运行 `deploy-agent`，桌面端通过 HTTP 调用它完成上传与切换。

```toml
# deploy-agent.toml
bind = "0.0.0.0:7801"
tokenFile = "C:\\ProgramData\\Buildship\\agent-token.txt"
linkStrategy = "auto"

[[sites]]
name = "demo"
physicalPath = "C:\\sites\\demo"
healthCheckUrl = "https://demo.example.com/healthz"
keepReleases = 5
```

```bash
deploy-agent --config deploy-agent.toml --check   # 校验配置与目录权限
deploy-agent --config deploy-agent.toml
```

Web 服务器（IIS / nginx）站点根目录指向 `<physicalPath>\current`，
协议细节见 [`docs/deployment-protocol.md`](docs/deployment-protocol.md)。

## 文档

| 文档 | 内容 |
| --- | --- |
| [docs/architecture.md](docs/architecture.md) | 技术架构图、分层与模块边界、仓库设计、关键数据流 |
| [docs/database.md](docs/database.md) | ER 图、14 张表、部分唯一索引、审计触发器、备份恢复 |
| [docs/ipc-contracts.md](docs/ipc-contracts.md) | 52 个 IPC 命令、事件契约、错误码、capabilities |
| [docs/deployment-protocol.md](docs/deployment-protocol.md) | Agent HTTP 协议、目录布局、幂等与切换语义 |
| [docs/security.md](docs/security.md) | 威胁模型、子进程安全、凭据、脱敏、Agent 防护 |
| [docs/testing.md](docs/testing.md) | 测试策略、已覆盖用例、手工验收清单、实测执行记录 |

## 当前状态

| 模块 | 状态 |
| --- | --- |
| SQLite 全量 schema（14 表） | 已完成，8 条约束/触发器集成测试覆盖 |
| 项目扫描 / 构建 / 取消 / 日志 | 已完成 |
| 打包（ZIP + manifest + 哈希） | 已完成 |
| 环境 / 站点 / 发布 / 回滚 / 审计 | 已完成 |
| Vue 3 主面板（7 页）+ Aurora 悬浮栏 | 已完成 |
| Deploy Agent | 已完成，端到端脚本 24 个检查点全部通过 |
| 自动化测试 | `cargo test --workspace` → 50 通过 / 0 失败 |
| 静态检查 | `cargo clippy -D warnings` 8 个目标 0 警告；`vue-tsc --noEmit` 0 错误 |
| Windows 构建 | 已在本机验证（release exe 7.8 MB） |
| Windows 安装包（NSIS） | 已产出：`Buildship_0.2.1_x64-setup.exe` 3.05 MiB |
| Windows 安装包（MSI） | 已产出：`Buildship_0.2.1_x64_en-US.msi` 4.17 MiB、`zh-CN` 4.16 MiB（WiX 离线预置） |
| macOS 构建与实机验收 | **未验证**（无 Mac 设备，代码已按 `cfg(target_os)` 分支实现，不标记为已验收） |

## 许可

MIT
