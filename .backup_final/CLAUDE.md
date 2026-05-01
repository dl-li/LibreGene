# Geneie — 高级生物序列可视化编辑器

## 项目概述

基于 React + Vite 的质粒/基因序列可视化编辑器。纯 SVG 渲染，支持多行自适应换行、特征注释（含分段特征）、引物可视化、酶切位点标注。

## 技术栈

- React 18 + Vite (oxc 解析器)
- 纯 SVG 渲染，无第三方图形库
- Tailwind CSS (CDN, 仅 App.jsx)
- 字体：TeX Gyre Termes（酶名）、TeX Gyre Heros（特征/引物标签）、monospace（序列）

## 文件结构

```
Geneie/
├── index.html              # @font-face 声明 + 入口
├── vite.config.js
├── package.json
├── assets/Fonts/            # TeX Gyre Termes + Heros (8 个 .otf)
├── src/
│   ├── main.jsx            # 入口
│   ├── App.jsx             # 测试数据 + SequenceEditor 调用（无侧边栏，纯全屏）
│   └── SequenceEditor.jsx  # 核心编辑器 (~500 行)
└── CLAUDE.md
```

## SequenceEditor.jsx 架构

### 常量/纯函数（模块顶层）
| 常量 | 值 | 说明 |
|------|-----|------|
| `cw` | 14 | 字符宽度，所有水平定位的最小单位 |
| `startX` | 150 | 左边缘起始 X |
| `baseSeqY` | 100 | 第一行序列基线 Y |
| `primerTrackGap` | 36 | 重叠引物垂直间距 |
| `getX(col)` | — | 列索引 → 像素 X（左边缘） |
| `complement(c)` | — | 碱基互补 |
| `renderEnzName(name)` | — | 酶名在第二个大写字母处拆分为斜体/正体 |
| `splitRange(start, end, charsPerLine)` | — | 连续索引范围 → 按行 segments |

### 组件状态
- `charsPerLine` — 响应窗口宽度自动适配 (`Math.max(20, floor((w-300)/14))`)
- `hoveredFeature/Primer/Enzyme` — 悬停状态（均为 `null` 或 `id`）

### 数据计算管道

1. **normFeatures** (useMemo) — 统一 feature 格式：简单 `{start,end}` → `{segments:[{start,end}]}`

2. **collision avoidance** (useMemo) → `{ processedFeatures, primerTracks }`
   - Features: 按总长度降序、重叠分配 `trackIdx`。去重按 segments 的 start/end 完全相等判断
   - Primers: Fwd/Rev 独立 track，按可视范围排序

3. **rowAbove / rowBelow → rowY** — 自适应行高（两遍扫描）
   - Pass 1: 逐行计算 `aboveExt`（Fwd 引物标签 + 酶标签堆叠）和 `belowExt`（特征 + Rev 引物）
   - Pass 2: `h = max(internalH, interH)`, `interH = below[r] + above[r+1] + 14`
   - `getSeqY(row)` 返回累积 Y；`svgHeight` 基于末行高度 + 50px

4. **computeHighestY** (useCallback) — 酶标签高度避让
   - 同行 Fwd 引物名称 X 范围与酶标签 X 范围重叠时，抬高酶标签
   - `renderEnzymes` 和 `renderEnzymeOverlay` 共用

5. **enzymeTracks** (useMemo) — 酶标签自身避让（右→左排序，16px/层）

### 渲染层（Z-Index 顺序）
```
renderFeatures()         # 底层：特征线（含分段 gap）+ hover 展开矩形
renderEnzymes()          # 酶竖线 + 标签（不 hover 时被引物遮挡）
renderPrimers()          # 引物线条 + 序列文字 (hover) + 名称标签
renderEnzymeOverlay()    # hover 酶覆层（bgColor 粗线截断引物）
renderSeq()              # 主序列 monospace 文字（最上层）
renderTooltips()         # 酶切弹窗（内切酶/外切酶两种）
```

### 特征渲染规则

- **简单特征**: `{ start, end }` → 内部规范化为 `segments: [{start, end}]`
- **分段特征**: `{ segments: [{start, end}, ...] }`，间隔区域自动补充为 gap 段
- **Gap 段**: 与 solid 同粗（`strokeWidth=5`），透明度 0.25。Hover 时不展开
- **名称标签**: 每行第一个视觉元素（含 gap）显示在左侧。字体 TeX Gyre Heros 600
- **Track 避让**: 间距 `14px`

### 引物渲染规则

- **Fwd**: `matchY=seqY-30-trackOff`, 尾巴左延，箭头右端。Hover 向上展开
- **Rev**: `matchY=seqY+26+trackOff`, 尾巴右延，箭头左端。Hover 向下展开
- **尾巴**: `misY=matchY±4`，超出列边界 5 字符截断，省略号用居中 `···`
- **斜线**: 最后一匹配字符中心 → 第一尾巴字符中心
- **名称标签**: 5' 端，斜体，bgColor 背景遮罩酶线，Hover 淡出。字体 TeX Gyre Heros 600 italic
- **Track 避让**: `primerTrackGap=36`
- **跨行**: 每段独立渲染，无连接线；每行显示名称

### 酶切渲染规则

- **颜色**: 蓝色系 (`#2563EB` / `#dbeafe`)
- **名称**: TeX Gyre Termes，第二个大写字母前斜体 (`renderEnzName`)
- **hover**: 底层渲染 + `renderEnzymeOverlay` 覆层用 bgColor 粗线抹穿引物
- **事件**: `onMouseEnter/Leave` 在外层 `<g>` 上（稳定，不卸载）

## 重要约定

- `matchStart`/`matchEnd` 均为 **inclusive**（闭区间）
- `matchStr` 存储 **模板序列**（左→右），Rev 索引用 `absoluteIndex - matchStart`
- `getX(col)` 返回列左边缘，字符中心需 `+cw/2`
- `splitRange` 的 `colEnd` 是 inclusive，路径右端要 `+cw` 覆盖完整字符
- enzyme 事件在稳定 `<g>` 上，非条件渲染的 `<rect>`
- 纯函数提取到模块顶层

## 常见调试

- **引物字符不对齐**: `matchStr` 长度应为 `matchEnd - matchStart + 1`，`substring(ms, me+1)`
- **酶标签被遮挡**: 检查 `computeHighestY` X 范围重叠判断
- **行间重叠**: `interH = below[r] + above[r+1] + 14`
- **Hover 卡住**: 事件在外层 `<g>` 上，不用条件渲染元素
- **oxc parse error**: JSX 标签/括号配对
