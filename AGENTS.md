# LibreGene — 质粒编辑器

React 19 + Vite + Tauri v2 + Rust 桌面质粒编辑器。纯 SVG 渲染，支持分段特征、引物、酶切标注、序列比对（ab1 色谱）、插件系统；默认输出增强型 GenBank（含颜色和引物注释）。

**这是 Tauri v2 桌面应用，不要用浏览器测试，必须用 `npx tauri dev` 启动。**

## 开发命令

```bash
tmux new-session -d -s libregene 'npx tauri dev'   # 后台启动桌面应用
tmux attach -t libregene                            # 查看输出（Ctrl-B D 分离）
npx vite build                 # 前端编译检查
npm run format / lint          # Prettier / ESLint（src/）
npm test                       # 前端 vitest（src/__tests__/）

cd backend
cargo test -p libregene-core --lib                  # 单元测试
cargo test -p libregene-core --test roundtrip_test  # 读写往返测试
cd src-tauri && cargo build     # 构建 Tauri 后端（须在 src-tauri 目录执行；backend workspace 根跑 cargo build -p LibreGene 会报 package 不匹配）
```

## Release 流程

push 到 master 时 CI（`.github/workflows/build.yml`）检查 `package.json` version 对应 tag `v<version>` 不存在则自动发 Release（macOS dmg、Windows msi/nsis、Linux 仅 Flatpak；manifest 在 `flatpak/`）。发布步骤：

1. bump `package.json` version（`tauri.conf.json` 引用它，无需另改）
2. 手写 `release-notes/v<version>.md`（缺失时 CI 回退为自动 notes）
3. 合并到 master

发布后 `homebrew-tap` job 用本仓库 `homebrew/libregene.rb` 模板（唯一事实来源，勿直接改 tap 仓库）更新 `dl-li/homebrew-libregene`（需 secret `HOMEBREW_TAP_TOKEN`）。

## 文件结构

```
src/                    # 前端：App.jsx（顶层状态）、ProjectWorkspace.jsx、SequenceEditor.jsx、
                        # editorConstants.js（共享常量/location helper）、api.js/tauriApi.js、
                        # dialogs/、components/、editor/（编辑器纯函数+hooks+layers/）、
                        # workspace/、hooks/、plugins/（静态注册表 index.js）
backend/libregene-core/src/  # Rust 核心库（models、align+blastn、orf、search、codon、digest、enzyme、primer、file_io）
src-tauri/src/          # lib.rs（门面）、state/payload/kernels/tray、commands/（#[tauri::command] 按域拆分）、
                        # mcp/（嵌入式 MCP server：mod.rs + tools/ + tests/）
```

## 编码准则

### 通用

- **尽量不写注释**；必要时写简短注释说明 Why。
- 改完必须验证：前端 `npx vite build`，后端 `cargo test -p libregene-core --lib`，Rust 改动另跑 `cd src-tauri && cargo build`。
- **tauri dev 只监听 `src-tauri/`**：改 `backend/libregene-core` 需 `touch src-tauri/src/*.rs` 手动触发重编译。
- **Rust/后端改动改完直接重启 dev**：`tmux kill-session -t libregene && tmux new-session -d -s libregene 'npx tauri dev'`，无需询问。
- **禁止擅自用 MCP server（127.0.0.1:8766）驱动运行中的应用做测试/复现**，除非用户明确要求。
- **禁止擅自截屏/录屏**，除非用户明确要求。

### Bug 修复流程

1. 修复前 `git status` + `git log --oneline -5` 确认状态
2. **每修一个 Bug 单独提交一次**（`git add` 只含相关文件）
3. 提交前跑对应测试和构建；提交信息用英文：`fix: ...` / `refactor: ...`
4. 涉及 UI 的改动在 tmux 中的 Tauri dev 验证

### 前端

- 状态集中在 `App.jsx`，`SequenceEditor.jsx` 只管理 UI 状态；JSON 字段 camelCase。
- **分子类型模式**：`moleculeType`（`"dna"|"rna"|"protein"`，缺省 dna）决定编辑器形态；rna/protein 单链、隐藏 DNA 专属插件（`dnaOnly`/`rnaOnly`）；`.rna/.prot/.dna` 只读、必须 Save As。
- **插件机制**：编译期静态注册表（`src/plugins/index.js`），无运行时动态加载；新插件须进注册表并可在设置页禁用（localStorage `disabledPlugins`，禁用时 sidebar/dialog/nav 入口全消失）。注册表钩子：`dialog`（谓词 `dialogVisible`）、`track`（`useLane(ctx)` + `render(ctx, lane)`，模板锚定的绘制必须经 ctx 的 `colVis`/`colRuns` 换算，不可假设列与 x 线性对应）、`featuresMenuItem`、`settingsField`。
- **行布局用可视单元流**（`buildStreamLayout`，`src/editor/alignmentLayout.js`）：模板列与比对插入槽位列合成一条流，每行 `baseCpl` 个单元；坐标一律经 `rowOf`/`colOfAbs`/`colVis`/`colFromVis`/`colRuns`/`sp` 换算，**不要再假设「每行固定 `gridCpl` 个模板列」**。

