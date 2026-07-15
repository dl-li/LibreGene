# LibreGene — 质粒编辑器

基于 React + Vite + Tauri v2 + Rust 的桌面质粒编辑器。纯 SVG 渲染，支持多行自适应换行、分段特征、引物可视化、酶切位点标注。默认输出 SnapGene 风格 GenBank 文件。

**这是 Tauri v2 桌面应用，不要用浏览器测试，必须用 `npx tauri dev` 启动。**

## 技术栈

- **前端**: React 19 + Vite 8，shadcn v4 (Radix UI)，lucide-react 图标，Tailwind CSS v4
- **渲染**: 纯 SVG，Cascadia Code / TeX Gyre Heros 字体
- **后端**: Rust (edition 2021), tokio 1, gb-io 0.9
- **桌面壳**: Tauri v2，内嵌 libregene-core
- **无测试框架**（前端无测试，后端仅 Rust 单元测试 + golden tests）

## 文件结构

```
LibreGene/
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
│   ├── editHistory.js          # 撤销/重做历史栈（快照: { sequence, features, cursorIndex, selStart, selEnd }）
│   ├── tauriApi.js             # Tauri IPC API 客户端
│   ├── FeatureInfoDialog.jsx   # 双击特征弹窗：GenBank信息、ftype/color/location编辑
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
├── backend/                    # Rust 后端 (workspace)
│   ├── libregene-core/            # 核心库
│   │   ├── data/comm_only_enzymes.json  # 623 酶数据库（编译时嵌入）
│   │   └── src/
│   │       ├── models.rs       # ProjectData, Enzyme, Feature, Primer, BindingSite
│   │       ├── project.rs      # ProjectManager（HashMap, max 24, eviction）
│   │       ├── utils.rs        # complement, reverse_complement, DNA_COMP
│   │       ├── enzyme/         # 酶切引擎：search, matching, cut, methylation, data
│   │       ├── primer/         # 引物引擎：align, gbk, dna, tm
│   │       └── file_io/        # 文件解析/序列化：gbk, dna, fasta, ab1, color
│   └── test_data/              # Golden 测试数据
└── src-tauri/                  # Tauri v2 桌面壳
    ├── Cargo.toml              # Tauri 依赖 + 内嵌 libregene-core
    ├── tauri.conf.json         # Tauri 配置 (窗口 1400x900, bundle, CSP)
    ├── capabilities/           # 权限配置
    ├── icons/                  # 应用图标
    └── src/
        └── lib.rs              # 21 个 Tauri commands，多窗口路由，ProjectManager
```

## 开发命令

```bash
npx tauri dev                  # 启动桌面应用（唯一正确的开发方式）
npx vite build                 # 仅前端编译检查
npx shadcn add <component>     # 添加 shadcn 组件

# 后端
cd backend
cargo test -p libregene-core --lib                  # 单元测试
cargo test -p libregene-core --test golden_tests     # Golden 测试
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

### 特征编辑（FeatureInfoDialog）
- 双击 feature 弹窗展示 ftype / location / qualifiers（GenBank 格式）
- ftype：双击下拉菜单切换类型；color：弹窗标题栏取色器；location：双击编辑 + 后端校验
- 所有特征修改推入 `editHistory.push()`，支持 Cmd+Z 撤销并标记 dirty
- Feature 模型新增 `qualifiers: Vec<(String, String)>` 存储原始 GenBank 键值对

### 序列选择增强
- 拖拽选择时在光标左侧显示 `N bp, ~Tm°C`（等宽字体、bgColor stroke、底部对齐竖线底端）
- Tm 估算：Wallace rule (<20bp) / Marmur-Doty (≥20bp)
- 悬浮碱基上方显示 1-based 序号（低透明度深棕色、bgColor stroke）
- 序号仅在非拖拽时显示（选择完成后仍可看到）

### 引物颜色安全
- 前端 `safePrimerColor(c)` 过滤 `#000000`/`#000`/`black`，兜底为 `#166534`（绿色）
- 前端 `p.color || '#166534'` + `PrimerSegmentRenderer` 均有安全兜底

### 重要约定
- `matchStart/End` inclusive；`cutIndex` 0-based，切口在 `cutIndex-1` 与 `cutIndex` 之间
- Rev 引物前端从右到左遍历，显示 5'→3'
- 酶 hover 事件在稳定的 `<g>` 元素上
- 引物选中后 hover 事件禁用
- 所有 JSON 使用 camelCase（serde `rename_all`）

## API

### Tauri Commands（24 个）
```
get_project, get_project_by_id, open_file, save_file, update_sequence,
set_roi, clear_roi,
get_features, add_feature, delete_feature, update_feature_ftype, update_feature_color,
update_feature_location,
get_primers, add_primer, delete_primer,
set_methylation,
get_projects, activate_project, delete_project,
open_in_new_window, get_window_project_id
```

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

## 多文件 & 多窗口支持

- `ProjectManager` 内部 `HashMap<String, ProjectData>`，上限 24 个，超出自动驱逐
- **主窗口**：有 Sidebar，通过 `activate_project` 切换 active 项目
- **项目窗口**（label: `project-{id}-{ts}`）：每个 OS 窗口绑定一个特定项目，无 Sidebar，独立操作
- 每个 Tauri 命令通过调用窗口的 label 路由到正确的项目（`webview_window` 参数注入）
- mutation 命令返回完整数据，前端直接用返回值更新状态（不依赖事件广播）
- 关闭 active 项目时自动切换到下一个

## 待优化项

| 优先级 | 问题 | 说明 |
|--------|------|------|
| 中 | SequenceEditor.jsx ~1500 行 | 需拆分组件 |
| 中 | PrimerLayer 重复代码 | 三个组件共享渲染逻辑需抽象 |
| 中 | 引物编辑未接入 undo/redo | 需扩展 editHistory 快照格式 |
| 低 | 前端纯 JS | TypeScript 迁移成本 3-5 天 |
| 低 | FeatureLayer O(n²) 边界检测 | 实际特征数量少，不紧急 |
