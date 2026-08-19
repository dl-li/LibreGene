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
│   ├── App.jsx                 # 顶层状态管理，Sidebar + 路由
│   ├── SequenceEditor.jsx      # 核心编辑器：SVG 渲染、选择/光标、酶/引物/特征渲染
│   ├── editorConstants.js      # 共享常量与工具函数（cw, getX, measureWidth, splitRange）
│   ├── editHistory.js          # 撤销/重做历史栈
│   ├── api.js / tauriApi.js    # HTTP/WS 客户端（非 Tauri）/ Tauri IPC 客户端
│   ├── searchUtils.js          # IUPAC 模糊搜索引擎（查询含核苷酸字母表外字符时按肽段展开为简并密码子）
│   ├── EditorNavMenu.jsx       # 底部悬浮导航菜单（编辑/特征/引物/酶切/比对/搜索）
│   ├── *Dialog.jsx             # 各弹窗（特征/序列编辑/新建序列/引物比对等）
│   ├── plugins/                # 插件系统：index.js 注册表 { id, name, description, version, dialogKey, dnaOnly, rnaOnly, sidebarItems, dialog, navMenuOnly }
│   │   ├── alignment/          # 序列比对（管理弹窗 + 文本新增弹窗）
│   │   ├── orf/                # ORF 搜索（引擎在 Rust `find_orfs`，ORF 以 orf:true 虚拟 CDS 注入，仅展示不落盘）
│   │   ├── primerDesign/       # 引物设计（引擎 `design_primer_candidates`；navMenuOnly 半插件：选区交互由 SequenceEditor/EditorNavMenu 直接接线，注册表仅提供元数据与禁用开关）
│   │   ├── rnaFold/            # RNA 二级结构预测（ribossfold-wasm 前端 WASM 折叠 + fornac 力图渲染，均动态 import；rnaOnly；navMenuOnly：入口为导航栏 Folding 按钮）
│   │   └── codonOptimization/  # 密码子优化（引擎 `codon.rs`，三命令封装在 tauriApi.js）
│   └── components/ui/          # shadcn UI 组件
├── backend/libregene-core/src/ # Rust 核心库
│   ├── models.rs / project.rs  # 数据模型 / ProjectManager
│   ├── orf.rs / search.rs      # ORF 搜索 / IUPAC 模糊搜索（双链）
│   ├── codon.rs                # 密码子优化引擎（含 codon_usage.tsv 9 物种表）
│   ├── digest.rs               # MCP 文本摘要渲染（project_digest / read_sequence）
│   ├── enzyme/ / primer/       # 酶切引擎 / 引物引擎（design.rs 为引物设计候选生成）
│   └── file_io/                # 文件解析/序列化（gbk 变体、dna/rna/prot、gpt 变体、fasta 变体、ab1、seq 嗅探）
└── src-tauri/src/
    ├── lib.rs                  # Tauri commands + AppState + 共享 do_* 内核 + OS 文件打开事件 + 系统托盘
    └── mcp.rs                  # 嵌入式 MCP server（LibreGeneMcp 工具 + McpServer 启停控制）
