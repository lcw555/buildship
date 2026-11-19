# 测试策略与验收清单

> 对应方案 15 章与 16 章"每个阶段都要提供可运行成果、测试结果、变更清单和未完成项"。

## 1. 分层策略

| 层级 | 范围 | 手段 |
| --- | --- | --- |
| 单元测试 | 纯函数与状态机（哈希、脱敏、参数校验、状态流转、路径规范化） | `cargo test`，无外部依赖 |
| 集成测试 | 数据库迁移、约束、审计触发器、Agent 切换与解压 | 内存库 / 临时目录 |
| 端到端 | 扫描 → 构建 → 打包 → 上传 → 应用 → 回滚 | 手工清单（见第 4 节） |
| 静态检查 | 类型、格式、lint | `vue-tsc --noEmit`、`cargo clippy -D warnings`、`cargo fmt --check` |

原则：**能纯函数化的逻辑绝不留到手工验证**。`services/*` 里的编排逻辑靠状态机 +
数据库约束兜底，关键分支（幂等、回滚）在 Agent 侧有单测。

## 2. 已有自动化用例

| crate | 数量 | 覆盖点 |
| --- | --- | --- |
| `buildship-common` | 5 | 日志脱敏（`token=xxx` / `"password": "x"` / `--token xxx` / `Authorization: Bearer` / 非敏感键不受影响） |
| `buildship-domain` | 5 | 构建/发布/部署状态机合法与非法流转、回滚目标校验 |
| `buildship-build` | 6 | 参数白名单（拒绝 `& \| < > ^ % ! " ' \`` 与换行）、脚本路径识别、包管理器探测、框架识别、slug 折叠 |
| `buildship-artifact` | 5 | 目录树哈希与顺序无关、内容变化敏感、打包排除规则（`.env` / `node_modules` / 密钥扩展名）、中文文件名、篡改检测、空产物目录拒绝 |
| `deploy-agent` | 11 | 配置校验、站点名与路径规范化、固定时间比较、ZIP 解压、**zip-slip 拒绝**、`manifest.json` 不落站点、`link`/`copy` 两种切换、历史版本保留、未知版本拒绝 |
| `buildship-db` | 8 | 迁移一次建齐 14 张表、三类部分唯一索引、审计只追加（UPDATE/DELETE 均被拒）、外键生效、CHECK 拒绝非法枚举与短哈希、迁移后完整性检查通过 |

运行：

```bash
cargo test --workspace            # 全部单测（桌面端装配 crate 显式 test = false）
cd apps/desktop && npx vue-tsc --noEmit && npm run build
```

> 注意：不要用 `--all-targets` 全量跑。桌面端 `apps/desktop/src-tauri` 是纯装配 crate
> （无单测），其测试宿主会动态链接 `WebView2Loader.dll`；该 DLL 已由 `build.rs`
> 自动铺到产物目录，但仍建议 CI 只跑业务 crate。

## 3. 数据库验证方式

`crates/buildship-db/tests/constraints.rs` 用**内存库 + 真实迁移**验证数据库兜底的不变量：

| 用例 | 断言 |
| --- | --- |
| `migration_creates_all_fourteen_tables` | 迁移后业务表数量恰好 14，`schema_version >= 1` |
| `only_one_active_build_per_project` | 第二个 `queued`/`running` 构建被 `idx_build_tasks_active` 拒绝；前一任务结束后可再建 |
| `only_one_default_profile_per_project` | 第二个 `is_default=1` 被拒；非默认可多个 |
| `only_one_current_release_per_project_and_env` | 第二个 `is_current=1` 被拒；重复版本号被拒 |
| `audit_events_are_append_only` | `UPDATE` / `DELETE` 均报错，且原记录条数不变 |
| `foreign_keys_are_enforced` | 指向不存在项目的 `build_tasks` 插入失败 |
| `checks_reject_invalid_enum_and_hash` | 非法 `status` 与长度不足 64 的 `sha256` 被 CHECK 拒绝 |
| `integrity_check_passes_after_migration` | `PRAGMA integrity_check` = ok 且无外键冲突 |

这些是"绕过 service 层直接写库"的最后一道防线，因此必须自动化而不是手工验证。

## 4. 手工验收清单（Windows 实机）

### 4.1 启动与向导

- [ ] 首次启动进入引导页，展示 Node / npm 版本；缺失时给出安装指引而不是崩溃
- [ ] 数据目录创建成功，`buildship.db` 存在且 `schema_migrations` 有记录
- [ ] 关闭主窗口 → 收进托盘，进程仍在；托盘菜单可重新显示