### 后端（Rust）

- **锁顺序**：先 `pm` 再 `window_projects`；`agent_tabs` 锁不得与它们同时持有（取前先 drop 其他 guard）。
- 重计算（酶/引物/比对）放 `spawn_blocking`；所有 mutation 命令调用 `broadcast_project()` 同步多窗口。
- 环状序列 region 用 `wrap_template_region` 拼接（`% tlen` 可能为 0 导致空切片）。

### 多窗口

- 主窗口 label `"main"`；项目窗口 `"project-{safe_id}-{timestamp}"`（注册在 `window_projects`，经 `resolve_project_id()` 按 label 查项目，未命中=报错提示 reload，绝不回退主窗口 active）。
- MCP `open_project` 不开窗口：项目绑定为**主窗口侧边栏 Agent 标签**（`AppState.agent_tabs`，列表条目带 `agentLocked: bool|null`）。

## 核心模型约定

- **坐标分层**（非全局统一）：
  - **MCP 工具 + digest 渲染：1-based inclusive**（转换点：mcp.rs `to1`/`from1`、digest.rs `cut_flanks`/`cut_notation`）
  - **内部模型与 Tauri IPC：0-based inclusive**（`Feature.start/end`、`segments[]`；例外：`PrimerBindingSite.template_end` 为 0-based exclusive，数值等于 1-based 末端；`Enzyme.cut_index` 在 0-based cutIndex-1 与 cutIndex 之间）
  - **前端 UI 渲染与输入：1-based inclusive**，内部保持 0-based，只在边界转换（helper 在 `src/editorConstants.js`）
  - **GenBank 落盘/解析：1-based**（`gbk.rs::parse_location_string`；App 内用 `parse_location_string_0based`）；gb-io Range 是 0-based end-exclusive
- **分子类型 gate**：digest 渲染、酶/引物 recompute、translate 都按 `molecule_type` 分支，非 DNA 跳过（`ProjectData::is_dna()`，空串视为 DNA）；auto-annotation 例外（DNA 双通路，protein 按 aa 匹配，RNA 不支持）。
- **跨原点特征**：segments 按 join 顺序存储，后段 `start` < 前段 `start` 即跨原点；`selStart > selEnd` 表示跨原点选区（渲染/复制支持，replace/delete/paste 不支持）。
- **比对模型不变量**：段/插入按 read 自身 5'→3' 顺序走一遍必须精确重建 `Alignment.seq`（后端 `left_align_indels`/`anchor_loose_ends` 守护，`tests/align_model_test.rs`）；同一模板列最多一个插入条目；序列编辑后所有比对自动重算（`align::realign_project`），**重算必须由 strand 反推原始取向的 read**（存盘的 `Alignment.seq` 已是显示取向，直接回喂会翻转 strand、导致色谱镜像）；编辑瞬间前端先用 `src/alignmentEdit.js::adjustAlignmentsForEdit` 本地搬移模型防画面闪烁。
- **BlastN full-length 快速路径必须返回最优解**：trace 打包「胜出状态+延伸位」、回溯按状态机走；旋转候选覆盖整条模板取样（`tests/full_length_path_test.rs` 守护）。
- ab1 trace 不进 `ProjectData` 序列化，只记 `tracePath`（`.gbk` 中相对 .gbk 目录存储），前端经 `get_chromatogram` 懒加载；反向链 read 的峰图由 `orientChromatogram` 做 rev-comp 后显示。

## API

- **Tauri commands**：定义在 `src-tauri/src/commands/`（按域拆分），前端封装在 `src/tauriApi.js`。
- **HTTP API**：`libregene serve`，前缀 `http://127.0.0.1:8765`，见 `src/api.js`。

## MCP 支持

嵌入式 MCP server（`src-tauri/src/mcp/`）让外部 LLM Agent 操作应用。

