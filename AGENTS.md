# AGENTS.md

## 项目概览
Pastoid - Tauri + React + TypeScript 构建的轻量级剪贴板管理器，支持 **macOS 和 Windows**。
两端共用同一套前端与数据格式；平台差异集中在快捷键写法、粘贴按键、托盘图标和面板窗口上。
新增平台相关代码请沿用 `cfg!(target_os = "macos")` 的方式，保持单文件可读，不要再拆出整套 cfg 分支。

## 常用命令

```bash
# 开发模式（启动前端 + Tauri）
npm run tauri dev

# 生产构建
npm run tauri build

# TypeScript 检查
npm run tsc

# Rust 测试
cd src-tauri && cargo test

# 发布版本（需要 GITHUB_TOKEN 环境变量）
npm run release           # 交互式
npm run release:patch     # 0.1.0 -> 0.1.1
npm run release:minor     # 0.1.0 -> 0.2.0
npm run release:major     # 0.1.0 -> 1.0.0
```

**重要：** 项目使用 `bun` 作为包管理器（tauri 内部调用 bun run dev/build）

## 架构

### 前端 (src/)
- **React 19 + TypeScript + Vite**（多入口构建）
- **HeroUI v3 + Tailwind CSS v4**
- **i18next** 国际化: `src/locales/{en,zh}.json`
- **Zustand** 状态管理
- 路径别名: `@/*` -> `src/*`

**窗口入口：**
- `src/settings-entry.tsx` → `index.html` → `src/app/SettingsApp.tsx`（设置/关于页面）
- `src/quick-paste-entry.tsx` → `quick-paste.html` → `src/app/QuickPaste.tsx`（快速粘贴面板）
- Vite 配置多入口: `vite.config.ts` 中 `build.rollupOptions.input`

**快速粘贴面板交互约定（重要）：**
- **选中态用 `activeId`（id），不要用下标**：置顶/删除/新条目插入都会重排列表，
  下标会让选中漂到别的卡上，回车粘错内容
- **面板打开期间冻结排序**：置顶只换图钉不重排，下次打开 `loadItems()` 时才重新排序；
  面板开着时卡片不能从指针下面跑掉
- **hover 只改选中、不滚动**；滚动只由键盘触发（`scrollNext` ref 区分），否则鼠标下的卡片会自己跑
- **hover 反馈由 React 状态控制，不用 CSS `:hover`**（`hoveredId`）：键盘换卡后要立刻收掉悬浮样式，
  否则鼠标停留的那张卡会一直显示按钮/变暗，看起来像"还选中着"；键盘操作后鼠标要移动 ≥4px 才恢复 hover
- 卡片操作按钮的**四个槽位始终渲染**（隐藏只用 opacity + pointer-events:none），
  这样图钉按钮在「只显示固定状态」和「四个按钮全显示」时位置完全一致，不会点错。
  顺序是 标签/编辑/删除/固定——图钉放最后一个槽位，单独显示时贴着右边缘，
  和按钮全显示时的位置一致（rightGap 恒为 7px）
- **图片卡片没有编辑按钮，那一格直接不渲染**（不是 opacity 0）：否则中间会留一个 20px 的空洞，
  三个按钮看起来间距不匀。图片卡片因此是 3 个槽位，头部留白也按 76px 算
- 按钮 20×20、图标 10px；卡片头部在按钮可见时留 `pr-[96px]`，否则长的时间文案会被盖住
- 操作按钮的说明气泡用自定义 tooltip，且坐标要**减去面板左上角**：
  `.pastoid-panel` 有 `backdrop-filter`，会为 fixed 子元素创建 containing block
- 面板里不再有「清空全部记录」：这个能力只在设置页（避免在快速粘贴时误清空）
- **键盘换卡用指数逼近滚动**（`animateScrollLeft`，时间常数 60ms、差值 <1.2px 时吸附），
  不要直接赋值 `scrollLeft`，也不要用 `scrollIntoView({behavior:"smooth"})`：
  原生时长不可控，连续按键会拖沓；固定起止点的补间每次按键都要重启动画，会顿。
  指数逼近只改目标值，动画顺着新目标继续走
- 右边缘留 **28px peek**：滑到最右时露出下一张的边，既提示"后面还有"，也让每按一次都有可见滑动
- **滚动容器的 `padding-right` 不计入 `scrollWidth`**：滑到最右时最后一张会贴边。
  右边距要靠滚动内容里的**末尾垫片**（`w-3 -ml-2.5`，抵消 flex gap）做进内容宽度，
  别指望容器 padding
