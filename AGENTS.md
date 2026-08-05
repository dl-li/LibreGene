# LibreGene — 质粒编辑器

基于 React + Vite + Tauri v2 + Rust 的桌面质粒编辑器。纯 SVG 渲染，支持多行自适应换行、分段特征、引物可视化、酶切位点标注、序列比对、插件系统。默认输出增强型 GenBank 文件（含颜色和引物注释）。

**这是 Tauri v2 桌面应用，不要用浏览器测试，必须用 `npx tauri dev` 启动。**
**已在 macOS 和 Windows 上测试过，Linux 尚未验证。**

## 技术栈

- **前端**: React 19 + Vite 8，shadcn v4 (Radix UI)，lucide-react 图标，Tailwind CSS v4
- **渲染**: 纯 SVG，Cascadia Code / TeX Gyre Heros 字体
- **后端**: Rust (edition 2021), tokio 1, gb-io 0.9
- **桌面壳**: Tauri v2，内嵌 libregene-core
- **无测试框架**（前端无测试，后端仅 Rust 单元测试 + 集成测试）
- **License**: GPL-3.0-only

## 开发命令

```bash
tmux new-session -d -s libregene 'npx tauri dev'   # 后台启动桌面应用
tmux attach -t libregene                            # 查看输出（Ctrl-B D 分离）
npx vite build                 # 仅前端编译检查

# 代码质量
npm run format                 # Prettier 格式化所有 src/
npm run lint                   # ESLint 检查所有 src/

# 后端
cd backend
cargo test -p libregene-core --lib             # 单元测试（全部）
cargo test -p libregene-core --test roundtrip_test      # 读写往返测试
cd src-tauri && cargo build           # 构建 Tauri 后端（须在 src-tauri 目录执行）

# shadcn
npx shadcn add <component>
```

## 文件结构

