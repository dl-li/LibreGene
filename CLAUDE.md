# Geneie — 高级生物序列可视化编辑器

## 项目概述

基于 React + Vite 前端 + Python/FastAPI 后端的质粒编辑器。纯 SVG 渲染，支持多行自适应换行、分段特征、引物可视化、酶切位点标注。插件化架构，Agent 友好 CLI。

## 技术栈

- **前端**: React 18 + Vite (oxc)，纯 SVG，Cascadia Code / TeX Gyre Heros 字体
- **后端**: Python 3.12, Biopython, Click, FastAPI, uv 管理
- **桌面**: Tauri（计划中）

## 文件结构

```
Geneie/
├── index.html              # @font-face (Cascadia Code + TeX Gyre Heros + Termes)
├── vite.config.js
├── package.json
├── assets/Fonts/            # 14 个字体文件
├── src/
│   ├── main.jsx
│   ├── App.jsx             # 测试数据（硬编码常量）
│   ├── SequenceEditor.jsx  # 核心编辑器 (~600 行)
│   └── api.js              # 后端 API 客户端
├── backend/
│   ├── geneie_core/        # 核心：models, cli, server, file_io, plugin mgr
│   ├── geneie_plugins/     # 插件目录
│   │   └── enzyme_engine/  # 官方酶切插件
│   └── pyproject.toml
└── CLAUDE.md
```

## 前端架构 (SequenceEditor.jsx)

### 常量（模块顶层）
| 常量 | 值 | 说明 |
|------|-----|------|
| `cw` | 14 | 字符宽度 |
| `startX` | 220 | 左右页边距 |
| `primerTrackGap` | 36 | 重叠引物垂直间距 |
| `getX(col)` | — | 列左边缘，字符中心 +cw/2 |
| `measureWidth(text, font)` | — | Canvas 2D 文本宽度缓存 |
| `splitRange(start, end, charsPerLine)` | — | 索引范围 → 按行 segments |

### 渲染层（Z-Index）
```
renderFeatures() → renderEnzymes() → renderPrimers() →
renderEnzymeLabels() → renderEnzymeOverlay() → renderSeq() → renderTooltips()
```

### 引物
- Fwd: `matchY=seqY-30-trackOff`, Rev: `matchY=seqY+26+trackOff`
- 尾巴: `misY=matchY±4`, 超出行边界 5 字符截断 `···`
- 标签: 5' 端，TeX Gyre Heros 600 italic, bgColor 背景

### 酶切
- 独特: Cascadia Code 700, `sw=2, swM=3`
- 非独特: Cascadia Code 350, `sw=1, swM=1.5`, 竖线 0.8
- 弹窗: 白底，识别 700 Bold，非识别 200 ExtraLight
- spacer 支持 (DraIII 等)

### 特征
- 支持 `{start,end}` 和 `{segments:[...]}`, gap 段同粗低透明度
- 每行首元素显示标签，TeX Gyre Heros 600
- Track 18px

### 重要约定
- `matchStart/End` inclusive, `matchStr` = 模板序列
- Rev 索引: `absoluteIndex - matchStart`
- 路径右端 +cw 覆盖完整字符
- 酶 hover 事件在稳定 `<g>` 上

## 后端架构

### 核心模块
| 模块 | 职责 |
|------|------|
| `models.py` | 6 个 dataclass (ProjectData, Feature, Primer, Enzyme 等) |
| `cli.py` | Click CLI, 11 个命令 |
| `server.py` | FastAPI, 17 个端点 + WebSocket |
| `file_io.py` | .gbk 100% round-trip, .dna, .fasta 解析 |
| `plugin_protocol.py` | PluginProtocol + PluginManifest |
| `plugin_manager.py` | 自动发现并加载插件 |
| `project_manager.py` | 内存状态管理 + 变更回调 |

### CLI 命令
```bash
geneie open /path/file.gbk    geneie save /path/out.gbk
geneie roi set 10..280        geneie roi clear / show
geneie feature list           geneie enzyme list [--unique]
geneie seq show / select      geneie export [--output path]
geneie serve [--port 8765]
```

### REST API (17 routes)
```
GET    /project                     POST   /open?path=
POST   /save?path=                  PUT    /sequence
POST   /roi?s=&e=                   POST   /roi/clear
GET    /features                    POST   /features          DELETE /features/{id}
GET    /primers                     POST   /primers           DELETE /primers/{id}
WS     /ws                          (real-time push)
```

### .gbk 保真
- Features → native GenBank features (compound locations for segments)
- Primers → `primer_bind` features with `geneie_*` qualifiers
- Custom fields in qualifiers ≤20 chars
- Enzymes recomputed by plugin, not persisted

### 前端 API (src/api.js)
```js
import { getProject, openFile, addFeature, connectWS } from './api.js';
const ws = connectWS(data => setProject(data));
```
