# LibreGene — 质粒编辑器

基于 React + Vite + Tauri v2 + Rust 的桌面质粒编辑器。纯 SVG 渲染，支持多行自适应换行、分段特征、引物可视化、酶切位点标注、序列比对、插件系统。默认输出增强型 GenBank 文件（含颜色和引物注释）。

**这是 Tauri v2 桌面应用，不要用浏览器测试，必须用 `npx tauri dev` 启动。**
**已在 macOS 和 Windows 上测试过，Linux 尚未验证。**

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

## 文件结构

```
LibreGene/
├── src/                        # 前端 React 源码
│   ├── App.jsx                 # 顶层状态管理 + 路由；SequenceEditor.jsx 为核心 SVG 编辑器
│   ├── editorConstants.js      # 共享常量/工具（cw, getX, measureWidth, splitRange, location 字符串 helper）
│   ├── api.js / tauriApi.js    # HTTP/WS 客户端 / Tauri IPC 客户端
│   ├── searchUtils.js          # IUPAC 模糊搜索（含肽段→简并密码子展开）
│   ├── EditorNavMenu.jsx       # 底部导航菜单；*Dialog.jsx 为各弹窗
│   ├── plugins/                # 静态插件注册表 index.js；含 alignment/orf/primerDesign/rnaFold/codonOptimization
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
6. **除非用户明确说 commit，否则不要 commit；push 同理**

### 前端

- React 函数组件 + hooks；用 `useCallback`/`useMemo`，依赖数组完整
- 状态集中在 `App.jsx`，`SequenceEditor.jsx` 只管理 UI 状态
- JSON 字段 camelCase（Rust serde `rename_all = "camelCase"`）
- **分子类型模式**：`moleculeType`（`"dna" | "rna" | "protein"`，缺省 `"dna"`）决定编辑器形态。rna/protein 为单链：只渲染正链+特征层，导航只留 Edit/Features/Search，隐藏 DNA 专属插件（`dnaOnly: true`；RNA 用 `rnaOnly: true`）；长度单位 bp/nt/aa；protein 可直接存 `.gpt`，`.rna/.prot/.dna` 只读、必须 Save As
- **插件机制**：编译期静态注册表（`src/plugins/index.js`），不做运行时动态加载——插件引擎在 Rust 内核；外部自动化扩展走 MCP。新插件须进注册表并在设置页可禁用（localStorage `disabledPlugins`）。禁用时 sidebar 项/注册表 dialog/nav 入口都要消失；`navMenuOnly` 插件由直接接线方自行门控
- **Constants**：`cw = 12`、`startX = 220`、`baseSeqY = 100`，坐标计算依赖这些常量；`measureWidth()` 用 Canvas 2D 缓存测量（`CACHE_MAX = 2000`）

### 后端（Rust）

- **异步锁顺序**：永远先取 `window_projects` 读锁再取 `pm` 锁，反之亦然；`agent_tabs`/`mcp_opened` 锁不得与 `pm`/`window_projects` 同时持有（取前先 drop 其他 guard）。防死锁
- **重计算放 `spawn_blocking`**：酶/引物计算 CPU 密集
- **广播通知**：所有 mutation 命令调用 `broadcast_project()` 同步多窗口
- **环状序列**：region 计算注意 `% tlen` 可能为 0 导致空切片，用 `wrap_template_region` 拼接

### 多窗口

- 主窗口 label `"main"`（不在 `window_projects`）；项目窗口 `"project-{safe_id}-{timestamp}"`（注册在 `window_projects`）。MCP Agent 不再开独立窗口：`request_agent_tab` 把项目绑定为**主窗口侧边栏里的 Agent 标签**（`AppState.agent_tabs`，按 project_id 索引，记录 locked；项目留在主窗口列表并带 `agentLocked: bool|null` 字段）
- 项目窗口经 `resolve_project_id()` 按 label 查项目；`broadcast_project()` 只广播主窗口可见项目
- 窗口创建统一走 `spawn_project_window()`（仅项目窗口）；`do_delete_project` 清理 `agent_tabs`/`mcp_opened` 条目（不再有关闭窗口逻辑）

## 仍有改进空间（非 Bug）

- `SequenceEditor.jsx` ~2474 行，需拆分组件
- SVG 容器 `contain: 'layout style'` 可能影响固定定位元素
- `list_projects` JSON 构建可用序列化替代 `json!` 宏

## 待实现功能

`EditorNavMenu.jsx` 中酶切 → 自定义酶集合为占位（disabled，"即将推出"）。导航菜单用 `src/components/ui/dropdown-menu.jsx`（`@radix-ui/react-dropdown-menu`）。

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
compute_tm, get_mcp_config, set_mcp_config,
activate_custom_titlebar, reassert_traffic_lights, restore_native_titlebar
```

### HTTP API (libregene serve)

统一前缀 `http://127.0.0.1:8765`，见 `src/api.js`。

## MCP 支持

嵌入式 MCP server（`src-tauri/src/mcp.rs`）让外部 LLM Agent 像真实用户一样操作应用。