```
LibreGene/
├── index.html                  # Vite 入口，含 @font-face 定义
├── vite.config.js              # Vite 配置（es2022 target, vendor chunk, @ alias）
├── package.json                # React 19, Vite 8, shadcn v4, lucide-react
├── components.json             # shadcn 配置 (new-york style, neutral base)
├── jsconfig.json               # 路径别名 (@/ → src/)
├── assets/Fonts/               # 10 个字体文件 (Cascadia Code, TeX Gyre Heros/Termes)
├── examples/                  # 公开合成测试序列 (.gbk)
├── src/                        # 前端 React 源码
│   ├── main.jsx                # 入口，ReactDOM.createRoot
│   ├── App.jsx                 # 顶层状态管理，Sidebar + 路由
│   ├── SequenceEditor.jsx      # 核心编辑器：SVG 渲染、选择/光标、酶/引物/特征渲染
│   ├── editorConstants.js      # 共享常量与工具函数（cw, getX, measureWidth, splitRange）
│   ├── editHistory.js          # 撤销/重做历史栈
│   ├── api.js                  # HTTP/WebSocket API 客户端（非 Tauri 模式）
│   ├── tauriApi.js             # Tauri IPC API 客户端
│   ├── searchUtils.js          # IUPAC 模糊搜索匹配引擎
│   ├── FeatureInfoDialog.jsx   # 特征编辑弹窗
│   ├── SequenceEditDialog.jsx  # 序列编辑（插入/删除/替换）确认弹窗
│   ├── PrimerAlignmentDialog.jsx # 引物添加/编辑弹窗
│   ├── FeatureScrollbar.jsx    # 特征颜色滚动条
│   ├── ErrorBoundary.jsx       # React Error Boundary
│   ├── EditorNavMenu.jsx       # 底部居中悬浮导航菜单（编辑/特征/引物/酶切/比对/搜索）
│   ├── plugins/                # 插件系统：index.js 注册表，每个插件 { id, name, dialogKey, sidebarItems, dialog }
│   │   └── alignment/          # 序列比对插件（管理弹窗 + 文本新增弹窗）
│   │   └── orf/                # ORF 搜索插件（无弹窗；扫描逻辑在 Rust 端 `find_orfs`，侧边栏开关切换 showOrfs；ORF 以 orf:true 的虚拟 CDS 注入，仅展示不落盘）
│   │   └── primerDesign/       # 引物设计插件（Amplify/OE-PCR/PCR Mutagenesis；候选引物生成在 Rust 端 `design_primer_candidates`，PrimerDesignDialog.jsx 参数+候选弹窗；由 EditorNavMenu 直接接线，不走注册表）
│   ├── fileIcons.js            # 文件名 → lucide 图标映射
│   ├── components/
│   │   ├── DebugPanel.jsx      # 调试面板
│   │   ├── PrimerOverviewDialog.jsx  # 引物总览弹窗
│   │   ├── SettingsPage.jsx    # 设置页面
│   │   ├── TitleBar.jsx        # 拖拽标题栏（tauri-plugin-decoration 提供原生 macOS 红绿灯 / Windows overlay 控件）
│   │   └── ui/                 # shadcn UI 组件
│   ├── hooks/
│   │   └── use-mobile.js       # 移动端断点检测（768px）
│   └── lib/
│       └── utils.js            # cn() 工具（clsx + tailwind-merge）
├── backend/                    # Rust 后端 (workspace)
│   ├── Cargo.toml
│   ├── libregene-core/         # 核心库
│   │   └── src/
│   │       ├── lib.rs          # 模块导出
│   │       ├── models.rs       # 数据模型
│   │       ├── project.rs      # ProjectManager
│   │       ├── utils.rs        # complement / reverse_complement
│   │       ├── orf.rs          # ORF 搜索（find_orfs，双链三框，返回虚拟 CDS Feature）
│   │       ├── search.rs       # IUPAC 模糊序列搜索（find_seq_matches，双链）
│   │       ├── digest.rs       # MCP 文本摘要渲染（project_digest / read_sequence，含单元测试）
│   │       ├── enzyme/         # 酶切引擎
│   │       ├── primer/         # 引物引擎（design.rs 为引物设计候选生成）
│   │       └── file_io/        # 文件解析/序列化
│   └── test_data/
└── src-tauri/                  # Tauri v2 桌面壳
    ├── Cargo.toml
    ├── tauri.conf.json
    └── src/
        ├── lib.rs              # Tauri commands + AppState + 共享 do_* 内核
        ├── mcp.rs              # 嵌入式 MCP server（LibreGeneMcp 工具 + McpServer 启停控制）
        └── main.rs             # 入口
```

## 编码准则

### 通用

- **尽量不写注释**——代码本身应该表意清晰。必要时写简短注释说明 Why（不是 What）。
- 先读后改：改任何文件前，先 `Read` 理解上下文。
- 改完后必须编译/构建验证。前端：`npx vite build`。后端：`cargo test -p libregene-core --lib`。
- Rust 代码同时跑 `cd src-tauri && cargo build` 确保 Tauri 壳也编译（backend workspace 根目录跑 `cargo build -p LibreGene` 会报 package 不匹配）。
- 默认已通过 `tmux` 在后台运行 `npx tauri dev`，改 UI 后切到 tmux 看效果即可。

### Bug 修复流程

1. 开始修复前先用 `git status` + `git log --oneline -5` 确认当前状态
2. **每修一个 Bug 就单独提交一次**（`git add` 只包含相关的改动文件）
3. 提交前跑对应的测试和构建
4. 提交信息用英文，格式：`fix: 简短描述` 或 `refactor: 简短描述`
5. 涉及 UI 的改动用 tmux 中的 Tauri dev 验证
6. **除非用户明确说 commit，否则不要 commit；除非用户明确说 push，否则不要 push**（不要自作主张提交或推送）

### 前端

- **React 函数组件 + hooks**，无 class 组件（ErrorBoundary 除外）
- 用 `useCallback` 包裹传递给子组件的函数，依赖数组必须完整
- 用 `useMemo` 缓存计算开销大的派生数据
- 状态管理集中到 `App.jsx`，`SequenceEditor.jsx` 只管理 UI 状态（选择、光标、弹窗）
- 所有 JSON 字段使用 camelCase（Rust 端 serde `rename_all = "camelCase"`）
- `EMPTY_ARRAY = []` 作为共享空数组引用，避免重复创建
- 引用类型用 `useRef`，跨渲染保持引用稳定性
- 操作计数器 `operationGenRef` + `switchGenRef` 防止异步请求交叉污染