### 4.2 项目

- [ ] 选择目录 → 扫描出框架、包管理器、建议脚本、git commit
- [ ] 添加项目后 slug 唯一；重复路径加第二个 → 提示冲突
- [ ] 归档后列表默认不显示，历史构建记录仍可查

### 4.3 构建

- [ ] 启动构建 → 日志实时滚动，stdout/stderr 有区分
- [ ] 中文输出不乱码（GBK 容错）
- [ ] 构建中再次点构建 → `BUILD_ALREADY_RUNNING`
- [ ] 取消 → 子进程树全部结束（任务管理器无残留 node.exe）
- [ ] 构建失败 → 退出码与错误信息入库，日志可从历史回读
- [ ] 拔电/强杀后重启 → 残留任务被标记为失败（`recover_interrupted`），不会永远 running

### 4.4 发布包

- [ ] 生成包 → 目录内有 `.zip` 与 `.manifest.json`
- [ ] `artifact_verify` 通过；手工改一个字节后校验失败
- [ ] 产物目录含 `.env.production` 与 `node_modules` → 包内不含
- [ ] 已被发布引用的包 → 删除被拒绝

### 4.5 发布与回滚（需要 Agent）

```bash
# 服务器
deploy-agent --config deploy-agent.toml --check
deploy-agent --config deploy-agent.toml
```

- [ ] 站点检测 → 返回 Agent 版本、站点当前版本
- [ ] 发布 → 进度事件依次为 uploading / applying / deployed
- [ ] 服务器 `current` 已是新版本，站点可访问，健康检查 passed
- [ ] 把健康检查地址改成 404 → 发布失败 → **自动回滚**，站点仍可用，事件里有 `rolledBack`
- [ ] 手动回滚到上一版本 → 必须填写原因 → 原因出现在审计与 `audit.jsonl`
- [ ] 同一 `requestId` 重复调用 apply → 返回相同结果，磁盘不变（幂等）
- [ ] 同 releaseId 上传不同内容 → `CONFLICT`，不覆盖已部署版本

### 4.6 悬浮栏

- [ ] 图标为 Aurora Loop（2~3 条流体丝带、开放式环流、中心负空间）
- [ ] 在线状态点为独立绘制的绿点，不在图标内
- [ ] 鼠标不在区域内 → 完全穿透（能点到下面的窗口）
- [ ] 悬停 → 展开胶囊；移开 320ms → 收起
- [ ] 快捷构建可触发；点"面板""发布包" → 主面板跳到对应页
- [ ] 拖动可移动，贴边不越界

### 4.7 备份与恢复

- [ ] 创建备份 → 大小与 sha256 记录正确
- [ ] 恢复 → 自动先做安全备份；恢复后 DB 完整性检查通过；审计有记录
- [ ] 用损坏文件恢复 → 明确拒绝，原库不受影响

### 4.8 审计

- [ ] 新增项目 / 构建 / 打包 / 发布 / 回滚 / 恢复备份 都有记录
- [ ] 用 SQLite 工具尝试 `UPDATE audit_events` → 报错拒绝

## 5. 平台差异

| 项 | Windows（已验证） | macOS（未验证） |
| --- | --- | --- |
| WebView | WebView2 | WKWebView |
| 凭据 | Windows 凭据管理器 | 钥匙串 |
| 鼠标穿透 | `GetCursorPos` + `set_ignore_cursor_events` | 无 `GetCursorPos`，热区检测降级（悬浮栏不穿透，避免永久挡住点击） |
| 进程树终止 | `taskkill /T /F` | `kill(-pid)` 进程组 |
| 打包 | NSIS + MSI | DMG |
| 构建验证 | `npx tauri build` 成功（release 7.8 MB + NSIS 3.04 MiB） | **未在本机验证，不得标记已验收** |

## 6. 未覆盖与后续

| 缺口 | 原因 | 计划 |
| --- | --- | --- |
| `services/*` 无单测 | 依赖 Tauri `AppHandle` 与真实进程 | 抽出纯逻辑（如 `build_args`、`version_of`）后单测；编排靠 E2E 清单 |
| 长时构建（>30min）内存曲线 | 未做压测 | 日志改为按批 flush，观察 RSS |
| 并发多站点发布 | 设计上串行（站点锁） | 如需并行，改为每站点独立部署队列 |
| macOS 实机 | 无设备 | 需 CI 的 macOS runner 或借一台机器手工过一遍清单 |

## 7. 实测执行记录（Windows 11 / rustc 1.100.0 / Node 22.22.2 / gcc 16.2.0）

