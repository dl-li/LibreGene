# LibreGene — 质粒编辑器

基于 React + Vite + Tauri v2 + Rust 的桌面质粒编辑器。纯 SVG 渲染，支持多行自适应换行、分段特征、引物可视化、酶切位点标注、序列比对、插件系统。默认输出增强型 GenBank 文件（含颜色和引物注释）。

**这是 Tauri v2 桌面应用，不要用浏览器测试，必须用 `npx tauri dev` 启动。**
**已在 macOS、Windows、Linux（Fedora aarch64, Wayland）上测试过。**

## 技术栈

- **前端**: React 19 + Vite 8，shadcn v4 (Radix UI)，lucide-react 图标，Tailwind CSS v4
- **渲染**: 纯 SVG，Cascadia Code / TeX Gyre Heros 字体
- **后端**: Rust (edition 2021), tokio 1, gb-io 0.9；Tauri v2 壳内嵌 libregene-core
- **测试**: 无测试框架（前端无测试；后端仅 Rust 单元测试 + 集成测试）
- **License**: GPL-3.0-only

## 开发命令

```bash
tmux new-session -d -s libregene 'npx tauri dev'   # 后台启动桌面应用
tmux attach -t libregene                            # 查看输出（Ctrl-B D 分离）
npx vite build                 # 仅前端编译检查
npm run format                 # Prettier 格式化 src/
npm run lint                   # ESLint 检查 src/

cd backend
cargo test -p libregene-core --lib                  # 单元测试
cargo test -p libregene-core --test roundtrip_test  # 读写往返测试
cd src-tauri && cargo build                         # 构建 Tauri 后端（须在 src-tauri 目录执行）

npx shadcn add <component>
```

## Linux 调试（SSH 远程虚拟机）

被要求"在 Linux 上启动调试"时，按以下步骤操作（目标机为 Fedora aarch64 虚拟机，SSH 已通）：

1. **同步代码**：`rsync -az --exclude=node_modules --exclude=target --exclude=dist --exclude=.git ./ user@host:LibreGene/`（首次还需在虚拟机装系统依赖 webkit2gtk4.1-devel 等 + rustup，并 `npm install`）
2. **启动**（必须强制 Wayland 后端，否则自定义标题栏激活失败出现双标题栏）：

   ```bash
   ssh user@host 'cd ~/LibreGene && . ~/.cargo/env && \
     XDG_RUNTIME_DIR=/run/user/1000 WAYLAND_DISPLAY=wayland-0 GDK_BACKEND=wayland \
     nohup npx tauri dev > tauri-dev.log 2>&1 &'
   ```

3. **后续改动**：前端改动重新 rsync 后 Vite 热更新即生效；`src-tauri` 改动自动重编；`backend/libregene-core` 改动需 `touch src-tauri/src/*.rs` 触发
4. 远程 kill 进程时 `pkill -f` 的模式别写成会匹配到自己 SSH 命令行的字符串（会误杀自身 shell）

## Release 流程

push 到 master 时，CI（`.github/workflows/build.yml` 的 `release` job）检查 `package.json` 的 version 对应 tag `v<version>` 是否已存在；不存在则自动创建 GitHub Release 并附各平台安装包：macOS（dmg/app.tar.gz）、Windows（msi/nsis）、Linux（仅 Flatpak，amd64 + aarch64 两个包；manifest 在 `flatpak/`，不再产出 deb/AppImage）。发布步骤：

1. bump `package.json` 的 version（`tauri.conf.json` 的 version 引用它，无需另改）
2. 手写 `release-notes/v<version>.md` 作为 release note（缺失时 CI 回退为 GitHub 自动生成 notes）
3. 合并到 master 即可

## 文件结构

```
LibreGene/
├── src/                        # 前端 React 源码
│   ├── App.jsx                 # 顶层状态管理 + 路由；SequenceEditor.jsx 为核心 SVG 编辑器
│   ├── editorConstants.js      # 共享常量/工具（cw, getX, measureWidth, splitRange, location 字符串 helper）
│   ├── api.js / tauriApi.js    # HTTP/WS 客户端 / Tauri IPC 客户端
│   ├── searchUtils.js          # IUPAC 模糊搜索（含肽段→简并密码子展开）
│   ├── EditorNavMenu.jsx       # 底部导航菜单；*Dialog.jsx 为各弹窗
│   ├── plugins/                # 静态插件注册表 index.js；含 alignment/orf/primerDesign/rnaFold/codonOptimization/blast
│   └── components/ui/          # shadcn UI 组件
├── backend/libregene-core/src/ # Rust 核心库（models/project、orf、search、codon、digest、enzyme/、primer/、file_io/）
└── src-tauri/src/
    ├── lib.rs                  # Tauri commands + AppState + 共享 do_* 内核 + 系统托盘
    └── mcp.rs                  # 嵌入式 MCP server（工具 + 启停控制）
```