#### Constants（editorConstants.js）

- `cw = 12`（字符宽度 px），`startX = 220`，`baseSeqY = 100`
- 所有坐标计算依赖这四个常量
- `measureWidth()` 使用 Canvas 2D 缓存测量，`CACHE_MAX = 2000`

### 后端（Rust）

- **异步锁的顺序**：永远先获取 `window_projects` 读锁再获取 `pm` 锁，反之亦然。防止死锁。
- **重计算在 spawn_blocking 里做**：酶和引物的计算是 CPU 密集的，必须用 `tokio::task::spawn_blocking`。
- **广播通知**：所有 mutation 命令都需要调用 `broadcast_project()` 以同步多窗口。
- **坐标约定**：模型坐标是 0-based inclusive；gb-io Range 是 0-based end-exclusive。
- **引物模型**：`template_start` 0-based inclusive，`template_end` 0-based exclusive。
- **环状序列**：Window/region 计算时注意 `% tlen` 可能产生 0，导致空切片。用 `wrap_template_region` 做环状拼接。

### 关于多窗口的注意事项

- 主窗口 label 是 `"main"`（不在 `window_projects` map 里）
- 项目窗口 label 是 `"project-{safe_id}-{timestamp}"`
- 项目窗口通过 `resolve_project_id()` 按 label 查找项目
- `broadcast_project()` 只广播主窗口可见的项目（排除项目窗口拥有的）

## 仍有改进空间的地方（非 Bug）

- **SequenceEditor.jsx ~2474 行** — 需拆分组件（如 FeatureLayer、PrimerLayer、EnzymeLayer 等）
- **SVG 容器 `contain: 'layout style'`** — 创建新层叠上下文，可能影响固定定位元素
- **`list_projects` JSON 构建** — 可用序列化替代 `serde_json::json!` 宏

## 待实现功能（导航菜单占位）

`EditorNavMenu.jsx` 中以下菜单项为占位（disabled，标注"即将推出"）：

- 酶切：自定义酶集合

导航菜单使用 `src/components/ui/dropdown-menu.jsx`（基于 `@radix-ui/react-dropdown-menu`，通过 shadcn 方式添加）。

## API

### Tauri Commands

```
get_project, get_project_by_id, open_file, save_file, write_text_file,
update_sequence, set_roi, clear_roi,
get_features, add_feature, delete_feature,
update_feature_ftype, update_feature_color, update_feature_name,
update_feature_strand, update_feature_location,
get_primers, add_primer, add_primers, delete_primer, check_primers_binding,
compute_primer_alignment, design_primer_candidates, find_orfs, search_sequence,
get_enzyme_database,
add_alignment, add_alignment_seq, remove_alignment,
set_methylation,
get_projects, activate_project, delete_project,
open_in_new_window, get_window_project_id, rekey_project,
compute_tm, get_mcp_config, set_mcp_config,
activate_custom_titlebar, reassert_traffic_lights, restore_native_titlebar
```

### HTTP API (libregene serve)

统一前缀 `http://127.0.0.1:8765`，见 `src/api.js`。

## MCP 支持

嵌入式 MCP（Model Context Protocol）服务器让外部 LLM Agent 可以像真实用户一样操作应用。

