# LibreGene MCP 功能测试总报告

- **测试日期**：2026-08-18
- **被测对象**：LibreGene 桌面应用内嵌 MCP server（`mcp__libregene__*` 工具集，Streamable HTTP @ 127.0.0.1:8766）
- **测试方式**：三项分子克隆任务各委派一个上下文干净的子代理（DeepSeek v4 Flash），**逐个串行**执行；子代理仅通过 MCP 工具完成序列操作
- **测试纪律**：全程不关闭 LibreGene；不修改任何原始输入文件；不读取参考答案（`Answer.txt`、`Reference Result.gbk`）
- **分任务报告**：`alignment/report.md`、`rnai/report.md`、`primer-design/report.md`（含逐条调用记录表，本报告为汇总）

---

## 一、任务总览与结论

| # | 任务 | 目录 | 子代理结论 | 状态 |
|---|------|------|-----------|------|
| 1 | Sanger 测序验证 BbsI 位点移除 | `tasks/Alignment/` | **pVA-seq-2 与 pVA-seq-3 成功**（模板 2998..3003 `GAAGAC` 第 3 位 A→T → `GATGAC`）；pVA-seq-1 未突变 | ✅ 完成 |
| 2 | pNP 转基因 RNAi 质粒构建（AS=TTAGAGGCTACCTTCTTGCTT） | `tasks/Drosophila RNAi/` | **构建成功**，产物 `test-results/rnai/pNP-GOI.gbk`（7895 bp circular），发夹插入、酶切位点重构、引物结合三重验证通过 | ✅ 完成 |
| 3 | mEGFP 克隆引物 + 菌落鉴定引物 + A206K 诱变引物 | `tasks/Primer Design/` | 三部分引物全部设计完成并经 `check_primer_binding` 单一位点验证，答案存于 `test-results/primer-design/primers.md` | ✅ 完成 |

### 各任务最终答案摘要

**任务 1（Alignment）**：模板 BbsI 位点 2998..3003（`GAAGAC`，单切）。seq-1 位点内无差异（保留）；seq-2/seq-3 均在 pos 3000 发生 A→T（位点变为 `GATGAC`，BbsI 不再识别），且突变点恰落在设计引物 pVA-dBbsI-R/F 的缝隙处，两个独立样本互证。附带发现：seq-2 在 3231..3250 有 9 bp 缺失簇（读段 5' 端附近，疑似 Sanger 起始区伪影，建议复核）。

**任务 2（Drosophila RNAi）**：按 protocol 推导 Primer-F/R（各 71 nt），发夹结构 = sense(`AAGCAAGAAGGTAGCCTCTAA`)-loop(`TAGTTATATTCAAGCATA`)-antisense(`TTAGAGGCTACCTTCTTGCTT`)。in silico 克隆：EcoRI+NheI 双酶切切除 617 bp MCS 片段，连入 71 bp 退火产物，8441→7895 bp。验证：插入区逐段序列吻合；EcoRI/NheI 位点两端重构保留（与 pNP 多 shRNA 组装设计自洽）；Primer-F/R 71 nt 完全匹配（annealLen 71/71）；protocol 验证引物 U-F/Ftz 正常结合且坐标平移 −546 精确印证。

**任务 3（Primer Design）**：
- 克隆引物：Fwd `GCGGGATCCTTACTTGTACAGCTCGTCCATG`（BamHI 尾，Tm 60.8）、Rev `GCGAAGCTTATGGTGAGCAAGGGCGA`（HindIII 尾，Tm 61.8），产物 720 bp 无内部酶切位点；
- 菌落 PCR：Fwd `GTAAAACGACGGCCAGTG`（M13 骨架区）+ Rev `ATGGTGAGCAAGGGCG`（mEGFP 5' 端），~791 bp，跨连接处、空载体/反插无条带；
- 诱变引物（A206K，经典单体化位点）：Fwd `TCGTTGGGGTCTTTGCTCAGTTTGGACTGGGTGCTCAGGT`、Rev `ACTACCTGAGCACCCAGTCCAAACTGAGCAAAGACCCCAACG`，工具自检确认 GCG(Ala)→AAA(Lys)、`aaPositionExcludingMet=206`。

---

## 二、MCP 工具调用统计（汇总）

| 类别 | 任务1 | 任务2 | 任务3 | 合计 |
|------|-------|-------|-------|------|
| 调用总次数 | 9 | 14 | 14 | **37** |
| 成功 | 9 | 13 | 14 | **36** |
| 失败 | 0 | 1 | 0 | **1**（2.7%） |
| 重试后成功 | — | 1 | — | 1 |

按工具合计（37 次）：

| 工具 | 次数 | 工具 | 次数 |
|------|------|------|------|
| read_sequence | 7 | get_region_view | 1 |
| find_restriction_sites | 5 | get_project_overview | 1 |
| close_project | 5 | add_feature | 1 |
| open_file | 4 | list_primers | 1 |
| list_projects | 2 | check_primer_binding | 2 |
| add_alignment | 3 | design_primers | 3 |
| edit_sequence | 2 | save_file | 1 |

**结论**：21 个 MCP 工具中本次实际用到 14 个，覆盖读取（read/overview/region_view/search 类）、酶切、比对、编辑、特征、引物设计/验证、文件 I/O 全链路。唯一 1 次失败为调用方笔误被 `expected_old` 乐观校验正确拦截（防护性行为），**无一例工具自身故障**。

