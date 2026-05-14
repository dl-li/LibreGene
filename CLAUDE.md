# Geneie — 质粒编辑器

基于 React + Vite 前端 + Rust/axum 后端的质粒编辑器。纯 SVG 渲染，支持多行自适应换行、分段特征、引物可视化、酶切位点标注。默认输出 SnapGene 风格 GenBank 文件。

## 技术栈

- **前端**: React 19 + Vite 8，纯 SVG，Cascadia Code / TeX Gyre Heros 字体
- **后端**: Rust (edition 2021), axum 0.8, tokio 1, gb-io 0.9
- **无测试框架**（前端无测试，后端仅 Rust 单元测试 + golden tests）

## 文件结构

```
Geneie/
├── index.html              # Vite 入口，含 @font-face 定义
├── vite.config.js           # Vite 配置（es2022 target, vendor chunk, @ alias）
├── package.json             # React 19, Vite 8, lucide-react
├── .gitignore               # node_modules, dist, target, .DS_Store, .venv
├── assets/Fonts/            # 10 个字体文件 (Cascadia Code, TeX Gyre Heros/Termes)
├── test/                    # 测试文件 (.dna, .gbk)
├── src/                     # 前端 React 源码
│   ├── main.jsx             # 入口，ReactDOM.createRoot
│   ├── App.jsx              # 顶层组件：状态管理、工具栏、调试面板、键盘快捷键
│   ├── SequenceEditor.jsx   # 核心编辑器：SVG 渲染、行计算、酶/引物/特征渲染
│   ├── api.js               # 后端 REST + WebSocket API 客户端
│   ├── demoData.js          # Demo 模式数据（baseSeq, features, enzymes, primers）
│   ├── editorConstants.js   # 共享常量与工具函数（cw, getX, measureWidth, splitRange）
│   ├── ErrorBoundary.jsx    # React Error Boundary
│   ├── primerRenderer.jsx   # 引物几何计算（segment path, hover background）
│   └── PrimerSegmentRenderer.jsx  # 引物 segment 渲染组件
├── backend-rs/              # Rust 后端
│   ├── Cargo.toml           # workspace: geneie-core, geneie-server
│   ├── geneie-core/         # 核心库
│   │   ├── Cargo.toml       # serde, regex, gb-io, quick-xml, rayon 等
│   │   ├── data/comm_only_enzymes.json  # 623 酶数据库（编译时嵌入）
│   │   └── src/
│   │       ├── lib.rs       # pub mod enzyme, file_io, models, primer, project, utils
│   │       ├── models.rs    # ProjectData, Enzyme, Feature, Primer, BindingSite 等
│   │       ├── project.rs   # ProjectManager（多文件 HashMap + activeId）
│   │       ├── utils.rs     # complement, reverse_complement, DNA_COMP
│   │       ├── enzyme/      # 酶切引擎：search, matching, cut, methylation, data, elucidate
│   │       ├── primer/      # 引物引擎：align, gbk, dna, tm
│   │       └── file_io/     # 文件解析/序列化：gbk, dna, fasta, ab1, color
│   └── geneie-server/       # axum HTTP + WebSocket 服务
│       ├── Cargo.toml       # axum 0.8, tokio, tower-http, clap
│       └── src/
│           ├── main.rs      # 服务入口，:8765，CORS + 压缩
│           ├── routes.rs    # 16 个 REST 端点
│           └── ws.rs        # WebSocket 实时推送
└── backend/                 # [参考] 原 Python 后端（不再使用，保留用于 golden file 生成）
```

## 开发命令

```bash
# 后端
cd backend-rs
cargo build -p geneie-server --release && ./target/release/geneie-server  # :8765
cargo test -p geneie-core --lib           # 单元测试
cargo test -p geneie-core --test golden_tests  # Golden 测试

# 前端（需先启动后端）
npx vite --port 5173                       # 默认连接 :8765

# 前端 build
npx vite build
```

## REST API

```
GET    /project?enzyme_filter=unique&row_start=N&row_end=M&cpl=60
POST   /open?path=                PUT    /sequence
POST   /save?path=                POST   /roi?s=&e=
POST   /roi/clear                 GET    /features
POST   /features                  DELETE /features/{id}
GET    /primers                   POST   /primers
DELETE /primers/{id}              POST   /methylation?systems=dam,dcm,ecoki
GET    /projects                  POST   /projects/activate?id=
DELETE /projects/{id}             WS     /ws
```

- `/project` 默认返回 unique 酶（每行首次出现），`enzyme_filter=all` 返回全部
- WebSocket 广播格式：`{ type: "project", data: ProjectData, projects: [...], activeId: "..." }`
- 所有 JSON 使用 camelCase（serde `rename_all`）

## 核心模型

### ProjectData（顶层）
`sequence`, `length`, `topology` ("circular"/"linear"), `features`, `primers`, `enzymes`, `methylation_systems`, `roi`

