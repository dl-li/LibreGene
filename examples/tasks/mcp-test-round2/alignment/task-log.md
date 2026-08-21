# MCP 功能测试记录 —— 测序验证 BbsI 位点移除

测试员：Kimi Code CLI（MCP Agent）
任务：比对三条 .ab1 Sanger 测序读段到参考质粒 pVA-MCS，判断哪些样本成功移除了 BbsI 酶切位点。
工作目录：`/Users/lidonglin/LibreGene/examples/tasks/mcp-test-round2/alignment/`

---

## 工作日志

### 1. 打开参考质粒

参考质粒 `pVA-MCS.dna` 为 SnapGene 格式。先用 `open_file` 加载为项目，以获得 `project_id`，再对项目进行操作。

**观察**：项目加载成功，质粒为 5214 bp 环状 DNA。FEATURES 中已有一个名为 `bbs1` 的 misc_feature，位置 `2999..3004`，颜色 #a6acb3，应即为待验证的 BbsI 位点。

### 2. 寻找参考质粒上的 BbsI 酶切位点

虽然 feature 标注已提示位置，但仍通过 `find_restriction_sites` 确认 BbsI 在参考序列上的精确识别位点和切口，以便后续判断读段差异是否落在识别序列内。

**BbsI 位点确认**：参考质粒上仅 1 个 BbsI 位点，识别序列 `GAAGAC`，模板正链位置 `2999..3004`，top 链切口 `3006^3007`，bottom 链切口 `3010^3011`。该位点落在 `bbs1` feature 内。

### 3. 添加三条 Sanger 测序读段

使用 `add_alignment` 将三条 `.ab1` 文件分别比对到参考质粒。长读段推荐用 `path` 输入。

**三条读段均已成功比对**：
- pVA-seq-1（aln-1）：负链，覆盖 3772..5214 与 1..3771，identity 99.92%，差异在 3911、185，插入在 5、396。
- pVA-seq-2（aln-2）：正链，覆盖 3229..5214 与 1..3221，identity 99.69%，差异集中在 3232–3256 一段（大量缺失/插入）以及 3001、3911 等。
- pVA-seq-3（aln-3）：负链，覆盖 3888..5214 与 1..3887，identity 99.90%，差异在 3911、185、3001。

### 4. 查看 BbsI 位点区域差异

BbsI 识别位点位于 `2999..3004`。为判断哪些读段在该位点发生突变，使用 `get_region_view` 查看 2995..3010 区间的比对差异；同时用 `read_sequence` 确认参考序列上下文。

**BbsI 位点区域结果（2995..3010）**：
- 参考序列：`CGCACTAGTGAAGACGAGCTC`，其中 `2999..3004 = GAAGAC` 为 BbsI 识别位点。
- pVA-seq-1：窗口内无任何差异，BbsI 位点保持野生型。
- pVA-seq-2：在 `3001` 处有一个错配 `A > T`，识别序列变为 `GATGAC`。
- pVA-seq-3：在 `3001` 处同样有一个错配 `A > T`，识别序列变为 `GATGAC`。

这说明 **pVA-seq-2 与 pVA-seq-3** 在 BbsI 识别序列内部发生了单碱基替换，破坏了 BbsI 位点；**pVA-seq-1** 未突变。

---

## MCP 工具调用记录

| 工具名 | 关键参数 | 成功/失败 | 结果摘要 |
|---|---|---|---|
| open_file | path: pVA-MCS.dna | 成功 | 项目 ID 为文件路径；5214 bp 环状 DNA；已标注 bbs1 特征 2999..3004 |
| find_restriction_sites | enzymes: ["BbsI"] | 成功 | 参考质粒上唯一 BbsI 位点：2999..3004 GAAGAC，top 切口 3006^3007，bottom 切口 3010^3011 |
| add_alignment | path: pVA-seq-1.ab1, name: pVA-seq-1 | 成功 | aln-1，负链，identity 99.92%，差异 3911、185；插入 5、396 |
| add_alignment | path: pVA-seq-2.ab1, name: pVA-seq-2 | 成功 | aln-2，正链，identity 99.69%，差异含 3001 A>T；另有 3232–3256 区域大量 indel |
| add_alignment | path: pVA-seq-3.ab1, name: pVA-seq-3 | 成功 | aln-3，负链，identity 99.90%，差异含 3001 A>T，以及 185、3911 |
| get_region_view | start: 2995, end: 3010 | 成功 | 窗口内仅 pVA-seq-2、pVA-seq-3 在 3001 有 A>T；pVA-seq-1 无差异 |
| read_sequence | start: 2990, end: 3010 | 成功 | 参考序列 `CGCACTAGTGAAGACGAGCTC`，确认 2999..3004 = GAAGAC |

---

## 卡顿点记录

1. **add_alignment 返回信息量巨大**：每次返回完整的 orientedSequence 与全质粒 regionView，直接看难以定位 BbsI 位点差异。解决：后续用 `get_region_view` 对 2995..3010 窗口单独查询，差异一目了然。
2. **pVA-seq-2 存在大片段indel干扰**：该读段在 3232–3256 区域有 5 处缺失（共 9 bp）和 1 处插入，乍看差异很多，容易让人误判为整体测序质量差。但聚焦 BbsI 窗口后，发现其关键位点突变与 pVA-seq-3 一致。
3. **BbsI 切口位置在识别序列之外**：`find_restriction_sites` 返回 topCutIndex=3006、botCutIndex=3010，位于识别序列 2999..3004 下游，属 BbsI（type IIS）正常特性；初次查看时可能需要确认“切口位置不等于识别序列位置”。
4. **坐标方向问题**：三条读段 strand 不同（seq1/3 为 -，seq2 为 +），但 `mismatchDetails` 已按模板正链归一化，因此可直接用 `templateBase > readBase` 判断突变，无需手动反补。

---

## 结论

**成功移除 BbsI 酶切位点的样本：pVA-seq-2 和 pVA-seq-3。**

**证据**：
- 参考质粒中 BbsI 识别序列位于 **2999..3004**（`GAAGAC`），对应参考序列上下文 `2990..3010 = CGCACTAGTGAAGACGAGCTC`。
- **pVA-seq-2** 与 **pVA-seq-3** 在 **3001** 位点均检测到错配：**模板碱基 A → 读段碱基 T**，使识别序列由 `GAAGAC` 变为 `GATGAC`，不再被 BbsI 识别。
- **pVA-seq-1** 在 2995..3010 窗口内无任何差异，BbsI 位点保持野生型，未成功移除。

---

| close_project | project_id: pVA-MCS.dna | 成功 | 已关闭本次打开的参考质粒项目，未修改序列、未保存文件 |

**任务结束**：未修改质粒序列，未保存文件，已关闭项目。