## 编码准则

### 通用

- **尽量不写注释**——代码本身表意清晰；必要时写简短注释说明 Why。
- 先读后改；改完必须编译/构建验证：前端 `npx vite build`，后端 `cargo test -p libregene-core --lib`，Rust 改动另跑 `cd src-tauri && cargo build`（backend workspace 根跑 `cargo build -p LibreGene` 会报 package 不匹配）。
- **tauri dev 只监听 `src-tauri/`**：改 `backend/libregene-core` 不会触发重编译，需 `touch src-tauri/src/*.rs` 手动触发。

### Bug 修复流程

1. 修复前用 `git status` + `git log --oneline -5` 确认状态
2. **每修一个 Bug 单独提交一次**（`git add` 只含相关文件）
3. 提交前跑对应测试和构建
4. 提交信息用英文，格式：`fix: 简短描述` 或 `refactor: 简短描述`
5. 涉及 UI 的改动在 tmux 中的 Tauri dev 验证

### 前端

- React 函数组件 + hooks；用 `useCallback`/`useMemo`，依赖数组完整
- 状态集中在 `App.jsx`，`SequenceEditor.jsx` 只管理 UI 状态
- JSON 字段 camelCase（Rust serde `rename_all = "camelCase"`）
- **分子类型模式**：`moleculeType`（`"dna" | "rna" | "protein"`，缺省 `"dna"`）决定编辑器形态。rna/protein 为单链：只渲染正链+特征层，导航只留 Edit/Features/Search，隐藏 DNA 专属插件（`dnaOnly: true`；RNA 用 `rnaOnly: true`）；长度单位 bp/nt/aa；protein 可直接存 `.gpt`，`.rna/.prot/.dna` 只读、必须 Save As
- **插件机制**：编译期静态注册表（`src/plugins/index.js`），不做运行时动态加载——插件引擎在 Rust 内核；外部自动化扩展走 MCP。新插件须进注册表并在设置页可禁用（localStorage `disabledPlugins`）。禁用时 sidebar 项/注册表 dialog/nav 入口都要消失；`navMenuOnly` 插件由直接接线方自行门控
- **Constants**：`cw = 12`、`startX = 220`、`baseSeqY = 100`，坐标计算依赖这些常量；`measureWidth()` 用 Canvas 2D 缓存测量（`CACHE_MAX = 2000`）

### 后端（Rust）

- **异步锁顺序**：永远先取 `pm` 锁再取 `window_projects` 读锁；`agent_tabs` 锁不得与 `pm`/`window_projects` 同时持有（取前先 drop 其他 guard）。防死锁
- **重计算放 `spawn_blocking`**：酶/引物计算 CPU 密集
- **广播通知**：所有 mutation 命令调用 `broadcast_project()` 同步多窗口
- **环状序列**：region 计算注意 `% tlen` 可能为 0 导致空切片，用 `wrap_template_region` 拼接

### 多窗口

- 主窗口 label `"main"`（不在 `window_projects`）；项目窗口 `"project-{safe_id}-{timestamp}"`（注册在 `window_projects`）。MCP Agent 不再开独立窗口：MCP `open_project` 打开文件时把项目绑定为**主窗口侧边栏里的 Agent 标签**（`AppState.agent_tabs`，按 project_id 索引，记录 locked；项目留在主窗口列表并带 `agentLocked: bool|null` 字段）
- 项目窗口经 `resolve_project_id()` 按 label 查项目（映射未命中=项目被驱逐，返回错误提示 reload，绝不回退主窗口 active）；`broadcast_project()` 只广播主窗口可见项目
- 窗口创建统一走 `spawn_project_window()`（仅项目窗口）；`do_delete_project` 清理 `agent_tabs` 条目并关闭绑定到被删项目的项目窗口（防幽灵 webview）

## 仍有改进空间（非 Bug）

- `SequenceEditor.jsx` ~5200 行，需拆分组件
- SVG 容器 `contain: 'layout style'` 可能影响固定定位元素
- `list_projects` JSON 构建可用序列化替代 `json!` 宏

## API

### Tauri Commands