- 滚动补间期间要**锁住 hover 选中**（`hoverLockUntil`）：卡片从静止的指针下经过会补发 `mouseover`，
  不锁的话键盘选中的卡会被抢走；开面板重置滚动位置时同样要锁
- **出入场动画**在 `src/index.css`：入场 90ms（panel-opened 事件重放 class）、退场 70ms；
  粘贴路径不走退场动画（Rust 侧 hide 后还有 Cmd+Tab 的等待），`prefers-reduced-motion` 下归零
- 横向卡片条的**滚轮映射**用原生 `wheel` 监听（`passive: false`），React 的 onWheel 拦不住页面滚动
- 面板定位在 `shortcut.rs::position_panel`：按光标选屏 + `work_area` 对齐底部，
  隐藏窗口的 `current_monitor()` 是上次的位置，直接用会弹错屏
- **主题 = 语义 token 覆盖，不动组件结构**：`html[data-style="<id>"]` + `.light/.dark`，
  面板/卡片的形状走 `--panel-*` / `--card-*`（见 `src/index.css` 的 `.pastoid-panel` / `.pastoid-card`）
- **外观 = 7 套风格 + 3 个明暗按钮**（自动/浅/深）：风格只保差异最大的 7 套。
  Material 3 / 柔雾 / 云母 / 学院与已有风格太像，玻璃拟态在无系统 vibrancy 时只是半透明，
  五套已从 `THEMES` 移除（前四套 CSS token 还在，恢复加一行；glass 的 token 已删）
- `backdrop-filter` 在透明窗口里**没有可模糊的内容**，别再用它假装毛玻璃；
  真玻璃需要窗口级 vibrancy（macOS NSVisualEffectView / Windows Acrylic）
- 内层控件圆角用 `--control-radius` / `--chip-radius`（由 `--card-radius` 派生），
  不要写死 `rounded-md`——否则粗野/侘寂会出现"外方内圆"
- 对比度改动后跑 `npm run check:themes`：校验每套风格两版齐全、核心 token 齐全、
  正文/次要/主色/选中环/搜索底全部达到阈值
- 加新风格时两版都要写：结构（圆角/边框/阴影/blur）保持一致，只换颜色，
  否则切换明暗时会像换了一套主题
- `modes` 现在全是 `["light","dark"]`，留着是能力声明；`paint()` 的单档兜底仍然保留
- **明暗是两层**：CSS 层靠 `html.dark/.light` 切 token；原生层靠 `getCurrentWindow().setTheme(null|light|dark)`。
  两个窗口（面板/设置）都要设，只改 class 会让原生滚动条、输入控件停在旧明暗
- CSS 里 `html{color-scheme:light}` / `.dark{color-scheme:dark}`，否则浏览器自绘部分不跟主题
- `setTheme` 是异步的，media query 之后才更新：`applyTheme` 先画一次、promise 落地再画一次，
  另有 rAF + 120ms 兜底
- `Settings.theme` 保留但统一写 `system`，只为兼容旧配置文件
- **编辑正文会撞内容唯一索引**：`repository.update` 返回 `Result`，命令把错误透传给前端，
  前端显示"内容与已有条目重复"，不能假装保存成功

### 后端 (src-tauri/)
- **Tauri 2 (Rust)**
- **rust-i18n** 托盘菜单国际化: `src-tauri/locales/app.yml`
- 核心文件:
  - `src/lib.rs` - 应用初始化、插件注册
  - `src/commands.rs` - Tauri IPC 命令（剪贴板操作、设置管理、图片读取）
  - `src/clipboard.rs` - 剪贴板监控逻辑（文本 + 图片，按内容哈希去重）
  - `src/tray.rs` - 系统托盘菜单（显示快捷键 accelerator）
  - `src/storage.rs` - 数据目录、日志轮转、孤儿图片回收（历史数据的读写在 db.rs）
  - `src/db.rs` - 历史库（SQLite）：schema、JSON 迁移、上限裁剪
  - `src/models.rs` - 数据结构定义
  - `src/shortcut.rs` - 全局快捷键注册/注销、健康监控、面板定位
  - `src/repository.rs` - ClipboardRepository（SQLite 写穿，单条操作即时落盘）

**历史数据存储约定（重要）：**
- 历史存 `history.db`（rusqlite bundled，自带 SQLite 编译，无系统依赖），**SQLite 是唯一数据源**：
  单条操作即时落盘（写穿），没有内存全量缓存、没有定时快照、没有 flush 线程——
  旧的「全量序列化 + 覆写」是 O(N)，条目一多就是卡顿源，而且 `fs::write` 非原子，崩溃截断会丢历史