---

## 三、错误原因与分析

全程仅 1 次失败调用（任务 2）：

- **现象**：`edit_sequence` 报 `expected_old mismatch at index 1`，附 ±20 bp 上下文与完整 `currentContent`。
- **根因**：子代理手工构造 617 bp 校验串时把开头 `CTAGC…` 抄成 `CATGG…`——**调用方笔误**，引擎乐观校验正确拦截，防止了错误前提下的编辑。
- **恢复**：直接用错误响应中的 `currentContent` 作为 `expected_old` 重试，一次成功。错误体的自恢复设计（差异索引 + 上下文 + 可复制权威值）表现优秀。

另有 2 条**预期警告**（任务 3，非错误）：

1. mutagenesis 对整密码子替换（A→K 三碱基全换）发出"all bases replaced，确认链方向"警告——对该场景属误报性质，靠 mutation 自检块可确认正确。
2. `check_primer_binding` 中克隆引物的 annealLen/Tm 高于 design 值——酶切尾巴（GGATCC/AAGCTT）恰好部分匹配模板真实 BamHI/HindIII 位点，check 按"3' 端连续匹配"口径计入尾巴。工具文档已声明此口径差异，但数值跳升（Tm 60.8→68.6）易造成困惑。

---

## 四、MCP 改进建议（合并三分报告，按优先级去重）

**高价值（任务流卡点）**：

1. **比对差异的区域化查询**（任务 1）：判断"某位点是否被突变"需手工比对 mismatchDetails 坐标与位点坐标的相交关系。建议 `get_region_view` 附带窗口内比对差异明细，或 `add_alignment` 支持 region-of-interest 参数——Sanger 验证/定点突变确认是高频场景。
2. **`add_alignment` 响应增强**（任务 1）：增加按模板方向归一化的读段序列字段（便于直接目检位点窗口），并在 alignments 条目中显式给出读段覆盖起止坐标（当前 `join(...)` 文本需自行解析）。
3. **design_primers（amplify）负链 CDS 方向语义**（任务 3）：负链 CDS 时 fwd 引物落在编码链 3' 端，命名反直觉、易误判为设计错误。建议响应附带 `cds.strand` 提示或注明"产物正链 = 模板正链 seg 区间"。

**易混淆点（文档/口径）**：

4. **坐标基准不一致提示**（任务 2）：`add_feature`/`update_feature` 的 location 为 GenBank 1-based，其余工具均 0-based，建议在工具描述开头加粗强调。
5. **check_primer_binding 尾巴扩展匹配标记**（任务 3）：尾巴意外匹配模板导致 annealLen/Tm 跳升时加 `tailExtendsAnneal` 类标记，并文档化"设计 Tm 与 check Tm 差异的常见场景"。
6. **mutagenesis "all bases replaced" 警告分级**（任务 3）：整密码子替换（预期）与疑似选错链应区分，或在文案中提示整密码子替换属预期。
7. **`edit_sequence` mismatch 恢复路径文档化**（任务 2）：错误响应已含可直接重试的 `currentContent`，在工具描述中明示即可省去手工构造校验串。

**低优先级（体验）**：

8. `find_restriction_sites` 描述中指引"区域切口全景可用 `get_region_view(compact:false)`"，避免盲猜酶名（任务 2）。
9. `read_sequence` 小窗口标尺可紧凑化/可关闭（任务 2、3 均提及）。
10. mutation 自检块的 `aaPosition1Based`/`aaPositionExcludingMet` 双编号值得在工具描述中正面说明（任务 3）。

---

## 五、总体评价

- **功能完备性**：三项真实分子克隆任务（测序验证、酶切连接克隆、引物设计/诱变）全部通过 MCP 独立完成，原语集合（read/edit/酶切/比对/引物）足够覆盖，无需手工绕过。
- **健壮性**：37 次调用仅 1 次调用方错误被防护性拦截且恢复顺畅；无工具故障、无连接中断。
- **数据一致性**：多步编辑后特征平移/裁剪回显、坐标校验、落盘重开独立验证均正确。
- **主要短板**：比对结果的"区域差异视角"缺失（任务 1 的核心诉求需手工推导）；个别口径差异（check vs design 的 annealLen/Tm、1-based location）依赖文档细读，建议按上表改进。

## 六、产出文件清单

```
examples/tasks/test-results/
├── REPORT.md                      ← 本报告
├── alignment/report.md            ← 任务1 完整报告（含逐条调用记录）
├── rnai/
│   ├── report.md                  ← 任务2 完整报告
│   ├── pNP-GOI.gbk                ← 任务2 产物质粒（7895 bp）
│   ├── derivation.txt             ← 发夹推导过程
│   └── insert_seq.txt
└── primer-design/
    ├── report.md                  ← 任务3 完整报告
    └── primers.md                 ← 任务3 最终引物答案
```

原始输入文件（`pVA-MCS.dna`、`*.ab1`、`pNP(Vm2).dna`、`BlueScribe-mEGFP.gbk`）全程未修改；参考答案文件全程未读取；LibreGene 应用保持运行，测试结束后各子代理打开的 MCP 项目均已 `close_project` 清理。