```
get_project, get_project_by_id, open_file, take_pending_opens, create_project, save_file, write_text_file,
update_sequence, set_roi, clear_roi,
get_features, add_feature, delete_feature, update_feature_ftype/color/name/strand/location,
get_primers, add_primer, add_primers, delete_primer, check_primers_binding, compute_primer_alignment,
design_primer_candidates, find_orfs, search_sequence, annotate_features, annotate_sequence,
list_codon_species, preview_codon_optimization, apply_codon_optimization, get_enzyme_database,
add_alignment, add_alignment_seq, remove_alignment, set_methylation,
get_projects, activate_project, delete_project, open_in_new_window, get_window_project_id, rekey_project,
get_agent_tab_state, set_agent_tab_locked,
compute_tm, blast_submit, get_mcp_config, set_mcp_config,
activate_custom_titlebar, reassert_traffic_lights, restore_native_titlebar, force_quit
```

### HTTP API (libregene serve)

统一前缀 `http://127.0.0.1:8765`，见 `src/api.js`。

## MCP 支持

嵌入式 MCP server（`src-tauri/src/mcp.rs`）让外部 LLM Agent 像真实用户一样操作应用。

- **架构**：进程内 Streamable HTTP，绑定 `127.0.0.1:8766`（仅回环），与前端共享 `AppState` 的 `Arc<RwLock<ProjectManager>>`。所有 mutation 工具走同一套 `crate::do_*` 内核（同 recompute/dirty/broadcast 路径，UI 实时更新；Tauri command 只是薄包装）
- **启停/入口**：默认 `enabled=true, port=8766`，配置存 localStorage `mcpConfig`；侧边栏 "MCP Server" 打开 `McpGuideDialog.jsx`（开关/端口/令牌/自动生成的 Agent 配置提示词）。关主窗口只是隐藏，进程与 MCP 继续跑；托盘 Quit 遇未保存改动先经前端确认再走 `force_quit`
- **鉴权**：每请求需 `Authorization: Bearer <token>` 且 `Host` 严格等于 `127.0.0.1:<port>`（防 DNS rebinding）；令牌存 `<app_config_dir>/mcp_auth_token`；文件路径经 `validate_user_path` 校验（拒绝 `..` + 扩展名白名单）
- **Agent 标签页（强制隔离）**：MCP `open_project` = 加载 + 绑定为**主窗口侧边栏 Agent 标签**（`AppState.agent_tabs`，默认 locked，不开窗口）。已绑定则复用+重锁；已加载未绑定（用户项目）则拒绝，指引 Agent 用 bash `cp` 复制副本再打开。mutation 工具对未绑定项目报错；任何工具调用自动重锁标签（统一入口 `resolve_project_id`/`resolve_project`/`resolve_project_light`，后者 clone 时置空 enzymes 减负）；解锁走前端 `set_agent_tab_locked`；项目列表每条带 `agentLocked: bool|null`
- **工具**：18 个——`list_projects`、`get_project_overview`、`get_region_view`、`read_sequence`、`search_sequence`、`find_restriction_sites`、`list_primers`、`open_project`、`save_file`、`close_project`、`edit_sequence`、`set_feature`、`add_primer`、`add_alignment`、`find_orfs`、`design_primers`、`check_primer_binding`、`optimize_cds`。`project_id` 必填（无 active 回退）；mutation 工具统一返回 `{ok, message, projectId, regionView?}`。**各工具的参数与行为细节以 `mcp.rs` 内工具描述为准，不在本文件重复**
- **文件优先 I/O**：工具描述统一引导 Agent 用文件传序列（`path`/`replacement_path`/`input_path`/`output_path`），纯文本只留给短输入（引物、点突变、短插入）；改描述时保持此口径一致
- **测试**：`src-tauri` 内 `cargo test --lib` 覆盖 MCP 启停/错误体、Agent 标签绑定/门控/重锁、各工具正反例与 digest 渲染

### 功能 MCP 适配清单

**新增/修改功能时必须更新本清单**：标注「已适配」（给工具名）或「未适配」（记原因）。已适配工具的参数语义见 `mcp.rs` 工具描述。

已适配（功能 → 工具）：

- 项目/文件管理、Agent 标签绑定、子序列导出（`region`） → `open_project` / `save_file` / `close_project` / `list_projects`
- 序列读取、坐标转换、自动标注（只读展示）、甲基化展示 → `read_sequence` / `get_project_overview` / `get_region_view`
- 序列编辑 → `edit_sequence`；特征 → `set_feature`
- 引物 → `add_primer` / `list_primers` / `check_primer_binding`；引物设计 → `design_primers`
- ORF → `find_orfs`；序列比对 → `add_alignment`；IUPAC 搜索 → `search_sequence`；酶切位点 → `find_restriction_sites`；密码子优化 → `optimize_cds`
- 上述 DNA 专属工具（`find_restriction_sites`/`find_orfs`/`design_primers`/`check_primer_binding`/`add_primer`/`add_alignment`/`search_sequence`）对 protein/rna 项目返回 isError