- `clipboard_data.json` **只当迁移源和备份：永远不写、不删、不改名**。
  `PRAGMA user_version >= 1` 表示已迁移过；迁移在 `ClipboardRepository::new` 里跑：
  单事务导入、按 id/内容去重，成功后才写 version
- JSON 读失败 / 解析失败都**不写 version**（下次启动重试）；解析失败时原文件保持不动，
  日志里给出路径供手动恢复——别把「解析失败」和「读失败」混成一个分支
- 去重靠唯一索引 `ux_items_dedup (type, content_hash)`，`content_hash = blake3(内容)`：
  最长 512KB 的原文不能当索引键；图片另有 `image_hash` 唯一索引
- `max_items = 0` 表示不限（无限保存）：条数裁剪跳过，`get_history` 也不拼 LIMIT
  （`LIMIT 0` 会永远返回空）；非 0 时裁剪置顶优先保留，被砍掉的图片条目同步删文件
- 图片有独立磁盘护栏 `db::MAX_IMAGES = 1000`：图片条目超过 1000 张按 `created_at DESC` 保留最新，
  删行 + 删文件（`enforce_image_limit`，由 `enforce_max_items` 在条数裁剪后调用，
  `max_items == 0` 的分支也调）——文本可以不限，图片不限会把磁盘吃光
- **带 tag 的条目永不参与滚动删除**：`db::TAGGED_PREDICATE` 把它们从条数裁剪和图片护栏里排除。
  打 tag 是明确的用户投入，复制新内容时不能把它挤掉。
  注意 tagged **仍然计入** `max_items` 名额（上限 3 且 1 条 tagged → 只留 2 条普通），
  只有 pinned/tagged 本身就超过上限时总数才会突破上限
- **搜索同时命中正文和 tag**：`repository.list` 用 `json_each` 展开 tags 再 LIKE，
  给条目打上 "work" 之后搜 "work" 能捞出来
- **调低上限 / 关闭「不限」会删数据**：`set_max_items` 在 `new_max > 0 && (old_max == 0 || new_max < old_max)`
  时立刻裁剪（0 是无穷大，所以 0→n 也要裁）。设置页提交前必须用 `preview_trim_history`
  算一遍会删多少条并弹确认，用户取消就保持原设置
- **`#` 前缀是标签专用查询**：`#work` 只匹配 tags、不碰正文（否则正文里的 "work" 会污染结果）；
  前端输入 `#` 时用 `get_clipboard_tags` 的建议列表补全，点卡片上的标签 chip 也会切到 `#tag`
- 标签以 JSON 数组存在 `items.tags`（`["work","客户A"]`），空数组等价于没打标签
- **按标签批量删除**（`db::delete_by_tag`）：`json_each.value = ?` 精确匹配，不是 LIKE；
  图片条目同步删磁盘文件。这是用户明确要求的删除，所以不做"tagged 保护"，
  但设置页必须先弹确认（不可恢复）
- 数据目录可能在 exFAT / 网络盘上：开库时尝试 WAL，PRAGMA 退回原模式就改用回滚日志 + `synchronous=FULL`
- `images/`、`settings.json`、`location.txt` 与历史库无关，数据目录搬家逻辑原样生效

**剪贴板采集约定（重要）：**
- 文本按「内容 + 类型」去重，图片按 `imageHash`（blake3）去重
- 图片文件名直接使用哈希（`images/<hash>.png`），同一张图不会在磁盘上出现第二份
- **先判重、再落盘**：只有确定新增时才写 PNG。历史上的 bug 就是每轮轮询都另存一张新图，
  21 小时写了 5.9 万个文件（29GB），把历史冲垮，看起来像「复制记录不生效」
- 用户删除/清空时会把内容键记进 `AppState.suppressed`，避免剪贴板里残留的同一条内容被重新加回来；
  一旦剪贴板内容真的变了，抑制表整体清空（否则旧键越积越多，语义也难推理）
- 启动时后台跑一次 `cleanup_orphan_images`，回收没有被任何条目引用的图片文件

**采集优先级与轮询（踩过坑，别改回去）：**
- 文本轮询 250ms，位图探测单独限流到 1 秒。位图 `get_image()` 每次都要把剪贴板解码成 RGBA，
  跟着文本一起加密会白烧 CPU；但文本轮询太慢会漏记——实测 1 秒轮询下间隔 250ms 连续复制的 10 条只记到 3 条
- **文本优先于位图**：剪贴板同时带文本和位图很常见（浏览器复制图片会附带 URL/HTML）。先取位图会让用户
  复制的文本凭空消失；只有当文本是 URL / data URI / HTML 片段这类「复制图片的副产物」时才让位给图片