以下为**真实执行过**的结果，不是计划。

### 7.1 自动化与静态检查

| 检查 | 命令 | 结果 |
| --- | --- | --- |
| 单元 + 集成测试 | `cargo test --workspace` | **40 通过 / 0 失败**（5+5+8+6+5+0+11） |
| 编译 | `cargo check --workspace` | 0 error（桌面端 + Agent 全量） |
| 格式 | `cargo fmt --all -- --check` | 无差异 |
| Lint | `cargo clippy -p … --all-targets -- -D warnings` | 0 warning（7 个 crate） |
| 前端类型 | `npm run typecheck` | 0 error |
| 前端构建 | `npm run build` | 双入口产物生成成功 |

### 7.2 桌面端运行

启动 `target/debug/buildship.exe` 后 8 秒：

| 检查项 | 结果 |
| --- | --- |
| 进程存活 | ✅ 常驻内存约 52 MB |
| 悬浮窗 | ✅ 出现 `Buildship Floating` 窗口 |
| 数据目录 | ✅ `%APPDATA%\Buildship\buildship.db`（+ `-wal` / `-shm`）、`logs\buildship.<date>.log` |
| schema | ✅ 14 张业务表、23 个索引、2 个审计触发器 |
| 完整性 | ✅ `PRAGMA integrity_check = ok`，`journal_mode = wal` |
| 迁移记录 | ✅ `schema_migrations = (1, 'init', 2026-11-19T01:24:25.752Z)` |

### 7.3 Agent 端到端（curl 实打实跑通）

| 步骤 | 期望 | 实际 |
| --- | --- | --- |
| `GET /healthz` | 200 `ok` | ✅ |
| `GET /v1/status` 无 token | 403 | ✅ `CREDENTIAL_UNAVAILABLE` |
| `GET /v1/status` 带 token | 站点概览 | ✅ |
| 上传 v1 | 解压成功 | ✅ `fileCount: 2`，返回 `treeSha256` |
| 发布 v1 | 切换 + 健康检查 | ✅ `healthStatus: passed`，**`switchMode: link`** |
| 读 `current/index.html` | v1 内容 | ✅ |
| 发布 v2 | `previousReleaseId: rel-v1` | ✅ |
| 同 `requestId` 重放 | 不重复切换 | ✅ `appliedAt` 与首次完全一致 |
| 回滚到 v1 | 原因落审计 | ✅ `previousReleaseId: rel-v2`，内容已回退 |
| 哈希不符上传 | 422 | ✅ `CHECKSUM_MISMATCH` |
| 健康检查失败 | 自动回滚 | ✅ `healthStatus: failed` + `rolledBack: true`，内容回到 v1 |
| 越权 `physicalPath` | 403 | ✅ `PERMISSION_DENIED` |

> `switchMode: link` 说明 Windows 目录联接（`DeviceIoControl(FSCTL_SET_REPARSE_POINT)`）
> 实际生效，无需管理员权限、未调用 `mklink`。

以上场景已固化为**可重复执行的脚本**，不是一次性手工操作：

```bash
cargo build -p deploy-agent        # 先构建
bash scripts/e2e-agent-smoke.sh    # 自起 Agent、自建站点、跑完自动收尾
```

脚本每次用独立临时目录（避免上一轮版本与幂等缓存干扰），共 **29 条断言**，
实测 `29 通过 / 0 失败`。输出形如：

```text
==> 3. 发布 v1（健康检查通过）
  ✓ 应用成功
  ✓ 健康检查通过
  ✓ 使用目录联接切换
  ✓ current 指向 v1 内容
...
==== 结果：29 通过 / 0 失败 ====
```

### 7.4 构建产物

| 产物 | 路径 | 大小 | 结果 |
| --- | --- | --- | --- |
| Release exe | `target/release/buildship.exe` | 7.8 MB | ✅ PE Subsystem = 2 (GUI) |
| NSIS 安装包 | `target/release/bundle/nsis/Buildship_0.2.3_x64-setup.exe` | 3.22 MiB | ✅ |
| MSI 安装包 | `target/release/bundle/msi/Buildship_0.2.3_x64_{en-US,zh-CN}.msi` | 4.35 / 4.34 MiB | ✅（WiX 3.14 离线预置后 `candle` + `light` 均通过） |
| Agent（release） | `target/release/deploy-agent.exe` | 2.4 MB | ✅（`--check` 自检通过） |