- **架构**：MCP server 运行在 Tauri 进程内（`src-tauri/src/mcp.rs`），Streamable HTTP 绑定 `127.0.0.1:8766`（仅回环），与前端共享 `AppState` 的 `Arc<RwLock<ProjectManager>>`。
- **共享内核**：所有 mutation 工具与对应 Tauri command 走同一套 `crate::do_*` 内部函数（`src-tauri/src/lib.rs`），同一 recompute/dirty/broadcast 路径，UI 实时更新。Tauri command 只是薄包装。
- **启停控制**：`McpServer`（`mcp.rs`）持有配置 `{enabled, port}` 与 server task；`set_mcp_config` 在原进程内停止/重启服务器（端口冲突时自动重试），无需重启应用。默认 `enabled=true, port=8766`。配置持久化在前端 localStorage（key `mcpConfig`），启动时前端调用 `set_mcp_config` 应用。
- **鉴权与错误响应**：每个 MCP 请求必须带 `Authorization: Bearer <token>` 且 `Host` 严格等于 `127.0.0.1:<port>`（防本地进程与浏览器 DNS rebinding），由 axum middleware 检查，不符返回 401。令牌持久化在 `<app_config_dir>/mcp_auth_token`，重启 app 不更换；前端经 `get_mcp_token` 读取，`regenerate_mcp_token` 手动轮换（立即生效，无需重启 server——middleware 每请求读共享令牌）。同一 middleware 还把协议层错误改写为可读的 JSON-RPC 错误体：缺少/错误 `Accept` 头返回 406 + code -32600（message 说明 POST 需 `Accept: application/json, text/event-stream`、GET 需 `text/event-stream`）；rmcp 的纯文本 404 "Session not found" 被改写为 code -32001（message 说明 session 已失效、需重新 initialize）。`open_file`/`save_file`/`write_text_file`（含 MCP 侧复用路径）经 `validate_user_path` 校验：拒绝 `..` 遍历并限制扩展名白名单。
- **会话健壮性**：session 存于内存（`LocalSessionManager`），不绑定 TCP 连接，连接断开不会关闭 session；rmcp 默认的 `SessionConfig.keep_alive` 空闲超时仅 5 分钟（LLM agent 两次工具调用之间的思考可能超过），`serve_mcp` 已调大为 24 小时；SSE keep-alive ping 从默认 15 s 缩短到 3 s（低于常见本地代理 ~4 s 的空闲断连阈值，长连接流不会被代理掐断）。
- **入口**：`src/components/McpGuideDialog.jsx`（启用开关 + 端口 + 令牌显示/刷新 + 各客户端配置片段，片段内含 Authorization header），从侧边栏 "MCP Server" 菜单项和 Empty 界面 "Connect an LLM agent via MCP" 链接打开。
- **工具**：目前 22 个工具（`list_projects`、`get_project_overview`、`get_region_view`、`read_sequence`、`search_sequence`、`get_enzyme_database`、`find_restriction_sites`、`list_primers`、`open_file`、`save_file`、`close_project`、`activate_project`、`edit_sequence`、`add_feature`、`update_feature`、`add_primer`、`set_methylation`、`add_alignment`、`remove_alignment`、`find_orfs`、`design_primers`、`check_primer_binding`）。mutation 工具统一返回 `{ok, message, projectId, regionView?}`，regionView 为 digest 渲染的编辑后区域摘要（compact 模式：酶切列表折叠为单行计数，避免完整酶切列表 56-82KB 撑爆输出；`get_region_view` 默认 compact，可传 `compact: false` 得完整列表；`get_project_overview` 的 UNIQUE CUTTERS 段默认折叠为一行计数，可传 `compactCutters: false` 得完整列表）。查 Tm 用 `check_primer_binding`（返回全部结合位点：每个 primer 含 `bindingSiteCount`、best-first（Tm 降序）的 `sites` 数组与兼容字段 `site`（= 最佳位点，无位点时为 null），位点字段 strand/templateStart/templateEnd/tm/annealLen/mismatchedTail；另有恒返回的顶层 `tmBasis` 字符串说明 Tm/annealLen 口径：按 3' 端实际连续匹配，尾巴碱基意外匹配模板会拉长退火区），不用单独的 compute_tm（已移除——裸数字返回值不符合 MCP structuredContent 规范）。`read_sequence` 除文本标尺外另返回机器可读的 `sequence` 纯碱基字段；`add_primer`/`check_primer_binding` 的结合位点含 `annealLen`（3' 端连续匹配长度，退火核心），`check_primer_binding` 另含 `mismatchedTail`（5' 端未退火碱基数，binds=true 仅代表 3' 退火核心结合）；`design_primers` amplify 模式恒返回 `internalSites`（空数组表示无内部酶切位点，非空时附 `warning`）；digest 酶切列表只列单一切口酶，多切酶汇总为一行计数。`design_primers` 与 `check_primer_binding` 的 annealLen/Tm 口径不同：design 只算设计退火区，check 算 3' 端实际连续匹配（尾巴与模板 MCS 连续匹配时 check 报的 annealLen/Tm 更高，详见 `tmBasis`）。
- **坐标约定（MCP 工具）**：0-based inclusive；primer `template_end` exclusive；酶切在 `pos-1` 与 `pos` 之间；环状序列读取支持 `start > end` 绕原点，编辑区间不允许绕原点（`end = start - 1` 为纯插入）。
- **测试**：`src-tauri` 内 `cargo test --lib` 有 McpServer 启停/换端口测试（mock runtime，真实 TCP 握手）、缺少 Accept 头的 406 JSON-RPC 错误体测试、未知 session 的 404 -32001 结构化错误测试；`check_primer_binding` 全位点返回有单元测试；digest 渲染在 `libregene-core` 有单元测试。