- 超长文本（>512KB）、超大图片（>40MP 或 >64MB）会被跳过，但**必须写一条 WARN 日志**，
  否则用户只会看到「复制了却没进历史」而完全无从排查

**关键插件：**
- `tauri-plugin-global-shortcut` — 全局快捷键（呼出 quick-paste）
- `tauri-plugin-updater` — 官方自动更新（签名验证 + 自动安装重启）
- `tauri-plugin-autostart` — 开机自启动（macOS AppleScript 模式）
- `tauri-nspanel` — macOS NSPanel（quick-paste 覆盖全屏应用）
- `tauri-plugin-clipboard-manager` — 剪贴板读写
- `tauri-plugin-notification` — 系统通知

## 国际化规范

**前端新增文本:**
1. 同时添加到 `src/locales/en.json` 和 `src/locales/zh.json`
2. React 中使用 `t('key')`

**后端托盘新增文本:**
1. 添加到 `src-tauri/locales/app.yml` 的 `tray:` 下
2. Rust 中使用 `t!("tray.key")`

## 发布流程
- `release-it` 自动同步 3 处版本: `package.json` → `src-tauri/Cargo.toml` → `src-tauri/tauri.conf.json`
- 自动生成基于 conventional commits 的 CHANGELOG
- 自动创建 GitHub Release（需要 `GITHUB_TOKEN` 环境变量）
- CI 自动构建并上传签名更新包（需要 `TAURI_SIGNING_PRIVATE_KEY` Secret）

## 构建产物
- macOS: `.app` + `.dmg`（x64 + aarch64 双架构分别构建）
- Windows: `.msi` + `.exe`（NSIS）
- 更新包: `.tar.gz` + `.sig` 签名文件

## 代码约定
- TypeScript strict 模式
- Rust edition 2021
- 使用 Git 提交信息规范（conventional commits）

## 平台差异速查

| 关注点 | macOS | Windows |
| --- | --- | --- |
| 默认快捷键 | `Cmd+Shift+V` | `Ctrl+Shift+V`（`normalize_shortcut` 负责 Cmd→Ctrl） |
| 粘贴 | Cmd+Tab 切回原应用后 `Key::Unicode('v')` | 直接 Ctrl+V，且必须用 `Key::V`（见下） |
| 面板窗口 | `tauri-nspanel` 转 NSPanel，浮在全屏应用之上 | 普通 always-on-top 窗口 + 所有虚拟桌面可见 |
| 托盘图标 | `icons/tray-template/32x32.png`（纯黑模板图，`icon_as_template(true)`，深色菜单栏自动变白） | `icons/tray/32x32.png`（彩色品牌图标，模板图在 Windows 不反色） |
| 面板底部留白 | 110pt（避开 Dock） | 60pt（避开任务栏） |
| 数据目录 | `~/Library/Application Support/<id>` | `%LOCALAPPDATA%\<id>`（不用 Roaming，避免漫游同步图片） |
| 安装包 | `.dmg` | `.msi` / NSIS `.exe` |

Windows 上「复制文件」会以多行路径（有时带引号）出现在剪贴板文本里，
`clipboard::first_image_path` 负责逐行去引号后识别，别退化回「整段当路径」。

**enigo 的坑**：Windows 上 `Key::Unicode(c)` 无法映射成虚拟键，enigo 会打一条 warn 后
降级成「文本输入」（`queue_char`），Ctrl 和字符变成两条独立事件，应用收不到 Ctrl+V，
表现是回车粘贴没反应、或者往编辑器里插了个 `v`。Windows 必须用平台专属的虚拟键
（`Key::V` / `Key::A` …），它们带 `#[cfg(target_os = "windows")]`，用 cfg 分平台取。

**数据目录与迁移**：
- 默认目录由 `storage::default_data_dir` 决定；用户可在设置里改成任意路径，
  记录方式是默认目录下的 `location.txt`（第一行目标路径，可选第二行 `from=<旧目录>`）
- 切换目录一律延后到下次启动：仓库、图片目录、监控线程都持有旧路径，运行中切换没有意义
- 启动时 `get_data_dir` 先解析指针，再把 `from=` 指定的旧目录搬过去，然后清掉 `from=` 那行
- 自定义目录不可用（移动硬盘没插）时回落默认目录，指针保留，等磁盘插回来再切回去
- `get_data_dir` 在 logger 初始化之前执行，所以它把要打的提示作为返回值带出来，别在里面直接 log
- 首次运行会弹出数据目录引导（标记文件 `.first-run-done`；已指定过目录则不再问），
  并在 `lib.rs` 里把设置窗口叫出来