> 打包机在受限网络下首次 `tauri build` 会因下载 NSIS / WiX 失败（`peer closed connection`
> 或 `CONNECT proxy failed`）。离线预置方法见 README「打包」小节；预置后 NSIS 与 MSI 均可产出。

安装脚本已确认 `SetOutPath $INSTDIR` + `File /oname=WebView2Loader.dll`，
即 GNU 工具链所需的 WebView2 加载器会落到安装根目录，卸载时同步清理。

### 7.5 本轮修复的问题（构建阶段暴露）

| 问题 | 根因 | 处置 |
| --- | --- | --- |
| `cargo check` 报 `cannot find crate sqlx/reqwest` | `default-features = false` 关掉了 `derive` | 工作区开启 `derive` feature |
| Windows 链接报 `export ordinal too large: 149624` | `crate-type` 含 `cdylib`，导出表超 6.5 万 | 桌面端收窄为 `["rlib"]` |
| 测试二进制启动即退 `0xC0000135` | GNU 目标下缺 `WebView2Loader.dll` | 固化 `vendor/` 并在 `build.rs` 铺到产物目录 |
| Agent 所有 apply 被 403 | `canonicalize` 返回 `\\?\` 前缀，与客户端路径不等 | `strip_verbatim` + Windows 大小写不敏感比较 |
| `Authorization: Bearer x` 未被脱敏 | 值在下一段，旧实现只处理同段 | 重写为 `Pending::MaskScheme` 状态机并补 2 个用例 |
| `vue-tsc` 报 `Cannot find module 'node:url'` | 缺 `@types/node` | 补 devDependency + `types: ["node"]` |

### 7.6 安装包语言（中文 / 英文）

验证目标：安装包在中文系统上直接显示中文，且不存在"配置改了但没生效"的情况。

| 检查 | 方法 | 结果 |
| --- | --- | --- |
| 语言表是否进入安装包 | `target/release/nsis/x64/installer.nsi` 中应有两条 `!insertmacro MUI_LANGUAGE` | ✅ `English` + `SimpChinese` |
| 中文自定义文案 | Tauri 生成的 `SimpChinese.nsh`（"已安装 / 正在运行 / 维护操作"） | ✅ 已生成并被 `!include` |
| 系统语言自动检测 | 用同款 NSIS 工具链编译最小探针，`Section` 内把 `$LANGUAGE` 写入文件后 `/S` 运行 | ✅ 本机 `zh-CN` → `$LANGUAGE = 2052`（简体中文），无对话框 |
| 语言选择器开关 | `installer.nsi` 中 `!if "${DISPLAYLANGUAGESELECTOR}" == "true"` 包裹 `MUI_LANGDLL_DISPLAY` | ✅ 当前为 `false`，即不弹选择框，走系统语言 |
| `/LANG=` 覆盖 | 同一探针加 `/LANG=1033` | ⚠️ 仍为 2052 —— `/LANG` 由 LangDLL 解析，**只有开启选择器才生效**，已在 README 写明 |
| MSI 中文界面（WixUI） | 扫描 `_zh-CN.msi` 中的 GBK 串 | ✅ "下一步 / 取消 / 欢迎使用" 均为中文 |
| MSI 中文自定义文案 | 扫描 `_zh-CN.msi` 中 "启动 Buildship / 安装 Buildship 桌面应用" | ✅ 命中；`_en-US.msi` 对应位置为英文、无中文残留 |

> 一个容易误判的点：MSI 的字符串按 `Codepage` 存放（zh-CN 为 936，GBK 字节），
> 用 UTF-16 或 ASCII 去搜必然"找不到中文"，据此判断"没本地化"是错的。
> NSIS 则整块 LZMA 压缩，连英文 `Next >` 都搜不到，明文扫描同样不可用于判断。

为 MSI 新增的文件：`apps/desktop/src-tauri/wix/zh-CN.wxl`（只覆盖 Tauri 模板的
`LaunchApp / DowngradeErrorMessage / PathEnvVarFeature / InstallAppFeature / TauriLanguage / TauriCodepage`）。

### 7.7 控制台与日志渲染（启动黑框 / ANSI 乱码）

现象：双击启动后主面板旁边多出一个 `cmd.exe` 黑框，框内日志是
`[2m2026-11-19T02:17:05Z[0m [32m INFO[0m ...` 这样的原始转义序列。

| 检查 | 方法 | 结果 |
| --- | --- | --- |
| PE 子系统 | `objdump -p target/{debug,release}/buildship.exe` 看 `Subsystem` | ✅ 两者均为 `00000002 (Windows GUI)` |
| 双击是否分配控制台 | GUI 子系统下系统不会为进程创建控制台 | ✅ 无黑框（此前 `main.rs` 漏了 `windows_subsystem` 属性，debug/release 都会弹） |
| 终端启动仍有日志 | `platform::console` 用 `AttachConsole(ATTACH_PARENT_PROCESS)` 后重定向 `CONOUT$` | ✅ 路径成立；输出已被重定向时自动跳过，不抢占调用方句柄 |
| 重定向是否被尊重 | 用 `cmd` 把 stdout 重定向到文件后运行并检查文件 | ✅ 171 字节、UTF-8、含"启动"与"数据库就绪" |
| 是否还有 ANSI 裸序列 | 同上文件内查 `0x1B` | ✅ 无转义字节（非控制台时自动 `with_ansi(false)`） |
| 中文是否按代码页乱码 | 控制台路径改用 `WriteConsoleW`（UTF-16） | ✅ 不再依赖 936/GBK 解码 |
| 文件日志 | `%APPDATA%\Buildship\logs\buildship.<date>.log` | ✅ 纯文本、无转义码 |
| panic 可见性 | GUI 子系统下 stderr 可能无处可写 | ✅ 已注册 panic hook，写入日志 |

顺带修掉两处此前 clippy 未覆盖到桌面端的告警（`floating.rs` 嵌套 if、`release.rs` 相同的 if/else 分支），
现在 `cargo clippy -p buildship -p buildship-common --all-targets -- -D warnings` 也是 0 告警。

### 7.8 构建日志渲染、悬浮窗透明与布局校准

来自实机截图的三个问题：日志里出现 `[94m…[39m` 与中文乱码；悬浮图标背后一块白色圆角矩形；构建页左右两栏高低不齐。

| 检查 | 方法 | 结果 |
| --- | --- | --- |
| 日志里的 ANSI 是否还裸露 | 前端 `composables/useAnsi.ts` 解析 SGR 并转内联样式；用 5 组真实输出做样本校验 | ✅ `[94m`→文件路径蓝、`42m/30m`→绿底深字、`2K/1G` 被丢弃、`\r` 进度条取最后一段 |
| GBK 中文是否变 `����` | `buildship_common::text::decode_line`：严格 UTF-8 → 失败按 GBK 解码 | ✅ 单测覆盖 GBK "构建失败" 字节与 ANSI 保留 |
| 历史日志能否回读 | `read_log` 改为按字节切行再逐行解码（原 `read_to_string` 遇非法 UTF-8 会整份失败） | ✅ 日志含混合编码时仍可读，`tail` 参数直接取末尾 2000 行 |
| 构建成功却报"进程异常终止" | 根因是事件循环在管道关闭时 `break`，没等到 `child.wait()`，`exit_code` 为 `None` | ✅ 新增 `crates/buildship-build/tests/exit_code.rs`：断言退出码 0→succeeded、退出码 3→failed |
| 悬浮窗白色背景 | 只给 `body` 设透明不够，`html` 的 `--bs-bg` 会铺满整块画布 | ✅ `html.bs-floating-doc` 一并透明，并在 `platform::floating::setup` 里 `set_background_color(Color(0,0,0,0))` 兜底 |
| 构建页两栏高度 | 改为整页 `height:100%` + 双列 `grid` + 卡片 `flex` 列，日志区 `flex:1` | ✅ 两卡底边对齐，任务列表与日志各自滚动，不再出现左卡半截 |
| 工具栏控件错位 | 工具栏统一成白色卡片，内部按钮/下拉统一 34px 高 | ✅ 五个页面（构建/项目/发布包/发布/审计）共用同一套 `.toolbar` |
| 工作台双栏对齐 | 表头常显（空数据也显示），空状态在剩余空间垂直居中，`align-items: stretch` | ✅ 左右两卡同高、内容起始基线一致 |
| 上万行日志卡顿 | 只渲染最后 1200 行并提示总行数，解析结果按 `taskId:seq` 缓存 | ✅ 渲染量有硬上限（1200 行），与日志总行数无关 |

> 界面观感类改动（间距、对齐、透明）只做了构建与代码级验证，未在真机上逐像素复核；
> 日志着色、编码与退出码判定有单元/集成测试兜底。

### 7.9 一键卸载与图标切换

现象：卸载后在 `%APPDATA%\Buildship` 留下数据库、artifacts、backups、logs、work 一整目录。

| 检查 | 方法 | 结果 |
| --- | --- | --- |
| 卸载残留的根因 | 看 Tauri 生成的 `installer.nsi`：`RmDir /r "$APPDATA\${BUNDLEID}"`，BUNDLEID 是 `com.buildship.desktop`，而 Buildship 数据在 `%APPDATA%\Buildship` | ✅ 路径根本不是同一个，勾选「删除应用数据」也删不到 |
| 安装包侧修复 | `bundle.windows.nsis.installerHooks → nsis/hooks.nsh`，在 `NSIS_HOOK_POSTUNINSTALL` 里补 `RmDir /r "$APPDATA\Buildship"` | ✅ 生成的 `installer.nsi` 第 32 行 include 了钩子，宏在卸载末尾执行 |
| 应用内「清理本机数据」 | 停构建 → 删 keyring 条目 → `pool.close()` → 逐项 `remove_dir_all` → `paths.ensure()` → 延迟重启 | ✅ 编译与静态检查通过；返回 `CleanSummary` 列出删除/失败项与释放体积 |
| 应用内「一键卸载」 | 清理后以 `/S` 启动 `uninstall.exe`；便携/开发运行找不到卸载器时只清数据并提示 | ✅ 卸载器在静默模式下会结束仍在运行的应用 |
| 误删防护 | `ensure_safe_root`：目录名必须是 `Buildship` 且父目录存在 | ✅ 拒绝 `C:\`、用户主目录等危险路径 |
| 备份不被一起删掉 | 「先导出备份到桌面」在删库前 `VACUUM INTO` 再 `copy` 到桌面 | ✅ 备份路径随 `CleanSummary.backupPath` 返回 |
| 应用图标 | 素材包 `Buildship_AppIcon_1024.png` 生成全套（`tauri icon`），再用素材的 `Buildship_Windows.ico` / `Buildship_macOS.icns` 覆盖 | ✅ 圆角方块蓝底 + 极光丝带；`bundle.icon` 与托盘 `default_window_icon()` 同源 |
| 悬浮图标 | `AuroraMark.vue` 改用素材包 `Buildship_Launcher_{animated,static}.svg`，以 `<img>` 加载 | ✅ 圆形蓝盘 + 流动高光；`prefers-reduced-motion` 时切静态版 |

两个实现细节值得记一笔：

1. 素材 SVG 里有 `ribbonA/ribbonB` 等同名渐变 id，同时内联到同一文档会互相串色，
   因此用 `<img>` 引用而不是内联，命名空间天然隔离。
2. 清理时若日志文件仍被 tracing 持有，删除会失败并进入 `failed[]`；
   应用重启后目录已重建，不会影响使用，返回结果里会列出具体条目。

### 7.10 启动仍弹控制台（旧安装副本）与后台日志

现象：换上最新安装包后双击启动，仍然弹出 cmd 窗口并打印启动日志。

| 检查 | 方法 | 结果 |
| --- | --- | --- |
| 仓库产物是否真的是 GUI | 读 PE OptionalHeader `Subsystem`（`e_lfanew+24+68`） | ✅ `target/release/buildship.exe` = `2 (WINDOWS_GUI)` |
| 用户实际运行的是哪个文件 | 枚举 `%ProgramFiles%`、`%LOCALAPPDATA%\Programs`、`%LOCALAPPDATA%\Buildship` 并逐个读 Subsystem | ⚠️ `%LOCALAPPDATA%\Buildship\buildship.exe` = `3 (CONSOLE)`，时间戳 09:50，早于加 `windows_subsystem` 的改动 |
| 快捷方式指向 | 桌面与开始菜单的 `Buildship.lnk` 均指向 `%LOCALAPPDATA%\Buildship\buildship.exe` | ✅ 全部落在旧副本上，源码没问题 |
| 发布版脱离控制台 | `console::init` 加 `console_requested()`：release 默认 false，需 `BUILDSHIP_CONSOLE=1` 才挂回；`logging::console_writer` 在标准句柄为空时改用 `io::sink` | ✅ `cargo clippy -D warnings` / 50 测试全过 |
| 覆盖安装 | 新 NSIS 包 `setup.exe /S` 静默安装到 `%LOCALAPPDATA%\Buildship` | ✅ 安装后 exe = `2 (GUI)`，`uninstall.exe` 同步更新 |
| 运行时有无黑框 | 启动后用 `EnumWindows` + `GetWindowThreadProcessId` 枚举该 PID 的顶层窗口 | ✅ 只有 `Tauri Window`×2、`tray_icon_app`、`IME` 等，`ConsoleWindowClass` 数量 **0** |
| 日志是否进后台 | 双击路径下标准句柄为空 → `io::sink`；文件层始终写入 | ✅ `%APPDATA%\Buildship\logs\buildship.2026-11-19.log` 正常追加"Buildship 启动""数据库就绪" |

结论：源码的子系统声明一直是对的，问题在于机器上留着一份 09:50 的旧安装副本，
而快捷方式与用户习惯都指向它。判断某个 exe 属于哪一版，读 PE 的 `Subsystem` 字段最快（`2` 无黑框，`3` 有）。

### 7.11 启动报 "state not managed"、悬浮窗拖拽与透明、工作台对齐

**① 启动失败（截图报 `state not managed for field 'state' on command 'system_bootstrap'`）**

| 检查 | 方法 | 结果 |
| --- | --- | --- |
| 状态注册时机 | 读 `lib.rs`：`manage(shared)` 写在 `setup` 里，而 `Db::open` 是同步阻塞的 | ⚠️ Tauri 的窗口在 `setup` **之前**就开始加载页面，前端首帧的 `system_bootstrap` 必然撞上未注册状态 |
| 修复 | 把 paths / 日志 / 数据库 / 状态全部提到 `Builder` 之前，`.manage()` 直接挂在 Builder 上 | ✅ 窗口加载前状态已就绪 |
| 失败兜底 | 启动失败时窗口还不存在，新增 `platform::fatal`（Windows `MessageBoxW`） | ✅ 不再静默退出 |
| 残留任务收尾 | 原先在 `setup` 里 `block_on`，拖慢首帧 | ✅ 改 `spawn` 后台执行 |
| 前端兜底 | `app.load()` 对 `not managed` 类错误以 200ms 间隔重试最多 25 次，真实错误立刻上报 | ✅ |
| 验证 | 启动后日志出现 `前端引导完成 schema=1`（新增的判据日志） | ✅ 启动到引导完成约 0.9s |

**② 悬浮窗不能拖拽**

根因不是拖不动，而是拖完就被拽回去：`floating_expand/collapse` 每次都会 `monitor_anchor` 贴右下角。

| 检查 | 方法 | 结果 |
| --- | --- | --- |
| 位置模式 | 新增 `AUTO_ANCHOR`：用户拖过一次就转为自定义位置 | ✅ |
| 展开/收起 | 自定义位置下保持**右边缘与底边**不动，向左扩展 | ✅ |
| 越界保护 | `clamp_to_monitor` 限制在当前显示器内，避免拖出屏幕找不回 | ✅ |
| 持久化 | `WindowEvent::Moved` → `app_settings.floating_anchor`（物理坐标），2s 落盘一次 | ✅ 实测拖动后库中值随之更新 |
| 重启恢复 | `setup` 读回并 `set_position`，日志 `恢复悬浮窗位置 x=.. y=.. actual=..` | ✅ actual 与库中值一致 |
| 误判修正 | 事件坐标与 `outer_position()` 在不同 DPI 下可能不一致，统一回读 `outer_position()` | ✅ |
| 拖丢找回 | 托盘菜单新增「复位悬浮窗位置」，清库并回到右下角 | ✅ |

> 验证脚本用 Python 的 `GetWindowRect` 读数时，因进程无 DPI 感知会拿到逻辑坐标（与物理值差 1.5 倍），
> 不要据此判断后端存错——以日志里的 `actual` 为准。

**③ 悬浮胶囊有底色，要透明**

`background: rgba(255,255,255,0.82)` + `backdrop-filter: blur()`。后者在 Windows 透明窗口里采样不到桌面内容，
只会渲染成一块灰底。

| 修改 | 值 |
| --- | --- |
| 胶囊底色 | `rgba(255,255,255,0.82)` → `rgba(255,255,255,0.14)` |
| `backdrop-filter` | 移除 |
| 描边/分隔线 | 改为白色半透明，按钮 hover 改为白色高光 |
| 文字 | 加一层白色微光 `text-shadow`，保证透明底上仍清晰 |

**④ 工作台布局**

| 现象 | 根因 | 修复 |
| --- | --- | --- |
| 右侧「最近发布」比左侧低一截、底边不齐 | `.card + .card { margin-top: 16px }` 在双栏网格里同样生效，把第二列整体推下 16px | `.grid-2 > .card + .card { margin-top: 0 }` |
| 侧栏「数据目录」长路径被裁 | 路径 `word-break: break-all` 换行成多行，把底部区块撑出可视区 | 改单行 `ellipsis` + `title` 悬浮显示完整路径；侧栏加 `overflow-y: auto` 兜底 |



### 7.12 主内容区无法滚动、侧栏页脚被裁

**现象**：工作台「最近项目」、设置「数据与备份」两张卡片在窗口底部被切断，页面没有竖向滚动条；
左侧栏底部的「数据目录」只露出半截。

**根因**（与视觉无关，是 CSS 高度链断了）：

```
.app-shell  height: 100vh; overflow: hidden      ← 窗口高度锁死
  .main     grid 子项，min-height 默认 auto      ← 被内容撑到内容高度
    .content  flex: 1; overflow: auto            ← 父级没有高度约束，永不触发
```

grid 子项的 `min-height` 初始值是 `auto`（不是 0），所以 `.main` 会随着内容一起长高，
`.content` 从来没有"溢出"过，`overflow: auto` 形同虚设；多出来的部分被外层 `overflow: hidden` 直接切掉。

**修复**：

| 位置 | 改动 |
| --- | --- |
| `.main` | 显式 `min-height: 0` + `overflow: hidden`，把滚动交回 `.content` |
| `.sidebar` | 同样 `min-height: 0`，并改为 `overflow: hidden` |
| `.sidebar-nav`（新增） | 导航区 `flex: 1; min-height: 0; overflow-y: auto` —— 品牌与「数据目录」固定，中间导航单独滚 |
| `.brand` | `flex: none`，不被压缩 |

**实测**（Playwright + 本机 Edge，加载真实构建产物 `dist/`）：

| 视口 | 页面溢出 | 内容区可滚 | 导航可滚 | 页脚底边 / 视口高 |
| --- | --- | --- | --- | --- |
| 1050×640 | 0 | ✅ 1946 > 571 | 否（放得下） | 622 / 640 |
| 1050×480 | 0 | ✅ | ✅ | 462 / 480 |
| 900×600 | 0 | ✅ | 否 | 582 / 600 |

三项都是"页面本身不溢出、滚动发生在内容区、侧栏页脚始终完整可见"。


### 7.13 构建时弹出 cmd 黑框（子进程控制台窗口）

**现象**：点「构建」后桌面弹出一个标题为 `C:\Program Files\Volta\yarn.exe` 的黑框，
里面是构建输出，关掉它构建就中断。

**根因**：应用是 `windows_subsystem = "windows"` 的 GUI 进程，自身没有控制台。
Windows 在 GUI 进程里 `CreateProcess` 一个**控制台子系统**的可执行文件（`cmd.exe`、`yarn.exe`、
`taskkill.exe`、`uninstall.exe`）时，若没有显式指定创建标志，系统会为子进程**新建一个控制台窗口**
并显示在桌面上。把 `stdout/stderr` 接成管道并不能阻止控制台窗口被创建。

**修复**：所有控制台子进程一律带 `CREATE_NO_WINDOW`（`0x0800_0000`）：

| 位置 | 子进程 |
| --- | --- |
| `crates/buildship-build/src/process.rs` `CommandSpec::spawn` | 构建命令（`cmd.exe /d /c yarn build` 及其子孙进程） |
| `crates/buildship-build/src/process.rs` `kill_tree` | `taskkill /T /F` |
| `apps/desktop/src-tauri/src/services/maintenance.rs` `launch_uninstaller` | `uninstall.exe /S` |

**实测**（`pythonw.exe` 作为 GUI 父进程，子进程为 `cmd.exe /d /c ping`，
用 `EnumWindows` 统计 `ConsoleWindowClass` 数量）：

| 启动方式 | 控制台窗口增量 |
| --- | --- |
| 不带标志 | **+1**（就是用户看到的黑框） |
| `CREATE_NO_WINDOW` | **0** |

注意：如果父进程本身挂在某个控制台上（例如从 cmd / PowerShell 里跑 `cargo test`），
子进程会**继承**那个控制台而不再新建窗口，此时测不出差异——必须用 GUI 父进程复现。

**端到端复现**：`crates/buildship-build/examples/console_window_probe.rs` 是一个
`windows_subsystem = "windows"` 的探针程序，内部走**真实的** `CommandSpec::spawn`
启动 `cmd.exe /d /c ping`。先用 `cargo build --example console_window_probe -p buildship-build`
编译，运行期间用外部脚本采样 `ConsoleWindowClass` 数量：

| 被测对象 | 控制台窗口增量 | 结论 |
| --- | --- | --- |
| `pythonw.exe` + 子进程不带标志（反向对照） | +1 | 能正确检出黑框 |
| `console_window_probe.exe` + `CREATE_NO_WINDOW` | **0** | 通过 |

