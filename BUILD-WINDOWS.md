# LibreGene — Windows 构建指南

本文档记录在 Windows 上从零构建 LibreGene 的完整流程，包括工具链安装、踩过的坑和验证结果。

## TL;DR

**结论：LibreGene 无需任何代码改动即可在 Windows 上编译和运行。**

作者标注的"仅 macOS 测试"只是未经验证，并非代码层面存在 Windows 不兼容。实测产物 `LibreGene.exe`（14.5 MB）可在 Windows 11 上正常启动并加载窗口。

---

## 一、环境要求

| 组件 | 版本（实测） | 说明 |
|---|---|---|
| Windows | 11 (10.0.26200) | Windows 10 亦可 |
| Node.js | v24.14.1（≥ 20 即可） | 前端构建 |
| Rust | 1.97.1（stable） | `x86_64-pc-windows-msvc` host |
| MSVC | 14.44.35207（VS 2022 Build Tools） | C/C++ 工具链 + 链接器 |
| Windows SDK | 11 (22621) | 编译 Windows API 调用 |
| WebView2 Runtime | 150.x | 已随 Edge 预装，无需单独装 |

---

## 二、工具链安装

三个脚本位于 [`scripts/setup-windows/`](scripts/setup-windows/)，按顺序右键 → "用 PowerShell 运行"（需管理员权限）：

### 1. 安装 Rust
```
scripts\setup-windows\1-install-rust.ps1
```
- 下载官方 `rustup-init.exe`
- 安装 `stable` + `x86_64-pc-windows-msvc` host + default profile
- 耗时约 5 分钟，约 250 MB

### 2. 安装 VS 2022 Build Tools
```
scripts\setup-windows\2-install-vs-buildtools.ps1
```
- 下载官方 `vs_BuildTools.exe` 引导程序
- 用 `.vsconfig` 静默安装以下组件：
  - `Microsoft.VisualStudio.Workload.VCTools`（C++ 桌面开发）
  - `Microsoft.VisualStudio.Component.VC.Tools.x86.x64`（MSVC 编译器）
  - `Microsoft.VisualStudio.Component.Windows11SDK.22621`（Windows 11 SDK）
  - `Microsoft.VisualStudio.Component.VC.Redist.14.Latest`（运行时）
- 耗时约 20-40 分钟，约 3-4 GB

### 3. 验证（可选）
```
scripts\setup-windows\3-verify-and-build.ps1
```
- 加载 `vcvars64.bat` 并跑 `cargo check -p libregene-core`

> **注**：第 3 个脚本依赖 PowerShell 管道，cargo 的进度行可能被缓冲看起来像卡住。建议直接看下面的"手动构建"，更可靠。

---

## 三、手动构建（推荐）

工具链装好后，在 **Git Bash** 或 **PowerShell** 里：

```bash
cd LibreGene

# 1. 前端依赖
npm install --legacy-peer-deps --no-audit --no-fund

# 2. 前端构建（可选验证，tauri build 会自动调）
npx vite build

# 3. 完整 Tauri 构建
export PATH="$HOME/.cargo/bin:$PATH"   # Git Bash 才需要
npx tauri build
```

产物：
```
src-tauri/target/release/LibreGene.exe     # 可执行文件 (~14.5 MB)
```

首次构建约 **8-10 分钟**（下 400+ crate + 编译），后续增量构建 1-2 分钟。

### 开发模式（热重载）
```bash
npx tauri dev
```

---

## 四、踩过的坑

### 坑 1：ESLint 依赖冲突（项目自身问题，非 Windows 特定）

**现象**：`npm install` 失败，报 `eslint@10` 与 `eslint-plugin-react@7.37.5` peer dependency 冲突。

**原因**：`package.json` 里 `"eslint": "^10.7.0"` 太新，而 `eslint-plugin-react@7.37.5` 只支持到 eslint 9.7。这个问题在 macOS 上同样存在。

**当前处理**：用 `--legacy-peer-deps` 跳过（npm 容忍冲突）。