**托盘图标分平台**：
- macOS 用模板图（纯黑 + alpha，`icon_as_template(true)`），系统按菜单栏明暗染色——深色栏白、浅色栏黑。
- Windows 用彩色版（`icon_as_template(false)`），模板图在 Windows 不会反色。
- 不要把彩色图开 template：系统只取 alpha，紫色会被染成单色、图案整个消失只剩色块（试过，见 git 历史）。
- 母版是矢量：`src-tauri/icons/source/char.svg`（角色）和 `wordmark.svg`（字标），由 `python3 scripts/build-icons.py` 生成全部尺寸。：板子 + 夹子 + 两条内容线 + 左侧两个点，只是粗细按 16px 调过（1.75/16）。

## macOS 权限（辅助功能 / 自动化）

- 自动粘贴靠 CGEvent 合成 ⌘V，需要「辅助功能」授权；没有权限时 `paste_item`
  直接返回 `NEED_ACCESSIBILITY`，不再去调 enigo（否则每次粘贴都弹系统框），
  面板里会显示「去授权」提示条
- 开机自启用 `MacosLauncher::LaunchAgent`，不碰 AppleScript，不需要「自动化」权限
- **TCC 按代码签名认应用**：ad-hoc 签名每次构建 cdhash 都变，更新后旧授权条目失效，
  用户必须在「隐私与安全性」里删掉旧条目再加一次。要跨版本保留授权，必须用固定证书签名
  （`APPLE_SIGNING_IDENTITY` + CI 里的 `APPLE_CERTIFICATE` secrets）

## 发布

- `npm run release:patch|minor` 走 release-it；需要 `GITHUB_TOKEN=$(gh auth token)`
- 国内网络偶尔连不上 github.com，`git config http.version HTTP/1.1` 后重试

## 改完代码必跑

| 命令 | 什么时候 |
| --- | --- |
| `cargo test`（`src-tauri/`） | 改了 Rust（默认跑，逻辑层） |
| `npm run smoke` | 改了窗口 / 面板 / NSPanel / 快捷键 / 托盘 —— 启动 app 连续开合面板 40 次，检查进程不崩、无 panic、无新 crash report |
| `npm run build` | 改了前端（`tsc` + `vite build`） |

单测抓不到平台生命周期问题（比如 NSPanel 的 dealloc 崩溃），所以 `smoke` 不能省。

## 容易踩的坑

- **NSPanel**：`window.to_panel()` 每次调用都会新建 `RawNSPanel` 并覆盖管理器里的引用，
  旧 panel 被 dealloc 后 AppKit 还在用 → 主线程 SIGBUS。只在 setup 调一次，
  之后一律用 `app.get_webview_panel("quick-paste")`
- **CI 签名**：`APPLE_SIGNING_IDENTITY` 即使是空字符串也会被 tauri 当成「指定了身份」，
  `codesign` 会失败。只有证书存在时才注入这些环境变量
- **workflow 改动**：先 `gh workflow run release.yml --ref <branch>` 干跑（产物是草稿），
  确认能出包再打正式 tag

## 代码签名（让 TCC 授权跨版本保留）

- 本机证书：`~/.tauri/pastoid-selfsigned/{cert.pem,key.pem}`（CN `Pastoid Self Signed`，
  有效期 10 年，私钥 600 权限）；本机测试时用它签，见下
- GitHub secrets：`APPLE_CERTIFICATE`（cert.pem 的 base64）、`APPLE_KEY`（key.pem 的 base64）、
  `APPLE_SIGNING_IDENTITY=Pastoid Self Signed`；配好后 Release 会自动签名
- 这台 macOS 的 `security import` 不认 PKCS12（PKCS#12 MAC 校验失败），
  所以 CI 里证书和私钥分开用 PEM 导入，不要再改回 p12
- 验证签名：`codesign -dv --verbose=2 Pastoid.app` 看 `Authority`；
  `codesign -d -r- Pastoid.app` 应包含 `certificate root = H"af7e..."`（跨版本固定）

本地签名构建：

```bash
export APPLE_SIGNING_IDENTITY="Pastoid Self Signed"
export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/pastoid.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="<更新密钥密码>"
npm run tauri build -- --target aarch64-apple-darwin
```

## 发版必须由用户明确下令

- **没有用户明确的「发版 / 发布 / release」指令，不得**：跑 `npm run release:*`、
  打 tag、`gh release create/edit/delete`、推送 tag、群发公众号
- 日常改动只提交（可选推送 `main`），版本号不动
- 即使用户说「改完发版」也要先确认这次改动是否已经验证通过；发版前必须跑
  `cargo test` + `npm run smoke` + `npm run build`