未适配（前端/UI 专有，MCP 不可用）：

- **ROI**、**视图/布局设置**（layoutParams、show* 开关、酶切过滤器、特征标签位置）：UI 视图状态
- **My Primers / My Enzymes 库**：存 localStorage，后端不可见
- **质粒图视图 / Map 水印 / RNA 折叠水印**（foldWatermark 与 mapWatermark 互斥，localStorage + Tauri 广播同步）、**选区 badge 分子量**：纯渲染
- **前端搜索 UI**（feature/enzyme/primer 名称匹配）：MCP 只有序列搜索
- **Agent 标签解锁按钮/导航控制条**：纯前端；锁定状态后端持有，MCP 不暴露
- **Tm 参数与引物分析设置**：`design_primers` 已暴露浓度参数；其余为渲染层状态
- **`add_alignment` 的 createdSites**：未实现；修序列后查位点走 `edit_sequence` + `find_restriction_sites`
- **自动标注弹窗**、**新建序列弹窗**、**复制粘贴标注迁移**、**rnaFold 插件**（WASM 无法走 Rust 内核）、**系统文件关联/窗口拖放打开**：纯前端/OS 集成
- **BLAST 插件**（右键选区 → `blast_submit`）：交互式外网操作，Agent 场景意义不大

## 核心模型约定

- **坐标分层**（基数按层划分，非全局统一）：
  - **Agent 可见面（MCP 工具 + digest 渲染）：1-based inclusive**。集中转换点：mcp.rs `to1`/`from1` 及 `*_1based` helper；digest.rs `cut_flanks`/`cut_notation`（pub，mcp.rs 复用）
  - **内部模型与 Tauri command IPC：0-based inclusive**（前端内部状态同层，只在渲染处 +1）。`Feature.start/end`、`Feature.segments[]`、`PrimerBindingSite.template_start` 0-based inclusive；`PrimerBindingSite.template_end` 0-based exclusive（数值恰等于 1-based inclusive 末端，MCP 只对 `templateStart` +1）；`Enzyme.cut_index/bot_cut_index` 在 0-based cutIndex-1 与 cutIndex 之间（渲染 `N^N+1`，环状原点切口 `len^1`）
  - **前端 UI 渲染与输入：1-based inclusive**，内部保持 0-based，只在边界转换（渲染处直接 +1；用户输入的 location 字符串发送前经 `locationStringTo0based` 转 0-based）。location helper 集中在 `src/editorConstants.js`：`locationString1based`/`locationString0based`/`locationStringTo0based`
  - **GenBank 落盘/解析：1-based**（外部文件格式）。1-based 解析只在 `gbk.rs::parse_location_string`（文件用）；App 内 location 字符串走 `parse_location_string_0based`
- 模型坐标 0-based inclusive；gb-io Range 是 0-based end-exclusive
- `ProjectData.molecule_type` — `"dna" | "rna" | "protein"`（serde 输出 `moleculeType`），默认 `"dna"`；RNA/蛋白序列通常线性
- **分子类型 gate**：digest 渲染、酶/引物 recompute、translate refresh 都按 molecule_type 分支，非 DNA 跳过酶切/引物/甲基化（`ProjectData::is_dna()` 统一判定，空串视为 DNA）；auto-annotation 例外（DNA 走 nt 级 + CDS 蛋白级双通路，protein 按 aa 匹配，RNA 不支持）
- 环状序列坐标用 `% tlen` 归一化，`wrap_template_region` 负责环状拼接
- **跨原点特征**：segments 按 join 顺序存储，后段 `start` < 前段 `start` 即跨原点（如 CmR `join(8886..9326,1..219)`）。特征选中范围取 join 顺序首段 start..末段 end，故 `selStart > selEnd` 表示跨原点选区（仅 circular 有效；渲染/复制支持，replace/delete/paste 不支持）；复制/导出序列按 join 顺序拼接，负链按逆序逐段 rev-comp（前端 helper 在 `src/editorConstants.js`：`featureSelRange`/`sliceRange`/`rangeLen`；图谱经 `MapView.jsx` 的 `unwrapRuns` 展开为单个跨原点箭头）