**建议修复**（需上游单独处理）：把 `package.json` 里 `eslint` 降到 `"^9.7.0"`，或升 `eslint-plugin-react` 到支持 eslint 10 的版本。

### 坑 2：PowerShell 管道吞掉 cargo 进度输出

**现象**：`cargo check` 通过 PS 脚本跑时，stdout 停在 "cargo check" 那行十几分钟没动静，看起来像卡死。

**原因**：PowerShell 的 `& cargo ... 2>&1` 重定向时，cargo 带 `\r` 回车刷新的进度行（`Compiling xxx`）会被管道缓冲，直到进程结束才 flush。

**处理**：改用 Git Bash 直接跑 `cargo check`，输出实时可见。**不需要手动加载 `vcvars64.bat`**——`cc` crate 会自动发现 MSVC 工具链。

### 坑 3：MSI 打包时 WiX 下载超时（网络问题，非 Windows 适配问题）

**现象**：`npx tauri build` 编译链接全部成功，生成 `LibreGene.exe`，但最后打包 `.msi` 时失败：
```
Downloading https://github.com/wixtoolset/wix3/releases/download/wix3141rtm/wix314-binaries.zip
failed to bundle project `timeout: global`
```

**原因**：Tauri 打 `.msi` 需要下载 WiX 工具集，国内网络访问 github.com 超时。

**处理**：
- `.exe` 本身已完整可用，不需要 `.msi` 安装包即可运行
- 如确实需要 `.msi`：手动从镜像下载 `wix314-binaries.zip`，解压到 `%LOCALAPPDATA%\tauri\WixTools314\`
- 或改用 NSIS 打包（修改 `tauri.conf.json` 的 `bundle.targets` 为 `["nsis"]`）

### 坑 4：cargo 首次 fetch 偶发卡住

**现象**：首次 `cargo check` 时，下载完 132 个 crate 后 cargo 进程 CPU=0、无 rustc 子进程，看似死锁。

**原因**：某些 crate 下载未完成，cargo 在静默重试等待。

**处理**：手动跑一次 `cargo fetch --verbose` 把依赖下全，再 `cargo check` 即可顺畅。

---

## 五、验证结果

构建完成后实测（2026-07-27，Windows 11）：

```
LibreGene.exe
  Size:         14.5 MB
  Version:      1.0.0
  Product:      LibreGene
  启动:         成功
  窗口标题:     "LibreGene"（WebView2 加载正常）
  内存占用:     ~37 MB（启动后稳定）
```

---

## 六、关于"代码无需改动"的说明

通读源码后确认，项目在 Windows 适配方面**已经做了准备**：

- `src/components/TitleBar.jsx` 已实现 Windows/Linux 风格的 caption 按钮（最小化/最大化/关闭），并通过 `navigator.userAgent` 自动切换
- `src-tauri/src/main.rs` 已加 `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]`（避免 release 弹出控制台窗口）
- `src-tauri/tauri.conf.json` 已包含 `icons/icon.ico`
- 后端 Rust 代码（`src-tauri/src/lib.rs`、`libregene-core`）无任何 `#[cfg(unix)]` / `#[cfg(windows)]` 平台特定分支，全部使用 Tauri 跨平台 API

因此本次 Windows 适配**没有修改任何源代码**，全部工作集中在工具链搭建和构建验证。

---

## 七、脚本说明

[`scripts/setup-windows/`](scripts/setup-windows/) 目录下的文件：

| 文件 | 用途 |
|---|---|
| `1-install-rust.ps1` | 装 Rust（MSVC host） |
| `2-install-vs-buildtools.ps1` | 装 VS 2022 Build Tools |
| `.vsconfig` | VS 安装组件清单（被脚本 2 引用） |
| `3-verify-and-build.ps1` | 验证工具链 + cargo check（自动定位仓库根，提示 PS 进度缓冲属正常） |

这些脚本随仓库一同分发，Windows 用户可按"二、工具链安装"的顺序执行，无需手动下载。