- **架构**：进程内 Streamable HTTP，绑定 `127.0.0.1:8766`（仅回环），与前端共享 `AppState` 的 `Arc<RwLock<ProjectManager>>`。所有 mutation 工具走同一套 `crate::do_*` 内核（同 recompute/dirty/broadcast 路径，UI 实时更新；Tauri command 只是薄包装）
- **启停**：`McpServer` 持配置 `{enabled, port}`；`set_mcp_config` 原进程内停止/重启（端口冲突自动重试）。默认 `enabled=true, port=8766`；配置存前端 localStorage `mcpConfig`
- **鉴权**：每请求需 `Authorization: Bearer <token>` 且 `Host` 严格等于 `127.0.0.1:<port>`（防 DNS rebinding）。令牌存 `<app_config_dir>/mcp_auth_token`；前端经 `get_mcp_token` 读、`regenerate_mcp_token` 轮换。middleware 把协议错误改写为可读 JSON-RPC 错误体（缺 `Accept` → 406/-32600；未知 session → -32001）。文件路径经 `validate_user_path` 校验（拒绝 `..` 遍历 + 扩展名白名单）
- **入口**：`src/components/McpGuideDialog.jsx`（开关 + 端口 + 令牌 + 各客户端配置片段），从侧边栏 "MCP Server" 打开
- **后台待命**：关主窗口只是隐藏（进程与 MCP 继续跑）；托盘菜单含 MCP 状态、Show、Quit；macOS Dock 图标经 `RunEvent::Reopen` 重开。项目窗口不参与
- **Agent 标签页（强制隔离）**：Agent 修改项目前必须先调 `request_agent_tab`。它把项目绑定为**主窗口侧边栏里的 Agent 标签**（`AppState.agent_tabs`，按 project_id 索引，默认 locked；已绑定则复用+重锁，返回 `reused: true`；不创建任何窗口）。**冲突策略**：只有 MCP `open_file` 打开过的项目（记录在 `AppState.mcp_opened`）才能绑定——用户打开的项目被拒绝，错误文案指引 Agent 用 bash `cp` 复制文件、`open_file` 副本后再绑定。门控：mutation 工具（`edit_sequence`/`add_feature`/`update_feature`/`add_primer`/`add_alignment`/`save_file`/`optimize_cds(apply)`/`find_orfs(add_as_features)`）对未绑定项目报错并提示先绑定；只读工具与 `open_file`/`close_project` 不受限；`activate_project` 拒绝 Agent 项目。自动重锁：`resolve_project_id`/`resolve_project`（所有工具解析项目的唯一入口）解析后调 `lock_agent_tab_for_project`——任何工具调用都把绑定标签重新锁定（仅 unlocked→locked 跃迁时 emit app 级 `agent-tab-lock` 事件，payload `{projectId, locked}`）。解锁/手动锁定走前端 `set_agent_tab_locked(projectId, locked)`；`get_projects`/`broadcast_project_arcs` 的项目列表每条带 `agentLocked: bool|null`
- **工具**：23 个（`list_projects`、`get_project_overview`、`get_region_view`、`read_sequence`、`search_sequence`、`find_restriction_sites`、`list_primers`、`open_file`、`request_agent_tab`、`save_file`、`export_subsequence`、`close_project`、`activate_project`、`edit_sequence`、`add_feature`、`update_feature`、`add_primer`、`add_alignment`、`find_orfs`、`design_primers`、`check_primer_binding`、`optimize_cds`、`convert_coordinates`）。mutation 工具统一返回 `{ok, message, projectId, regionView?}`；digest 酶切列表只列单切酶、多切酶折叠计数（`get_region_view(compact:false)` / `get_project_overview(compactCutters:false)` 得完整列表）。项目窗口 label 经 `sanitize_window_label`（非 `[A-Za-z0-9-_]` 字符全部替换为 `_`，含 `(` `)`/空格/`.` 的路径也能生成合法 label；Agent 标签按 project_id 直接索引，无需 sanitize）
- **文件优先 I/O 策略**：server `instructions` 与各工具/参数描述统一引导 Agent 用文件传序列（`open_file`/`replacement_path`/`path`/`input_path`/`output_path`/`export_subsequence`），纯文本只留给短手写输入（引物、点突变、短插入）；`read_sequence` 只作查看。改描述时保持此口径一致
- **测试**：`src-tauri` 内 `cargo test --lib` 覆盖 MCP 启停/错误体、Agent 标签门控与自动重锁、`request_agent_tab` 的复用/拒绝（非 MCP 打开项目含复制指引）/MCP 打开后可绑定、各工具正反例、digest 渲染等；digest 渲染与坐标转换在 `libregene-core` 有单元测试

### 功能 MCP 适配清单

**新增/修改功能时必须更新本清单**：每个面向用户的功能都要标注「已适配 MCP」（给工具名）或「未适配」（记原因）。新功能默认应考虑是否需要 MCP 工具。

已适配（功能 → 工具）：

