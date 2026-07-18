<p align="center">
  <img src="src-tauri/icons/icon.svg" alt="LibreGene" width="128" />
</p>

<h1 align="center">LibreGene — 质粒编辑器</h1>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-GPLv3-blue.svg" alt="License: GPL v3" /></a>
</p>

> **⚠️ 早期开发阶段** — 接口和文件格式尚未稳定，可能会有破坏性变更。
>
> [English README](README.md)

**LibreGene** 是一款轻量级跨平台桌面质粒编辑器，基于 React + Vite + Tauri v2 + Rust 构建，采用纯 SVG 渲染。自由开源，满足日常分子克隆需求。

## 功能特性

- **SVG 渲染** — 平滑可缩放的质粒图谱，支持多行自适应换行显示
- **特征标注** — 注释 CDS、启动子、终止子等，支持复合（joined）特征，自定义颜色和链方向
- **引物** — 添加、比对和可视化引物结合位点，基于最近邻热力学计算 Tm 值
- **限制性内切酶** — 内置 900+ 酶数据库，支持甲基化感知过滤、单/唯一切割位点视图
- **多窗口** — 同时在多个独立 OS 窗口中打开不同质粒
- **多项目标签页** — 侧边栏切换多个已打开质粒
- **撤销/重做** — 序列编辑和特征修改的完整历史记录
- **GenBank 读写** — 解析和写入 .gb/.gbk 文件，支持含颜色和引物注释的增强格式
- **甲基化分析** — 检测甲基化敏感型和甲基化依赖型酶切模式

### 优点

- **轻量化**（~15 MB 二进制）— 远小于同类商业软件
- **免费开源** — 无需许可证，无需订阅
- **快速** — 纯 Rust 后端，React + SVG 渲染，无重型原生控件
- **跨平台** — macOS、Windows、Linux（基于 Tauri v2）

### 局限性

- **早期阶段** — 许多功能仍为占位/原型状态（PCR 分析、引物设计、酶数据库管理）
- **单文件模式** — 不支持多记录 GenBank 或数据库管理
- **无序列比对** — BLAST / 双序列比对尚未实现

## 快速开始

### 前置要求

- Node.js ≥ 20
- Rust ≥ 1.75
- Tauri v2 系统依赖：[tauri.app/start/prerequisites](https://v2.tauri.app/start/prerequisites/)

### 开发

```bash
# 安装前端依赖
npm install

# 启动桌面应用
npx tauri dev
```

### 命令

| 命令 | 说明 |
|------|------|
| `npm run dev` | Vite 开发服务器（不可单独使用） |
| `npm run build` | 仅前端编译检查 |
| `npx tauri dev` | **启动桌面应用** |
| `npx tauri build` | 构建生产版本 |
| `npm run format` | Prettier 格式化 |
| `npm run lint` | ESLint 检查 |

#### Rust 后端

```bash
cd backend
cargo test -p libregene-core --lib              # 单元测试（117 项）
cargo test -p libregene-core --test roundtrip_test   # 读写往返测试
cargo build -p libregene                           # 构建 Tauri 后端
```

## 项目结构

```
LibreGene/
├── src/                  # 前端 React 源码
│   ├── App.jsx           # 顶层状态 + 侧边栏 + 路由
│   ├── SequenceEditor.jsx# 核心 SVG 编辑器（~2474 行，待拆分）
│   ├── editorConstants.js# 布局常量（cw, startX, baseSeqY）
│   ├── editHistory.js    # 撤销/重做历史栈
│   └── components/       # shadcn 基础 UI 组件
├── backend/              # Rust 工作空间
│   ├── libregene-core/   # 核心库：模型、酶、引物、文件 IO
│   └── test_data/        # 集成测试数据（JSON 导出）
├── src-tauri/            # Tauri v2 桌面壳
│   └── src/              # Tauri 命令 + AppState
├── assets/Fonts/         # 字体文件（Cascadia Code, TeX Gyre）
├── public/assets/Fonts/  # Vite 开发服务器用字体副本
└── test/                 # 测试 .dna / .gbk 文件
```

## 路线图

`EditorNavMenu.jsx` 中以下菜单项为占位（disabled，标注"即将推出"）：

- **特征**：始终展开特征
- **引物**：我的引物、PCR 分析、引物设计、选项
- **酶切**：自定义酶集合、酶数据库、酶切分析

详见 [AGENTS.md](AGENTS.md) 了解内部开发说明。

## 许可证

Copyright (C) 2025 dl-li

本程序是自由软件：你可以再分发和/或修改它，前提是遵守由自由软件基金会发布的 GNU 通用公共许可证（GNU General Public License）的条款，无论是许可证的第 3 版，还是（按你的选择）任何更新的版本。

本程序的发布是希望它会有用，但**不提供任何担保**；甚至没有对适销性或特定用途适用性的暗示担保。详情请见 GNU 通用公共许可证。

你应该已经随本程序收到一份 GNU 通用公共许可证的副本。如果没有，请访问 <https://www.gnu.org/licenses/>。

### 第三方许可

参见 [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md) 了解捆绑字体和依赖项的许可证信息。