- **架构**：进程内 Streamable HTTP，绑定 `127.0.0.1:8766`（仅回环），与前端共享 `AppState`；所有 mutation 走 `crate::do_*` 内核（同 recompute/dirty/broadcast 路径）。
- **鉴权**：`Host` 必须严格等于 `127.0.0.1:<port>`（始终强制）；`requireAuth`（默认开）时每请求需 `Authorization: Bearer <token>`，令牌存 `<app_config_dir>/mcp_auth_token`；文件路径经 `validate_user_path` 校验。
- **Agent 标签强制隔离**：`open_project` = 加载 + 绑定主窗口 Agent 标签（默认 locked）；已加载未绑定（用户项目）则拒绝，指引 Agent `cp` 副本再打开；mutation 工具对未绑定项目报错，任何调用自动重锁；解锁走前端 `set_agent_tab_locked`。
- **工具**：18 个，清单见下节；**参数与行为细节以 `mcp/mod.rs` 工具描述为准**（不在本文件重复）。`project_id` 必填；mutation 统一返回 `{ok, message, projectId, regionView?}`；项目相关响应带 `sequenceHash`/`revCompHash`（7 位 FNV-1a，大小写/空白不敏感，digest 头部带同样的 `SEQHASH:` 行），跨调用对比即可发现序列变化。
- **文件优先 I/O**：工具描述统一引导 Agent 用文件传序列，纯文本只留给短输入；改描述时保持此口径。
- **测试**：`src-tauri` 内 `cargo test --lib` 覆盖 MCP 启停/鉴权/Agent 标签门控/各工具正反例。

### 功能 MCP 适配清单

**新增/修改功能时必须更新本清单**：标注「已适配」（给工具名）或「未适配」（记原因）。

已适配（功能 → 工具）：

- 项目/文件管理、Agent 标签绑定、子序列导出 → `open_project` / `save_file` / `close_project` / `list_projects`
- 序列读取、坐标转换、自动标注/甲基化展示 → `read_sequence` / `get_project_overview` / `get_region_view`
- 序列编辑（连同已存比对自动重算）→ `edit_sequence`；特征 → `set_feature`
- 引物 → `add_primer` / `list_primers` / `check_primer_binding` / `design_primers`
- ORF → `find_orfs`；比对 → `add_alignment`（`algorithm`: "blast" 默认 / "smith-waterman"）；IUPAC 搜索 → `search_sequence`；酶切位点 → `find_restriction_sites`；序列转换/密码子优化 → `convert_sequence`（dna↔rna、→protein、protein 逆转录、密码子优化，批量逐项错误隔离）
- DNA 专属工具（`find_restriction_sites`/`find_orfs`/`design_primers`/`check_primer_binding`/`add_primer`/`add_alignment`/`search_sequence`）对 protein/rna 项目返回 isError

未适配（每项一句话记原因）：

- ROI、视图/布局设置（layoutParams、show* 开关、酶切过滤器、特征标签位置）：UI 视图状态
- GC 含量轨道（`src/plugins/gcContent/`）：纯渲染
- My Primers / My Enzymes 库：存 localStorage，后端不可见
- 酶 Provider 数据与筛选（`enzyme_providers.json`、`get_enzyme_providers`）：仅展示用，不进 recompute
- 质粒图视图 / 编辑器背景水印、选区 badge 分子量：纯渲染
- 前端搜索 UI（名称匹配）：MCP 只有序列搜索
- Agent 标签解锁按钮/导航控制条：纯前端，锁定状态后端持有
- Tm 参数与引物分析设置：`design_primers` 已暴露浓度参数，其余为渲染层状态
- `add_alignment` 的 createdSites：未实现，改用 `edit_sequence` + `find_restriction_sites`
- 自动标注弹窗、新建序列弹窗、复制粘贴标注迁移、rnaFold 插件（WASM）、系统文件关联/拖放打开：纯前端/OS 集成
- Dotplot 插件（`src/plugins/dotplot/`）：纯前端渲染
- BLAST 插件（右键选区 → `blast_submit`）：交互式外网操作，Agent 场景意义不大
- 拓扑切换（`set_topology`，仅 DNA）：未暴露 MCP 工具
- SnapGene 历史快照（`src/plugins/snapgeneHistory/`，`get_snapgene_history`/`open_snapgene_snapshot`）：未暴露 MCP 工具
- ab1 色谱图显示（`src/chromatogram.js` 等）：纯前端渲染

## 仍有改进空间（非 Bug）

- SequenceEditor 深化：selection 状态族集中成 useReducer/context、全局键鼠事件抽 hook、`src/editor/layers/` 渲染函数升级为真子组件
- SVG 容器 `contain: 'layout style'` 可能影响固定定位元素
- `list_projects` JSON 构建可用序列化替代 `json!` 宏