### 功能 MCP 适配清单

**新增/修改功能时必须更新本清单**：每个面向用户的功能都要明确标注「已适配 MCP」（并给出对应工具名）或「未适配」。新功能默认应考虑是否需要 MCP 工具；决定不适配时也在清单中记一笔原因。

已适配（功能 → MCP 工具）：

- 项目/文件管理（打开/保存/关闭/切换）→ `open_file`、`save_file`、`close_project`、`activate_project`、`list_projects`
- 序列读取 → `read_sequence`（返回文本标尺 + 机器可读 `sequence` 纯碱基字段）、`get_project_overview`（UNIQUE CUTTERS 段默认折叠为一行计数，可传 `compactCutters: false` 得完整列表）、`get_region_view`（默认 compact，酶切列表折叠为单行计数，可传 `compact: false` 得完整列表）
- 序列编辑（插入/删除/替换）→ `edit_sequence`（带 `expected_old` 乐观校验，失败时返回首个差异索引与 ±20 bp 对照上下文；会按 delta 平移/裁剪特征坐标，完全落在删除区间的特征被移除；恒返回 `removedFeatures`/`clippedFeatures` 回显编辑副作用：removed 为被整体删除的特征 `{name, ftype, location}`（移除前 0-based start..end），clipped 为坐标被裁剪（非整体平移）的特征 `{name, ftype, before, after}`）
- 特征新增与更新 → `add_feature`（恒返回顶层 `featureId`，另含 `{ok, message, projectId, regionView}`）、`update_feature`（单一工具：`feature_id` + 可选 `name/ftype/color/strand/location`，至少一项；location 为 GenBank 1-based 字符串，message 回显存储后的 0-based 坐标）
- 引物新增 → `add_primer`（返回重算后结合位点，含 `annealLen` 退火核心长度；名称冲突报错会指明冲突对象是已有引物还是已有特征）
- 引物清单 → `list_primers`（名称/序列/结合位点数/位点坐标，只读）
- 引物结合检查 / Tm 查询 → `check_primer_binding`（返回全部结合位点：`bindingSiteCount` + best-first（Tm 降序）`sites` 数组（strand/templateStart/templateEnd/tm/annealLen/mismatchedTail），兼容字段 `site` = 最佳位点、无位点时为 null；`annealLen` 为 3' 端实际连续匹配长度，`mismatchedTail` 为 5' 端未退火碱基数，binds=true 仅指 3' 退火核心结合；恒返回顶层 `tmBasis` 字符串说明 Tm/annealLen 按 3' 端实际连续匹配重算，尾巴碱基意外匹配模板会拉长退火区、Tm 高于 design 值）
- 引物设计（Amplify/OE-PCR/Mutagenesis）→ `design_primers`（amplify 支持 `fwd_enzyme`/`rev_enzyme` 酶切尾巴 + `protect_bases` 保护碱基，amplify 恒返回 `internalSites`（空数组表示无内部酶切位点，非空时附 `warning`）；mutagenesis 校验 `mut_seq` 与 seg 等长且差异 ≤3 bp，返回 `mutation` 自检块含正/负链上下文与 CDS 密码子/氨基酸变化——支持 join 分段 CDS，`cds.codonIndex` 为 CDS 内 0-based、`cds.aaPosition1Based` 为 1-based 氨基酸位置（含起始 Met）、`cds.aaPositionExcludingMet` 为不含 Met 的位置（aaPosition1Based - 1，首个密码子时省略），全碱基替换时附 `warning` 提示确认正链）
- ORF 搜索 → `find_orfs`（`add_as_features` 可直接落库）
- 序列比对（Sanger reads / 序列）→ `add_alignment`（`bases`/`path` 双输入，`path` 支持 .gbk/.dna/.fasta/.ab1；返回差异明细：`mismatchDetails` 每个 mismatch 的 0-based 模板位置与模板/读段碱基、`deletionDetails` 每个 deletion 的位置/长度/缺失碱基（跨环状原点自动合并）、`insertionDetails` 每个 insertion 的插入位点（pos-1 与 pos 之间）与序列/长度；另有 `identity`（全精度不四舍五入）与 `alignedLength`（覆盖模板长度）；成功响应恒附带 `alignments` 数组——该项目当前全部比对（每个含 alignmentId/name/identity/strand/segmentCount/alignedLength/mismatches/insertions/deletions 及差异明细），无需单独只读工具即可查看所有已存比对；短读段被拒时错误信息说明原因与最小长度阈值 50 bp）、`remove_alignment`
- IUPAC 序列搜索 → `search_sequence`
- 甲基化设置 → `set_methylation`
- 酶数据库查询 → `get_enzyme_database`
- 限制酶切位点查询 → `find_restriction_sites`（按名称列出识别位点与切口：`recStart`/`recEnd` 0-based inclusive、`recSeq`、识别链 `top`/`bottom`、`cuts` 每个 `topCutIndex`/`botCutIndex`（切口在 cut-1 与 cut 之间）；复用已算好的引擎结果，环状坐标已归一化，不传 `enzymes` 返回全部，未知酶名报错并给出近似名）