```

## 编码准则

### 通用

- **尽量不写注释**——代码本身应该表意清晰。必要时写简短注释说明 Why（不是 What）。
- 先读后改：改任何文件前，先 `Read` 理解上下文。
- 改完后必须编译/构建验证。前端：`npx vite build`。后端：`cargo test -p libregene-core --lib`，Rust 改动另跑 `cd src-tauri && cargo build`（backend workspace 根目录跑 `cargo build -p LibreGene` 会报 package 不匹配）。
- 默认已通过 `tmux` 在后台运行 `npx tauri dev`，改 UI 后切到 tmux 看效果即可。
- **tauri dev 只监听 `src-tauri/`**：改 `backend/libregene-core` 不会触发重编译，需 `touch src-tauri/src/*.rs` 手动触发。

### Bug 修复流程

1. 开始修复前先用 `git status` + `git log --oneline -5` 确认当前状态
2. **每修一个 Bug 就单独提交一次**（`git add` 只包含相关的改动文件）
3. 提交前跑对应的测试和构建
4. 提交信息用英文，格式：`fix: 简短描述` 或 `refactor: 简短描述`
5. 涉及 UI 的改动用 tmux 中的 Tauri dev 验证
6. **除非用户明确说 commit，否则不要 commit；除非用户明确说 push，否则不要 push**

### 前端

- **React 函数组件 + hooks**，无 class 组件（ErrorBoundary 除外）
- **分子类型模式**：`moleculeType`（`"dna" | "rna" | "protein"`，缺省 `"dna"`）决定编辑器形态。rna/protein 为单链模式：只渲染正链 + 特征层，不渲染互补链/引物/酶切/ORF/比对层，导航菜单只保留 Edit/Features/Search（protein 另隐藏 Copy Antisense/Translation 与 Reverse Complement），侧边栏隐藏 DNA 专属插件（注册表用 `dnaOnly: true` 标记；对称地 RNA 专属插件用 `rnaOnly: true`）；长度单位 bp/nt/aa；protein 项目可直接保存 `.gpt`，`.rna/.prot/.dna` 只读打开、必须 Save As。
- 用 `useCallback` 包裹传递给子组件的函数，依赖数组必须完整；用 `useMemo` 缓存开销大的派生数据
- 状态管理集中到 `App.jsx`，`SequenceEditor.jsx` 只管理 UI 状态（选择、光标、弹窗）
- 所有 JSON 字段使用 camelCase（Rust 端 serde `rename_all = "camelCase"`）
- `EMPTY_ARRAY = []` 作为共享空数组引用；引用类型用 `useRef` 保持跨渲染引用稳定
- 操作计数器 `operationGenRef` + `switchGenRef` 防止异步请求交叉污染
- **插件机制（设计决策）**：插件为编译期静态注册表（`src/plugins/index.js`），刻意不做运行时动态加载/单文件分发——插件引擎在 Rust 内核，有意义的扩展总要随本体发布；外部自动化扩展走 MCP。新插件以合入主线的方式添加，须进注册表并在设置页可禁用（localStorage `disabledPlugins`，SettingsPage 勾选框）。禁用时 sidebar 项、注册表 dialog 及 nav 菜单入口都要消失；`navMenuOnly` 插件（如 primerDesign）由直接接线方自行读取禁用状态门控。

#### Constants（editorConstants.js）

- `cw = 12`（字符宽度 px），`startX = 220`，`baseSeqY = 100`，所有坐标计算依赖这些常量
- `measureWidth()` 使用 Canvas 2D 缓存测量，`CACHE_MAX = 2000`

### 后端（Rust）

- **异步锁的顺序**：永远先获取 `window_projects` 读锁再获取 `pm` 锁，反之亦然。防止死锁。
- **重计算在 spawn_blocking 里做**：酶和引物的计算是 CPU 密集的，必须用 `tokio::task::spawn_blocking`。
- **广播通知**：所有 mutation 命令都需要调用 `broadcast_project()` 以同步多窗口。
- **环状序列**：Window/region 计算时注意 `% tlen` 可能产生 0 导致空切片，用 `wrap_template_region` 做环状拼接。

### 关于多窗口的注意事项

- 主窗口 label 是 `"main"`（不在 `window_projects` map 里）；项目窗口 label 是 `"project-{safe_id}-{timestamp}"`
- 项目窗口通过 `resolve_project_id()` 按 label 查找项目
- `broadcast_project()` 只广播主窗口可见的项目（排除项目窗口拥有的）

## 仍有改进空间的地方（非 Bug）

- **SequenceEditor.jsx ~2474 行** — 需拆分组件（如 FeatureLayer、PrimerLayer、EnzymeLayer 等）
- **SVG 容器 `contain: 'layout style'`** — 创建新层叠上下文，可能影响固定定位元素
- **`list_projects` JSON 构建** — 可用序列化替代 `serde_json::json!` 宏

## 待实现功能（导航菜单占位）

`EditorNavMenu.jsx` 中以下菜单项为占位（disabled，标注"即将推出"）：酶切：自定义酶集合。

导航菜单使用 `src/components/ui/dropdown-menu.jsx`（基于 `@radix-ui/react-dropdown-menu`）。

## API

### Tauri Commands

```
get_project, get_project_by_id, open_file, take_pending_opens, create_project, save_file, write_text_file,
update_sequence, set_roi, clear_roi,
get_features, add_feature, delete_feature,
update_feature_ftype, update_feature_color, update_feature_name,
update_feature_strand, update_feature_location,
get_primers, add_primer, add_primers, delete_primer, check_primers_binding,
compute_primer_alignment, design_primer_candidates, find_orfs, search_sequence,
annotate_features, annotate_sequence,
list_codon_species, preview_codon_optimization, apply_codon_optimization,
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

嵌入式 MCP 服务器让外部 LLM Agent 可以像真实用户一样操作应用。

- **架构**：运行在 Tauri 进程内（`src-tauri/src/mcp.rs`），Streamable HTTP 绑定 `127.0.0.1:8766`（仅回环），与前端共享 `AppState` 的 `Arc<RwLock<ProjectManager>>`。
- **共享内核**：所有 mutation 工具与对应 Tauri command 走同一套 `crate::do_*` 内部函数，同一 recompute/dirty/broadcast 路径，UI 实时更新。Tauri command 只是薄包装。
- **启停控制**：`McpServer` 持有配置 `{enabled, port}` 与 server task；`set_mcp_config` 原进程内停止/重启（端口冲突自动重试）。默认 `enabled=true, port=8766`。配置持久化在前端 localStorage（key `mcpConfig`），启动时前端调用 `set_mcp_config` 应用。
- **鉴权**：每个请求必须带 `Authorization: Bearer <token>` 且 `Host` 严格等于 `127.0.0.1:<port>`（防 DNS rebinding），不符返回 401。令牌持久化在 `<app_config_dir>/mcp_auth_token`；前端经 `get_mcp_token` 读取，`regenerate_mcp_token` 手动轮换（立即生效）。middleware 还把协议错误改写为可读 JSON-RPC 错误体（缺 `Accept` 头 → 406 / -32600；未知 session → -32001）。文件路径经 `validate_user_path` 校验：拒绝 `..` 遍历并限制扩展名白名单。
- **会话健壮性**：session 存内存不绑定 TCP 连接；`keep_alive` 空闲超时调大为 24 小时（rmcp 默认仅 5 分钟）；SSE keep-alive ping 缩短到 3 s（防本地代理掐断长连接）。
- **入口**：`src/components/McpGuideDialog.jsx`（开关 + 端口 + 令牌 + 各客户端配置片段），从侧边栏 "MCP Server" 或 Empty 界面链接打开。
- **后台待命（close-to-tray）**：关闭主窗口只是隐藏（拦截 `CloseRequested`），进程与 MCP server 继续运行；托盘菜单含 MCP 状态行、Show、Quit；macOS 点 Dock 图标经 `RunEvent::Reopen` 重开。项目窗口（`project-*`）不参与。
- **工具**：21 个（`list_projects`、`get_project_overview`、`get_region_view`、`read_sequence`、`search_sequence`、`find_restriction_sites`、`list_primers`、`open_file`、`save_file`、`export_subsequence`、`close_project`、`activate_project`、`edit_sequence`、`add_feature`、`update_feature`、`add_primer`、`add_alignment`、`find_orfs`、`design_primers`、`check_primer_binding`、`optimize_cds`）。mutation 工具统一返回 `{ok, message, projectId, regionView?}`，regionView 为编辑后区域 digest 摘要。digest 中酶切列表只列单切酶、多切酶折叠计数；`get_region_view` 默认 compact（`compact: false` 得完整酶切列表），`get_project_overview` 的 UNIQUE CUTTERS 同理用 `compactCutters: false`。
- **坐标约定（MCP 工具）**：MCP 工具输入/输出与 digest 渲染全部为 **1-based inclusive**（GenBank 惯例）；primer `templateEnd` 同为 1-based inclusive（数值与内部 0-based exclusive 末端恰好相等，只有 `templateStart` 需 +1）；酶切写作 `N^N+1` = 断在 1-based 碱基 N 与 N+1 之间（环状原点切口为 `len^1`）；环状读取支持 `start > end` 绕原点，编辑区间不允许绕原点（纯插入 = `start=N, end=N-1`，即插到碱基 N 之前）。`add_feature`/`update_feature` 用结构化 1-based 参数（`start`+`end` 或 `segments: [{start, end}]`，互斥）。比对的 `coverage` 为 [{start, end}] 1-based inclusive（环状跨原点分多段），`orientedSequence` 已按模板方向归一化（strand "-" 已 rev-comp），与 `mismatchDetails` 的坐标口径一致。
- **文件优先 I/O 策略**：server `instructions`（`#[tool_handler(..., instructions = ...)]`）与各工具/参数描述统一引导 Agent 用文件传序列（`open_file`/`replacement_path`/`path`/`input_path`/`output_path`/`export_subsequence`），纯文本参数仅留给短手写输入（引物、点突变、短插入）；`read_sequence` 只作查看。改描述时保持此口径一致。
- **测试**：`src-tauri` 内 `cargo test --lib` 覆盖 McpServer 启停/换端口、406/-32001 错误体、`check_primer_binding` 全位点与尾巴覆盖字段（`alignedTemplate`/`matchMask`）、`design_primers` amplify 的 `orientation`/`cdsOverlaps` 与 mutagenesis 整密码子替换警告分级、`export_subsequence` 正反例、`add_feature`/`update_feature` 结构化 1-based 参数正反例、`add_alignment` 的 `orientedSequence`/`coverage`（正/反链、环状跨原点分段）与 `get_region_view` 差异节窗口过滤；digest 渲染（含 read_sequence 小窗口标尺紧凑化）与 `template_coverage`/mutagenesis 分析在 `libregene-core` 有单元测试。

### 功能 MCP 适配清单

**新增/修改功能时必须更新本清单**：每个面向用户的功能都要明确标注「已适配 MCP」（给出工具名）或「未适配」。新功能默认应考虑是否需要 MCP 工具；决定不适配时也记一笔原因。

已适配（功能 → MCP 工具）：

- 项目/文件管理 → `open_file`（打开文件成为项目并返回 `projectId`，之后用 `project_id` 引用）、`save_file`、`close_project`、`activate_project`、`list_projects`。支持 gbk/gbf/gbff、dna/rna/prot、gpt/gp/gpe/gpff、fasta 变体（`.faa` 按蛋白）、ab1、`.seq` 嗅探；`save_file` 写 `.gbk/.gb/.gpt`（protein 项目写 gbk 报错提示用 `.gpt`）
- 子序列导出 → `export_subsequence`（把项目一部分写成新文件，`output_path` 必填；四种互斥区间：① `start`+`end`（环状可绕原点），② `feature_id`（分段按 5'→3' 拼接，负链 rev-comp），③ `enzyme1`+`enzyme2` 或 `cut1`+`cut2`（切口按 1-based 碱基编号，合法范围 1..=len：N 表示断在碱基 N 与 N+1 之间；线性 N=len 为分子末端切口，环状 N=len 为原点切口——线性模式下"第一个碱基之前"的切口不可表达），④ `fwd_primer`+`rev_primer` 扩增子；导出文件含与区间重叠的特征（部分覆盖的截断到区间），坐标平移；引物按「首要结合位点（binding_sites[0]，Tm 降序最优）与导出区域有任何重叠即导出」规则随文件写出（位点坐标同特征映射规则裁剪/平移/翻链，重新打开时重算修正），响应附 `primers` 名单；环状项目导出片段一律线性）
- 序列读取 → `read_sequence`（文本标尺 + 机器可读 `sequence` 字段；窗口 ≤60 bp 时省略标尺行、只留行首坐标前缀，大窗口行为不变）、`get_project_overview`、`get_region_view`。digest 按 molecule_type 分支，非 DNA 项目不渲染 PRIMERS/ENZYMES/甲基化等节；region digest 在窗口内有已存比对时附 `ALIGNMENT DIFFS IN REGION` 节（每条比对列出窗口内的 mismatch/deletion/insertion 明细，窗口外不列；无差异标 `no differences in window`；环状绕原点窗口正确过滤，跨原点合并删除按覆盖区间判定重叠）
- 序列编辑 → `edit_sequence`（`replacement` 字符串或 `replacement_path` 文件互斥，空串=删除；`strand: "-"` 先将替换序列反向互补再插入，仅 DNA 项目；`replacement_path` 文件携带的特征/引物随序列一并转移：特征裁剪到插入区间后重基到插入点（strand "-" 时镜像+翻链+分段逆序，`transfer_features_for_insert`），引物仅 DNA 项目、位点由重算补齐；名称与既有特征/引物冲突时自动加 ` (2)` 后缀（`unique_name`）；响应附 `transferredFeatures`/`transferredPrimers` 名单；`expected_old` 乐观校验，失败返回差异索引与 ±20 bp 上下文 + `currentContent`（权威当前内容，可直接复制为 `expected_old` 重试）；按 delta 平移/裁剪特征，恒返回 `removedFeatures`/`clippedFeatures` 回显副作用；protein 项目校验字母表 A-Z + 末尾可选 `*`）
- 特征 → `add_feature`（结构化 1-based inclusive 参数：`start`+`end` 或 `segments: [{start, end}]`（5'→3' 顺序，分段特征），两者互斥；可选 `strand`（"." / "+" / "-"，默认 "+"）/`color`/`notes`；恒返回 `featureId`）、`update_feature`（`feature_id` + 可选 name/ftype/color/strand/start+end/segments 至少一项；span 更新不动 strand）
- 引物 → `add_primer`（返回重算后结合位点，名称冲突报错指明与引物还是特征冲突）、`list_primers`（只读）、`check_primer_binding`（返回全部结合位点：`bindingSiteCount` + Tm 降序 `sites` 数组（strand/templateStart/templateEnd/tm/annealLen/mismatchedTail + 全长模板覆盖 `alignedTemplate`/`matchMask`——逐碱基展示 5' 尾巴与模板相邻碱基的配对，`|` 匹配/`.` 错配/`-` 线性模板端外无对应碱基），兼容字段 `site` = 最佳位点；`annealLen` 为 3' 端实际连续匹配长度，`binds=true` 仅代表 3' 退火核心结合；顶层 `tmBasis` 说明口径。查 Tm 也用它——单独的 compute_tm 已移除）
- 引物设计 → `design_primers`（Amplify/OE-PCR/Mutagenesis；amplify 支持 `fwd_enzyme`/`rev_enzyme` 酶切尾巴 + `protect_bases`，恒返回 `internalSites`（非空附 warning）+ `orientation`（产物正链 = 模板正链 seg 区间，Fwd 在其 5' 端、Rev 在 3' 端，命名跟随模板正链而非 CDS 编码链）+ seg 重叠 CDS 时的 `cdsOverlaps`（含 strand 与方向 note，负链 CDS 注明 Fwd 位于 CDS 3' 端）；mutagenesis 校验差异 ≤3 bp，返回 `mutation` 自检块含 CDS 密码子/氨基酸变化（支持 join 分段 CDS），氨基酸编号双口径 `aaPosition1Based`（含起始 Met）/`aaPositionExcludingMet`（不含，如 mEGFP A206K）；整段替换 warning 分级——seg 恰好为 CDS 内完整密码子（密码子对齐、长度 %3==0）时不报，无法确认 CDS 上下文才报；`design_primers` 与 `check_primer_binding` 的 annealLen/Tm 口径不同：design 只算设计退火区，check 算 3' 端实际连续匹配，尾巴意外匹配模板时 check 值更高（属预期，用 matchMask 看具体配对））
- ORF 搜索 → `find_orfs`（`add_as_features` 可落库）
- 序列比对 → `add_alignment`（`bases`/`path` 双输入，长读段用 `path`（.gbk/.dna/.fasta/.ab1）；返回 `identity`（全精度）、`alignedLength` 与差异明细 `mismatchDetails`/`deletionDetails`/`insertionDetails`，另附 `orientedSequence`（按模板方向归一化的全量读段序列，strand "-" 已 rev-comp，不截断——.ab1 读段可超 1000 bp）与 `coverage`（读段覆盖的模板区间列表 [{start, end}]，1-based inclusive，segments 级别；环状跨原点给出各段）；成功响应恒附全部已存比对的 `alignments` 数组（每条同含 `orientedSequence`/`coverage`）；短读段最小 50 bp）
- IUPAC 序列搜索 → `search_sequence`（查询含核苷酸字母表外字符时按肽段处理：每残基展开为简并密码子模式（标准遗传密码），双链搜编码区；纯核苷酸查询仍按核苷酸搜索）
- 甲基化系统 → 无独立工具；`get_project_overview` 的 LOCUS 行展示；环状 DNA 持久化在 GBK `KEYWORDS` 的 `methylation: ...` 标注，未注明时默认 dam/dcm/ecoki 全开；前端设置走 Tauri command `set_methylation`
- 限制酶切位点 → `find_restriction_sites`（按名称列出识别位点 `recStart/recEnd/recSeq/识别链` 与切口 `topCutIndex/botCutIndex`；不传 `enzymes` 返回全部；未知酶名报错并给近似名——以此探测可用酶名，替代已删除的整库查询工具；需要区域内全部酶切位点全景时用 `get_region_view(compact:false)`）
- 自动标注 → `get_project_overview` 末尾 `DETECTED COMMON FEATURES (auto)` 节（只列非 fragment 特征，附 `(already annotated)` 标记；只读不落库；DNA 与 protein 项目可用——DNA 项目为 nt 级 + CDS 蛋白级双通路（aa 级命中附 `(protein-level)` 标记），protein 项目只按氨基酸序列匹配库中 CDS 的翻译）；引擎另接 Tauri command `annotate_features` 返回完整 JSON；新建序列弹窗的实时预览走 `annotate_sequence`（`moleculeType` 参数区分，protein 走 `annotate_protein`）
- 密码子优化 → `optimize_cds`（三种互斥输入：① `project_id`+`feature_id`（仅限 DNA 项目），② `sequence` 直接传 DNA 编码序列，③ `input_path` 文件（DNA 文件或 .gpt/.prot 蛋白反向翻译）；apply=false 预览返回前后 CAI/GC/repairs/unresolved 只读不落库，sequence/input_path 模式额外返回 `optimizedSequence`；`output_path` 可选写结果文件（.gbk → DNA GenBank，.gpt → 蛋白 GenBank）；method 为 use_best_codon/match_codon_usage/harmonize_rca（后者需 original_species）；species 用 `list_species` 键名；Tauri 侧另有带扩展参数的 `preview_codon_optimization`/`apply_codon_optimization` 与 `list_codon_species`）

上述 DNA 专属工具（`find_restriction_sites`/`find_orfs`/`design_primers`/`check_primer_binding`/`add_primer`/`add_alignment`/`search_sequence`）对 protein/rna 项目返回 isError。

未适配（前端/UI 专有，MCP 不可用）：

- **ROI**：`set_roi`/`clear_roi` 只有 Tauri command，属 UI 视图状态
- **My Primers / My Enzymes 库**：存 localStorage，后端不可见
- **质粒图视图**：纯渲染（弹窗内交互选区除外）；Map 弹窗底部 "Show as Background" 开关可把图谱作为不可交互水印叠加到 SequenceEditor（fixed 层、opacity 0.1、pointer-events none，实时跟随选区/编辑；全局持久化在 localStorage `mapWatermark`，跨项目/重启生效，并经 Tauri 事件 `map-watermark-changed` 在同时打开的窗口间实时同步（storage 事件在 Tauri 多 webview 间不可靠，仅作浏览器回退），MCP 无需适配）
- **前端搜索 UI**（feature/enzyme/primer 名称匹配）：MCP 侧只有序列搜索
- **多窗口管理**：Agent 用 `activate_project` 切换即可
- **视图/布局设置**（layoutParams、show* 开关、酶切过滤器）：渲染层状态
- **选区 badge 的肽链分子量**：protein 项目选区时 SelectionLengthBadge 第二行显示所选肽段分子量（kDa，平均同位素残基质量 + 水，`peptideMassKda` in editorConstants.js）；纯渲染层信息，MCP 无需适配
- **Tm 参数与引物分析设置**：MCP 工具内用默认浓度，暂未暴露参数
- **`add_alignment` 的 createdSites**：未实现（需按差异重建编辑后序列并重扫酶库，语义复杂、价值有限）；修序列后查位点走 `edit_sequence` + `find_restriction_sites`
- **自动标注前端弹窗**：MCP 经 `get_project_overview` 的 auto 节查看检测结果；批量落库需前端交互或逐特征 `add_feature`
- **新建序列项目弹窗（`create_project`）**：MCP 侧可写临时序列文件 + `open_file` 实现同等效果
- **复制粘贴标注迁移（`src/clipboardAnnotations.js`）**：复制选区/特征 (+) 链时把该子序列的特征（裁剪+重基到 0）与引物（首要结合位点有重叠即收，只存 name/type/primerSeq）打包成 meta，经 ClipboardItem 自定义 MIME `web application/x-libregene-annotations` 写入剪贴板（外部应用粘贴仍是纯文本），并写 localStorage `clipboardAnnotations` 兜底（WKWebView 自定义 MIME 不可靠；按粘贴文本与记录文本完全一致匹配）；粘贴时弹窗提示标注数量并可勾选不迁移，确认后特征随 `update_sequence` 合并落库、引物经 `addPrimers` 补入（名称冲突自动加 ` (2)` 后缀，位点重算）；antisense/translation/引物/amplimer 复制不携带标注，引物导入不进 undo 栈；纯前端交互，MCP 无需适配
- **RNA 二级结构预测（rnaFold 插件）**：折叠在前端用 ribossfold-wasm 完成（WASM 无法走 Rust 内核），用户决定不暴露给 Agent
- **系统文件关联打开（Open With / 双击 / 拖到 Dock）**：OS 集成；统一入 `pending_opens` 队列 + `file-opened` 事件，前端复用 `open_file`。Agent 直接用 `open_file` 即可

## 核心模型约定

- **坐标分层约定**：坐标基数按层划分，不是全局统一——
  - **Agent 可见面（MCP 工具 + digest 渲染）：一律 1-based inclusive**（GenBank 惯例）。集中转换点：mcp.rs 的 `to1`/`from1` 及各 `*_1based` helper（`feature_json_1based`/`site_json_to_1based`/`alignment_json_1based`/`edit_impact_json`/`mutagenesis_json_1based`），digest.rs 的 `cut_flanks`/`cut_notation`（pub，mcp.rs 复用）。错误信息与 fail envelope 中的索引同样 +1。
  - **内部模型与 Tauri command IPC：一律 0-based inclusive**（前端内部状态——selection/cursor 索引、特征/引物数据——也走这层，只在渲染处 +1）。`Feature.start/end`、`Feature.segments[]`、`PrimerBindingSite.template_start`、`BindingSite.matchStart/End` — 0-based inclusive；`PrimerBindingSite.template_end` — 0-based exclusive（数值恰好等于 1-based inclusive 末端，故 MCP 输出只对 `templateStart` +1）；`Enzyme.cut_index / bot_cut_index` — 切口在 0-based cutIndex-1 与 cutIndex 之间，对应接口表述 "1-based 碱基 cutIndex 与 cutIndex+1 之间"（渲染 `N^N+1`，环状原点切口 `len^1`）。
  - **前端 UI 渲染与用户输入：一律 1-based inclusive**（GenBank 惯例，与 Agent 面对齐）。内部状态保持 0-based，转换只发生在边界：渲染处直接 `+1`（FeatureInfoDialog 的 location 标签、SequenceEditor 悬停位置/选区 location、SequenceEditDialog 光标/选区、MapView 选区栏、PrimerOverview/MyPrimers/PrimerAlignment/PrimerDesign/CodonOptimization 各弹窗、DetectFeatures/NewSequence 命中表）；用户输入的 location 字符串发送前经 `locationStringTo0based` 转 0-based（Tauri `add_feature`/`update_feature_location` 按 0-based 解析）。location 字符串 helper 集中在 `src/editorConstants.js`：`locationString1based`（渲染用）、`locationString0based`（IPC 用，如 DetectFeaturesDialog 落库路径）、`locationStringTo0based`（用户输入 → IPC，逐数字 token -1，join/order/complement 嵌套与单点无需特判）。注意 NewSequenceDialog 的落库不走 location 字符串（传结构化 0-based segments），其 locationString 仅用于显示。
  - **GenBank 文件落盘/解析：按规范 1-based**（外部文件格式，不算 App 编码）：1-based 解析只保留在 `gbk.rs::parse_location_string`（供 .gpt/.gbk 文件解析用），App 内 location 字符串一律走 `parse_location_string_0based`（支持 "0..99"/"join(...)"/"order(...)"/"complement(...)"/单点）
- 模型坐标 0-based inclusive；gb-io Range 是 0-based end-exclusive
- `ProjectData.molecule_type` — `"dna" | "rna" | "protein"`（serde 输出 `moleculeType`），默认 `"dna"`；RNA/蛋白序列通常线性
- **分子类型 gate**：digest 渲染、酶/引物 recompute、translate refresh 都按 molecule_type 分支——非 DNA 跳过酶切/引物/甲基化（`ProjectData::is_dna()` 统一判定，空串视为 DNA）；auto-annotation 例外：DNA 项目走 nt 级 + CDS 蛋白级双通路，protein 项目按 aa 序列匹配 CDS 翻译（`annotate_protein`），RNA 项目不支持
- 环状序列坐标用 `% tlen` 归一化，`wrap_template_region` 负责环状拼接
