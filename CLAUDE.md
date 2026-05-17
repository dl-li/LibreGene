# Geneie — 质粒编辑器

基于 React + Vite + Tauri v2 + Rust/axum 的桌面质粒编辑器。纯 SVG 渲染，支持多行自适应换行、分段特征、引物可视化、酶切位点标注。默认输出 SnapGene 风格 GenBank 文件。

**这是 Tauri v2 桌面应用，不要用浏览器测试，必须用 `npx tauri dev` 启动。**

## 技术栈

- **前端**: React 19 + Vite 8，shadcn v4 (Radix UI)，lucide-react 图标，Tailwind CSS v4
- **渲染**: 纯 SVG，Cascadia Code / TeX Gyre Heros 字体
- **后端**: Rust (edition 2021), axum 0.8, tokio 1, gb-io 0.9
- **桌面壳**: Tauri v2，内嵌 geneie-core
- **无测试框架**（前端无测试，后端仅 Rust 单元测试 + golden tests）

## 文件结构

```
Geneie/
├── index.html                  # Vite 入口，含 @font-face 定义
├── vite.config.js              # Vite 配置（es2022 target, vendor chunk, @ alias）
├── package.json                # React 19, Vite 8, shadcn v4, lucide-react
├── components.json             # shadcn 配置 (new-york style, neutral base)
├── jsconfig.json               # 路径别名 (@/ → src/)
├── .gitignore
├── assets/Fonts/               # 10 个字体文件 (Cascadia Code, TeX Gyre Heros/Termes)
├── test/                       # 测试文件 (.dna, .gbk)
├── src/                        # 前端 React 源码
│   ├── main.jsx                # 入口，ReactDOM.createRoot
│   ├── App.jsx                 # 顶层：Empty(无文件)/Sidebar+Editor(有文件)，状态管理
│   ├── SequenceEditor.jsx      # 核心编辑器：SVG 渲染、选择/光标、酶/引物/特征渲染
│   ├── editorConstants.js      # 共享常量与工具函数（cw, getX, measureWidth, splitRange）
│   ├── tauriApi.js             # Tauri IPC + 浏览器 fetch/WS fallback API 客户端
│   ├── ErrorBoundary.jsx       # React Error Boundary
│   ├── primerRenderer.jsx      # 引物几何计算（segment path, hover background）
│   ├── PrimerSegmentRenderer.jsx # 引物 segment 渲染组件
│   ├── components/
│   │   ├── DebugDialog.jsx     # 调试面板（酶过滤器、甲基化、引物参数等）
│   │   └── ui/                 # shadcn UI 组件
│   │       ├── button.jsx, checkbox.jsx, dialog.jsx, input.jsx
│   │       ├── label.jsx, select.jsx, separator.jsx, sheet.jsx
│   │       ├── sidebar.jsx, tooltip.jsx, empty.jsx, skeleton.jsx
│   ├── hooks/
│   │   └── use-mobile.js       # 移动端断点检测（768px）
│   └── lib/
│       └── utils.js            # cn() 工具（clsx + tailwind-merge）
├── src-tauri/                  # Tauri v2 桌面壳
│   ├── Cargo.toml              # Tauri 依赖 + 内嵌 geneie-core
│   ├── tauri.conf.json         # Tauri 配置 (窗口 1400x900, bundle, CSP)
│   ├── capabilities/           # 权限配置
│   ├── icons/                  # 应用图标
│   └── src/
│       └── lib.rs              # 19 个 Tauri commands，事件广播，ProjectManager
├── backend-rs/                 # Rust 后端 (workspace)
│   ├── geneie-core/            # 核心库
│   │   ├── data/comm_only_enzymes.json  # 623 酶数据库（编译时嵌入）
│   │   └── src/
│   │       ├── models.rs       # ProjectData, Enzyme, Feature, Primer, BindingSite
│   │       ├── project.rs      # ProjectManager（HashMap, max 24, eviction）
│   │       ├── utils.rs        # complement, reverse_complement, DNA_COMP
│   │       ├── enzyme/         # 酶切引擎：search, matching, cut, methylation, data
│   │       ├── primer/         # 引物引擎：align, gbk, dna, tm
│   │       └── file_io/        # 文件解析/序列化：gbk, dna, fasta, ab1, color
│   └── geneie-server/          # axum HTTP + WebSocket（浏览器 fallback）
│       └── src/
│           ├── main.rs         # :8765, CORS + 压缩
│           ├── routes.rs       # 19 个 REST 端点（含多项目管理）
│           └── ws.rs           # WebSocket 实时广播
└── backend/                    # [参考] 原 Python 后端（保留用于 golden file）
```

## 开发命令

```bash
npx tauri dev                  # 启动桌面应用（唯一正确的开发方式）
npx vite build                 # 仅前端编译检查
npx shadcn add <component>     # 添加 shadcn 组件

# 后端
cd backend-rs
cargo test -p geneie-core --lib                  # 单元测试
cargo test -p geneie-core --test golden_tests     # Golden 测试
```

## 前端架构

### 布局（App.jsx）
- **无文件打开**：全屏 shadcn Empty 组件（DNA 图标 + Open File 按钮）
- **有文件打开**：左侧绝对定位浮层 Sidebar（可折叠为图标）+ 全宽 SequenceEditor
- Sidebar 宽度 `12rem`（展开）/ `3rem`（折叠为图标），展开时浮于序列上方
- TooltipProvider 包裹全局
- 侧边栏：Open File 按钮 + 已打开文件列表（点击 `activateProject` 切换）
- 无 demo 数据，初始 `sequence` 为 `null`