未适配（前端/UI 专有，MCP 不可用）：

- **ROI（感兴趣区域）**：`set_roi`/`clear_roi` 只有 Tauri command，属 UI 视图状态
- **My Primers / My Enzymes 库**：`myPrimers.js`/`myEnzymes.js` 存 localStorage，后端不可见
- **质粒图视图（Plasmid Map）**：纯渲染
- **前端搜索 UI**（`searchUtils.js` 的 feature/enzyme/primer 名称匹配）：MCP 侧只有序列搜索
- **多窗口管理**（`open_in_new_window` 等）：UI 窗口概念，Agent 用 `activate_project` 切换即可
- **视图/布局设置**（layoutParams、showFeatures/Primers/Enzymes 开关、酶切过滤器）：渲染层状态
- **Tm 参数与引物分析设置**（`tmParams`、`primerSeedLength`）：前端设置项；MCP 工具内用默认浓度，暂未暴露参数
- **`add_alignment` 的 createdSites（新建酶切位点）**：未实现——`add_alignment` 不修改模板序列，创建位点需按差异重建「编辑后序列」并独立于已算好的引擎结果重新扫酶数据库（反向链/环状合并下重建语义与 alignment 差异表示耦合，语义复杂、价值有限）；如需实际修序列后查位点，走 `edit_sequence` + `find_restriction_sites`


## 核心模型约定

- `Feature.start/end` — 0-based inclusive
- `Feature.segments[]` — 分段特征的多段列表，每个 `{ start, end }` 0-based inclusive
- `PrimerBindingSite.template_start` — 0-based inclusive
- `PrimerBindingSite.template_end` — 0-based exclusive
- `Enzyme.cut_index / bot_cut_index` — 切口在 cutIndex-1 与 cutIndex 之间，0-based
- `BindingSite.matchStart/End` — inclusive
- 环状序列坐标用 `% tlen` 归一化，`wrap_template_region` 负责处理环状拼接