### BindingSite（引物结合位点）
`matchStart/End` (inclusive), `tm`, `fivePrimeTail`, `threePrimeTail`, `alignment: [{ templateCol, kind: "match"|"mismatch"|"gap", primerBase, templateBase, insertionAfter }]`

### Enzyme
`rec_seq`, `rec_seq_pattern` (含 IUPAC), `rec_start/end`, `display_start/end`, `cut_index` (第一对切点的 top_cut，向后兼容), `bot_cut_index` (向后兼容), `cut_pairs: [{ topCutIndex, botCutIndex }]` (全部切点对，标准酶 1 对，cut-twice 酶 2 对), `recognition_strand` ("top"/"bottom"), `cut_type` ("blunt"/"5overhang"/"3overhang"), `cut_twice`, `is_palindromic`, `methylation_blocked`, `methylated_offsets`, `methylation_required`, `methyl_required_sources`

## 前端架构要点

### 常量（editorConstants.js）
- `cw = 14`（字符宽度 px）, `startX = 220`（左边距）, `baseSeqY = 100`
- `getX(col)` = `startX + col * cw`（列左边缘）
- `measureWidth(text, font)` — Canvas 2D 缓存测量
- `splitRange(start, end, charsPerLine)` — 索引范围 → 按行 segment

### 渲染层 Z-Index（低→高）
```
FeatureLayer → EnzymeLines → PrimerLayer(skipSelected) → EnzymeLabels →
EnzymeOverlay → HoveredFwdPrimer → SelectedPrimerOverlay → SequenceRows → EnzymeTooltip
```

### 选择互斥
序列选择、引物选择、酶对选择同时只能存在一个。新选择自动清除旧选择。

### 引物配对交互
- mousedown → 选中；拖拽 >3px → 进入配对模式，同向引物淡化（opacity 0.2）
- 拖拽到对向引物上释放 → 完成配对；其他位置释放 → 取消选中
- Shift+单击对向引物 → 即时配对
- 配对后高亮扩增子中间区域

### 复制（Ctrl/Cmd+C）
- 单引物：`primerSeq`
- 配对引物：Fwd 序列 + 中间模板 + Rev 反向互补（PCR 产物）
- 序列选中：`seq.substring(start, end)`

### 重要约定
- `matchStart/End` inclusive
- `cutIndex` 0-based，切口在 `cutIndex-1` 与 `cutIndex` 之间
- Rev 引物前端从右到左遍历，显示 5'→3'
- 酶 hover 事件在稳定的 `<g>` 元素上
- 引物选中后 hover 事件禁用

## 酶切引擎算法

识别位点优先算法：

1. 全局 IUPAC 正则搜索所有识别位点（正链 + 反链互补，含重叠匹配）
2. 匹配 Biopython ci_1b 到对应识别位点（最小距离）
3. 从识别位点计算切点坐标
4. 环状序列：扩展序列搜索，坐标取模规范化

### 切点计算（0-based）

**上链识别位点**：
```
top_cut = rec_start + fst5
bot_cut = rec_start + rec_len + fst3
```

**下链识别位点**（非回文酶）：
```
top_cut = rec_start - fst3
bot_cut = rec_start + rec_len - fst5
```

**cut-twice 酶**：第二对切点使用 scd5/scd3 代替 fst5/fst3（同样公式）。

**回文酶**：上链/下链去重后统一归一化为上链。

性能：单酶 >500 位点自动跳过；flySWARM (13.4kb) 全酶计算 <2s。

## 甲基化检测

| 甲基化酶 | 靶点 | 修饰位置 |
|---------|------|---------|
| Dam | GATC | A (offset 2) |
| Dcm | CCWGG | C (offset 1) |
| EcoKI | AACN₆GTGC | A (offset 2) |

- 阻断：酶识别位点覆盖甲基化碱基 → `methylation_blocked=true`，前端灰显 + `[Blocked]`
- 依赖：DpnI 需 Dam 甲基化 → `methylation_required=true`，前端显示 `[Dam Methyl Required]`
- 依赖酶检测不受用户甲基化选择影响（始终自动计算）

## 多文件支持

- `ProjectManager` 内部 `HashMap<String, ProjectData>`，以文件路径为 ID
- 同时只有一个 active 项目
- `GET /projects` 返回摘要 + activeId；`POST /projects/activate?id=` 切换
- 关闭 active 项目时自动切换到下一个

## 待优化项

| 优先级 | 问题 | 说明 |
|--------|------|------|
| 中 | SequenceEditor.jsx ~1200 行 | 需拆分组件 + prop drilling 治理 |
| 中 | PrimerLayer 重复代码 | 三个组件共享渲染逻辑需抽象 |
| 低 | FeatureLayer O(n²) 边界检测 | 实际特征数量少，不紧急 |
| 低 | getXPositions 缓存无限增长 | 仅 resize 时增长，影响可忽略 |
| 低 | with_active_mut 每次 clone | 需改为直接操作 HashMap |
| 低 | 前端纯 JS | TypeScript 迁移成本 3-5 天 |