### 序列选择（SequenceEditor.jsx）
- **普通选择**：棕黑色 `#3E2723` 背景 + 白色文字，`selStart`/`selEnd` inclusive
- **光标**：棕黑竖线 + bgColor 描边，覆盖整行高度，5 秒不动自动消失
- **交互**：
  - 单击 → 光标定位；拖拽 → 选中序列；松开 → 光标消失
  - Shift+单击 → 从光标处扩展到点击处
  - 方向键 → 移动光标并清除选区
  - Ctrl/Cmd+C → 复制选中序列
  - 点击特征/标签 → 选中特征完整序列区域
- **有选区时不显示光标**（拖拽过程除外）
- `clientToSeqIndex` 检测字符格左/右半侧精确定位
- 逐字符 `<tspan>` 渲染，`textAnchor="middle"` 精确对齐网格

### 常量（editorConstants.js）
- `cw = 14`（字符宽度 px）, `startX = 220`（左边距）, `baseSeqY = 100`
- `getX(col)` = `startX + col * cw`（列左边缘）
- `measureWidth(text, font)` — Canvas 2D 缓存测量
- `splitRange(start, end, charsPerLine)` — 索引范围 → 按行 segment

### 渲染层 Z-Index（低→高）
```
Cursor → SelectionBg → FeatureLayer → EnzymeLines → PrimerLayer →
EnzymeLabels → EnzymeOverlay → HoveredFwdPrimer → SelectedPrimerOverlay →
SequenceRows → EnzymeTooltip
```

### 重要约定
- `matchStart/End` inclusive；`cutIndex` 0-based，切口在 `cutIndex-1` 与 `cutIndex` 之间
- Rev 引物前端从右到左遍历，显示 5'→3'
- 酶 hover 事件在稳定的 `<g>` 元素上
- 引物选中后 hover 事件禁用
- 所有 JSON 使用 camelCase（serde `rename_all`）

## API

### Tauri Commands（19 个）
```
get_project, open_file, save_file, update_sequence,
set_roi, clear_roi,
get_features, add_feature, delete_feature,
get_primers, add_primer, delete_primer,
set_methylation,
get_projects, activate_project, delete_project
```

### REST API（浏览器 fallback, 19 个端点）
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

- `/project` 默认返回 unique 酶，`enzyme_filter=all` 返回全部
- WebSocket/Event 广播：`{ type: "project", data: ProjectData, projects: [...], activeId: "..." }`

## 核心模型

### ProjectData
`sequence`, `length`, `topology` ("circular"/"linear"), `features`, `primers`, `enzymes`, `methylation_systems`, `roi`

### BindingSite
`matchStart/End` (inclusive), `tm`, `fivePrimeTail`, `threePrimeTail`, `alignment: [{ templateCol, kind: "match"|"mismatch"|"gap", primerBase, templateBase, insertionAfter }]`

### Enzyme
`rec_seq`, `rec_seq_pattern` (含 IUPAC), `rec_start/end`, `display_start/end`, `cut_index`, `bot_cut_index`, `cut_pairs: [{ topCutIndex, botCutIndex }]`, `recognition_strand`, `cut_type`, `cut_twice`, `is_palindromic`, `methylation_blocked`, `methylated_offsets`, `methylation_required`, `methyl_required_sources`

## 酶切引擎

1. 全局 IUPAC 正则搜索所有识别位点（正链 + 反链互补，含重叠）
2. 匹配 Biopython ci_1b 到对应识别位点（最小距离）
3. 从识别位点计算切点坐标
4. 环状序列：扩展序列搜索，坐标取模

**切点计算（0-based）**：
- 上链：`top_cut = rec_start + fst5`, `bot_cut = rec_start + rec_len + fst3`
- 下链：`top_cut = rec_start - fst3`, `bot_cut = rec_start + rec_len - fst5`
- cut-twice 酶：第二对用 scd5/scd3 替换 fst5/fst3
- 回文酶：上链/下链去重归一化为上链

## 甲基化

| 甲基化酶 | 靶点 | 修饰位置 |
|---------|------|---------|
| Dam | GATC | A (offset 2) |
| Dcm | CCWGG | C (offset 1) |
| EcoKI | AACN₆GTGC | A (offset 2) |

- 阻断：`methylation_blocked=true` → 前端灰显 + `[Blocked]`
- 依赖：`methylation_required=true` → `[Methyl Required]`
- 依赖酶检测始终自动计算，不受用户甲基化选择影响

## 多文件支持

- `ProjectManager` 内部 `HashMap<String, ProjectData>`，上限 24 个，超出自动驱逐
- 同时只有一个 active 项目
- `GET /projects` 返回摘要 + activeId；`POST /projects/activate?id=` 切换
- 关闭 active 项目时自动切换到下一个

## 待优化项

| 优先级 | 问题 | 说明 |
|--------|------|------|
| 中 | SequenceEditor.jsx ~1500 行 | 需拆分组件 |
| 中 | PrimerLayer 重复代码 | 三个组件共享渲染逻辑需抽象 |
| 中 | 选择渲染效果 | 背景色位置和光标交互需打磨 |
| 低 | 前端纯 JS | TypeScript 迁移成本 3-5 天 |
| 低 | FeatureLayer O(n²) 边界检测 | 实际特征数量少，不紧急 |
