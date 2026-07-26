# LibreGene — 质粒编辑器

基于 React + Vite + Tauri v2 + Rust 的桌面质粒编辑器。纯 SVG 渲染，支持多行自适应换行、分段特征、引物可视化、酶切位点标注、序列比对、插件系统。默认输出增强型 GenBank 文件（含颜色和引物注释）。

**这是 Tauri v2 桌面应用，不要用浏览器测试，必须用 `npx tauri dev` 启动。**
**仅在 macOS 上测试过。**

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
cargo build -p LibreGene                    # 构建 Tauri 后端

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
├── test/                       # 公开合成测试序列 (.dna, .gbk)
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
│   ├── fileIcons.js            # 文件名 → lucide 图标映射
│   ├── components/
│   │   ├── DebugPanel.jsx      # 调试面板
│   │   ├── PrimerOverviewDialog.jsx  # 引物总览弹窗
│   │   ├── SettingsPage.jsx    # 设置页面
│   │   ├── TitleBar.jsx        # 无框窗口自定义标题栏
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
│   │       ├── enzyme/         # 酶切引擎
│   │       ├── primer/         # 引物引擎
│   │       └── file_io/        # 文件解析/序列化
│   └── test_data/
└── src-tauri/                  # Tauri v2 桌面壳
    ├── Cargo.toml
    ├── tauri.conf.json
    └── src/
        ├── lib.rs              # Tauri commands + AppState
        └── main.rs             # 入口
```

## 编码准则

### 通用

- **尽量不写注释**——代码本身应该表意清晰。必要时写简短注释说明 Why（不是 What）。
- 先读后改：改任何文件前，先 `Read` 理解上下文。
- 改完后必须编译/构建验证。前端：`npx vite build`。后端：`cargo test -p libregene-core --lib`。
- Rust 代码同时跑 `cargo build -p LibreGene` 确保 Tauri 壳也编译。
- 默认已通过 `tmux` 在后台运行 `npx tauri dev`，改 UI 后切到 tmux 看效果即可。

### Bug 修复流程

1. 开始修复前先用 `git status` + `git log --oneline -5` 确认当前状态
2. **每修一个 Bug 就单独提交一次**（`git add` 只包含相关的改动文件）
3. 提交前跑对应的测试和构建
4. 提交信息用英文，格式：`fix: 简短描述` 或 `refactor: 简短描述`
5. 涉及 UI 的改动用 tmux 中的 Tauri dev 验证

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

- 特征：始终展开特征（Always Expand Feature）
- 引物：我的引物（My Primers）、PCR 分析、引物设计、选项
- 酶切：自定义酶集合、酶数据库、酶切分析

导航菜单使用 `src/components/ui/dropdown-menu.jsx`（基于 `@radix-ui/react-dropdown-menu`，通过 shadcn 方式添加）。

## API

### Tauri Commands

```
get_project, get_project_by_id, open_file, save_file, update_sequence,
set_roi, clear_roi,
get_features, add_feature, delete_feature,
update_feature_ftype, update_feature_color, update_feature_name,
update_feature_strand, update_feature_location,
get_primers, add_primer, delete_primer, compute_primer_alignment,
add_alignment, add_alignment_seq, remove_alignment,
set_methylation,
get_projects, activate_project, delete_project,
open_in_new_window, get_window_project_id, rekey_project
```

### HTTP API (libregene serve)

统一前缀 `http://127.0.0.1:8765`，见 `src/api.js`。

## 核心模型约定

- `Feature.start/end` — 0-based inclusive
- `Feature.segments[]` — 分段特征的多段列表，每个 `{ start, end }` 0-based inclusive
- `PrimerBindingSite.template_start` — 0-based inclusive
- `PrimerBindingSite.template_end` — 0-based exclusive
- `Enzyme.cut_index / bot_cut_index` — 切口在 cutIndex-1 与 cutIndex 之间，0-based
- `BindingSite.matchStart/End` — inclusive
- 环状序列坐标用 `% tlen` 归一化，`wrap_template_region` 负责处理环状拼接