- Agent 标签页（绑定后留在主窗口侧边栏、琥珀色 Bot 标识；锁定时不遮挡工作区，右侧悬浮面板 + Unlock，解锁后右下徽标 + Lock，下次 MCP 调用自动重锁）→ `request_agent_tab`（仅限 MCP `open_file` 打开的项目；用户打开的项目拒绝并指引 bash `cp` 复制副本）
- 项目/文件管理 → `open_file`（支持 gbk/gbf/gbff、dna/rna/prot、gpt 变体、fasta（.faa 按蛋白）、ab1、seq 嗅探）、`save_file`（写 .gbk/.gb/.gpt）、`close_project`、`activate_project`、`list_projects`
- 子序列导出 → `export_subsequence`（四种互斥区间：① 坐标 ② 特征 ③ 酶切或显式切口 ④ 引物扩增子；重叠特征截断+坐标平移、引物按首要位点重叠导出；环状项目导出线性）
- 序列读取 → `read_sequence`、`get_project_overview`、`get_region_view`（digest 按 molecule_type 分支；region 有比对时附 ALIGNMENT DIFFS 节 + ALIGNMENT VIEW 逐列视图：模板/掩码/read 三行，`|` 匹配 `.` 错配 `-` read 缺口，60 bp/行，插入与未覆盖区间以注记列出，覆盖窗口 >500 bp 省略视图；overview 另有 /translation 与 DNA 不一致 WARNING 行、多 read 相同 mismatch 的 MISMATCH CONSENSUS 提示）
- 序列编辑 → `edit_sequence`（字符串或 `replacement_path` 互斥；strand:"-" 先 rev-comp；携带特征/引物转移，名称冲突加 ` (2)`；`expected_old` 乐观校验失败附 `currentContent`）
- 特征 → `add_feature` / `update_feature`（结构化 1-based 参数：start+end 或 segments，互斥）
- 引物 → `add_primer`、`list_primers`、`check_primer_binding`（全位点 Tm 降序 + `alignedTemplate`/`matchMask` 尾巴覆盖）
- 引物设计 → `design_primers`（amplify/oepcr/mutagenesis；amplify 酶切尾巴 + `orientation`/`cdsOverlaps`/`internalSites`；mutagenesis 返回密码子/氨基酸自检块，氨基酸编号双口径）
- ORF 搜索 → `find_orfs`（`add_as_features` 可落库）
- 序列比对 → `add_alignment`（`bases`/`path` 双输入；默认仅**本次新增**比对回传完整明细（含 `orientedSequence`），已有比对只回统计字段（含 coverage，无差异明细与 orientedSequence）以降响应体积；`compact: true` 连新增比对的 orientedSequence 也省略并跳过 regionView；多段 coverage 段间有未覆盖模板区间时附 `coverageNote`——引擎产出的环状跨原点比对段间恒为 0 缺口，非 0 说明该模板区间未被 read 覆盖）
- IUPAC 搜索 → `search_sequence`（肽段→简并密码子展开，双链搜编码区）
- 甲基化 → 无独立工具；`get_project_overview` LOCUS 行展示；前端设置 `set_methylation`
- 限制酶切位点 → `find_restriction_sites`（未知酶名报错给近似名，可探测酶库）
- 自动标注 → `get_project_overview` 的 DETECTED COMMON FEATURES 节（只读不落库）；前端另有 `annotate_features`/`annotate_sequence`
- 密码子优化 → `optimize_cds`（项目 feature/序列/文件三输入；apply=true 仅项目模式，sequence/input_path 模式用 `output_path`）
- 坐标转换 → `convert_coordinates`（position / feature+offset / feature+aa 三种互斥输入）

上述 DNA 专属工具（`find_restriction_sites`/`find_orfs`/`design_primers`/`check_primer_binding`/`add_primer`/`add_alignment`/`search_sequence`）对 protein/rna 项目返回 isError。

未适配（前端/UI 专有，MCP 不可用）：

- **ROI**（`set_roi`/`clear_roi`）、**视图/布局设置**（layoutParams、show* 开关、酶切过滤器）：UI 视图状态
- **My Primers / My Enzymes 库**：存 localStorage，后端不可见
- **质粒图视图 / Map 水印**：纯渲染
- **前端搜索 UI**（feature/enzyme/primer 名称匹配）：MCP 只有序列搜索
- **Agent 标签的解锁按钮/悬浮面板/侧边栏 Bot 标识**：纯前端（`App.jsx`）；锁定状态后端持有，MCP 不暴露
- **选区 badge 的肽链分子量**：纯渲染层信息
- **Tm 参数与引物分析设置**：`design_primers` 已暴露浓度参数；其余为渲染层状态
- **`add_alignment` 的 createdSites**：未实现；修序列后查位点走 `edit_sequence` + `find_restriction_sites`
- **自动标注前端弹窗**、**新建序列弹窗**、**复制粘贴标注迁移**、**rnaFold 插件**（WASM 无法走 Rust 内核）、**系统文件关联打开**、**窗口内拖放文件**：纯前端/OS 集成

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
