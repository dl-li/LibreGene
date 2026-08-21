# LibreGene MCP 功能测试报告（第二轮）

- **测试时间**：2026-08-19
- **测试方式**：主代理逐项（串行）委派 3 个上下文干净的子代理（k2.7），仅通过 LibreGene MCP 工具完成任务；不修改原始任务文件（全部在 `mcp-test-round2/` 的工作副本上进行）；任务执行期间子代理与主代理均未阅读参考答案，答案仅在任务完成后用于核对。
- **总览**：3 项任务全部完成，结论均正确。**MCP 调用合计 31 次，失败 0 次，重试 0 次。**

| 任务 | 子代理 | MCP 调用 | 失败 | 结论正确性 |
|---|---|---|---|---|
| 1. Alignment（测序验证 BbsI 位点移除） | agent-14 | 8 | 0 | ✅ 与参考答案完全一致 |
| 2. Drosophila RNAi（pNP shRNA 质粒构建） | agent-15 | 13 | 0 | ✅ 全长 7895 bp 与参考仅差 2 bp，且差异为参考自身偏离 protocol 所致（见下） |
| 3. Primer Design（mEGFP 克隆/鉴定/诱变引物） | agent-16 | 10 | 0 | ✅ 引物功能与参考等价，设计风格略有差异（见下） |

各任务的逐条调用记录与完整日志见各子目录的 `task-log.md`；产物为 `rnai/result.gbk`、`primer-design/result.gbk`。

---

## 任务 1：Alignment —— 哪个样本成功移除了 BbsI 位点

**结论：pVA-seq-2 与 pVA-seq-3 成功（3001 位 A>T，GAAGAC→GATGAC）；pVA-seq-1 未突变。与参考答案（Answer.txt）完全一致。**

### 任务流程

1. `open_file` 打开 pVA-MCS.dna（5214 bp 环状；已有 `bbs1` feature 标注 2999..3004）。
2. `find_restriction_sites(BbsI)` 确认唯一识别位点 2999..3004（GAAGAC），切口 3006^3007 / 3010^3011。
3. `add_alignment` ×3 比对三条 .ab1 读段（均成功，identity 99.69–99.92%）。
4. `get_region_view(2995..3010)` 的 `ALIGNMENT DIFFS IN REGION` 节直接给出答案：seq-2/seq-3 在 3001 有 A>T，seq-1 无差异。
5. `read_sequence` 复核参考上下文，`close_project` 收尾。

### 卡顿点

1. `add_alignment` 返回全量 `orientedSequence` + 全质粒 regionView，信息量大，需再调一次 `get_region_view` 聚焦。
2. pVA-seq-2 在 3232–3256 有成片 indel（测序质量问题区），有干扰性，需聚焦窗口排除。
3. BbsI 为 type IIS 酶，切口在识别序列下游（3006^3007），与识别位点 2999..3004 分离，初次查看需分辨。
4. （正面）读段 strand 不同，但 `mismatchDetails` 已按模板正链归一化，无需手动反补——全程零逐字序列分析。

---

## 任务 2：Drosophila RNAi —— pNP(Vm2) shRNA 质粒构建

**产物：`rnai/result.gbk`，7895 bp。全长与 Reference Result.gbk 逐碱基比对仅差 2 bp（3954、3963），均位于 shRNA 茎的 sense 臂。**

### 任务流程

1. 阅读 protocol.txt：Primer-F = `ctagcagt + "As re-com"(=sense) + tagttatattcaagcata(loop) + "As" + gcg`，退火后连入 EcoRI/NheI 双酶切的 pNP。
2. `open_file` 打开 pNP(Vm2).dna（8441 bp）。
3. `find_restriction_sites(EcoRI, NheI)`：NheI 3935^3936、EcoRI 4552^4553，均唯一。
4. `read_sequence` 读取 617 bp 待替换区作为 `expected_old`。
5. `edit_sequence(3936..4552 → 71 bp 插入)` 一次成功，产物 7895 bp。
6. `add_feature` 标注 `shRNA-GOI`（3936..4006）。
7. 自查：`search_sequence` 验证 NheI/EcoRI 位点恢复且唯一、AS 臂（3983..4003）、sense 臂、loop（3965..3982）均正确；`save_file` 保存。

### 与参考答案的 2 bp 差异分析（重要）

- 子代理的 sense 臂 = 给定 AS 的**精确反向互补** `AAGCAAGAAGGTAGCCTCTAA`，形成完美 21 bp 茎，完全符合 protocol 对 "As re-com" 的定义。
- 参考文件的 sense 臂为 `AAGCAAGAAGCTAGCCTCTCA`，与 revcomp(AS) 有 2 个错配（3954 C/G、3963 C/A），茎不完美——**参考文件自身偏离了 protocol 公式，子代理的产物反而更忠实于 protocol**。其余 7893 bp 完全一致（酶切位点恢复、loop、AS 臂、连接处均相同）。
- 判定：任务完成，差异不归咎于 Agent 或 MCP。

### 卡顿点

1. protocol 的 Primer-F/R 写法抽象，需读取切口两侧实际序列手工推导哪条链是产物正链、NheI/EcoRI 黏性末端如何恢复（`G + CTAGC` / `G + AATTC`）。
2. loop 序列 `tagttatattcaagcata` 需逐字计数 18 nt（防多/漏 T）——为数不多的手动逐字分析点，最终靠 `search_sequence` 唯一命中确认。
3. 617 bp 的 `expected_old` 需先 `read_sequence` 取一次（多一次往返，但避免了手抄错误，属良性开销）。

---

## 任务 3：Primer Design —— mEGFP 克隆、菌落鉴定、A206K 诱变引物

**产物：`primer-design/result.gbk`（原序列 + 4 条新引物，未改序列）。**

### 任务流程与答案

1. `open_file` BlueScribe-mEGFP.gbk（3442 bp，mEGFP 在负链 450..1169）。
2. `find_restriction_sites(BamHI, HindIII)`：BamHI 444..449（上游）、HindIII 1170..1175（下游），mEGFP 内部无位点。
3. **Q1 扩增引物** `design_primers(amplify, seg=450..1169, fwd_enzyme=BamHI, rev_enzyme=HindIII)`：
   - `mEGFP-BamHI-F`：`GCGGGATCCTTACTTGTACAGCTCGTCCAT`（Tm 67.8）
   - `mEGFP-HindIII-R`：`GCGAAGCTTATGGTGAGCAAGGGCG`（Tm 65.5）
4. **Q2 菌落 PCR**：复用 Q1 引物对（插入特异性，阳性 ~720 bp，空载体无条带），备选 M13 Fwd/Rev（阳性 ~843 bp）。
5. **Q3 诱变引物**：确认文献编号 A206 = 编码链第 207 密码子 → 模板正链 549..551（CGC/GCG=Ala）；`design_primers(mutagenesis, seg=549..551, mut_seq=CTT)`，工具确认 `aaPositionExcludingMet=206, Ala(GCG)→Lys(AAG)`：
   - `mEGFP-A206K-F`：`TCGTTGGGGTCTTTGCTCAGCTTGGACTGGGTGCTCAGG`（Tm 59.8）
   - `mEGFP-A206K-R`：`ACTACCTGAGCACCCAGTCCAAGCTGAGCAAAGACCCCAA`（Tm 60.2）
6. `add_primer` ×4 落库（各 1 个结合位点），`save_file` 保存。

### 与参考答案对比

- Q1：参考为 `gtcactGGATCC+20nt` / `tcagtgAAGCTT+20nt`；子代理为 `GCGGGATCC+21nt` / `GCGAAGCTT+16nt`。尾巴保护碱基与退火长度不同，但酶切位点、方向、覆盖区间等价，功能相同。✅
- Q3：参考为经典 QuikChange 风格（两条引物完全互补重叠 531..569，突变居中）；子代理采用 `design_primers` mutagenesis 引擎输出的部分重叠/背对背风格（突变在 5' 侧，3' 退火核心 16–18 bp）。风格不同但均为有效的诱变设计，且这是工具引擎自身的输出风格。✅

### 卡顿点

1. **mEGFP 在负链，方向易混**：靠 `design_primers` 的 `orientation` 说明才确认产物正链 = 模板正链、酶切尾巴分配正确。
2. **A206 → 模板坐标需人工反推**：负链 CDS 的密码子顺序与正链相反，子代理手工计算"第 207 密码子 → 正链 549..551"并用 `read_sequence` 验证——**这是全流程中最易幻觉出错的人工步骤**。
3. **Tm 双口径**：`design_primers` 返回退火核心 Tm，`add_primer`/`check_primer_binding` 返回实际 3' 连续匹配 Tm（含尾巴意外配对时值更高），子代理一度困惑，最终以落库 Tm 为准。
4. **`mut_seq` 必须给正链内容**：负链 CDS 的 Lys 密码子 AAG 需手动反补为 `CTT`，又一次手动反补操作。

---

## 总体统计

| 工具 | 调用 | 成功 | 失败 |
|---|---|---|---|
| open_file | 3 | 3 | 0 |
| add_alignment | 3 | 3 | 0 |
| find_restriction_sites | 4 | 4 | 0 |
| get_region_view | 1 | 1 | 0 |
| read_sequence | 5 | 5 | 0 |
| search_sequence | 4 | 4 | 0 |
| edit_sequence | 1 | 1 | 0 |
| add_feature | 1 | 1 | 0 |
| design_primers | 2 | 2 | 0 |
| add_primer | 4 | 4 | 0 |
| save_file | 2 | 2 | 0 |
| close_project | 1 | 1 | 0 |
| **合计** | **31** | **31** | **0** |

---

## MCP 改进建议（按收益排序）

1. **mutagenesis 支持按氨基酸坐标指定突变**（最高优先级）。本轮最易出错的人工步骤是"文献氨基酸编号 → 模板正链 seg 坐标 + 正链 mut_seq"的反推（负链 CDS 要反补、编号双口径）。建议 `design_primers` 增加如 `feature_id + aa_position + new_aa`（含编号口径参数，默认不含起始 Met）的高层输入，由引擎自行计算 seg/mut_seq 并返回现有的 `mutation` 自检块。这样Agent 完全无需手工定位密码子。
2. **`add_alignment` 增加 compact 响应模式**。当前恒返回全量 `orientedSequence`（可超 1000 bp）+ 全质粒 regionView，上下文开销大且 Agent 还需二次查询窗口。建议加 `compact: true` 参数只回 identity/差异明细/coverage，或省略 orientedSequence。
3. **统一/显性标注 Tm 口径**。`design_primers`（退火核心）与 `add_primer`/`check_primer_binding`（实际 3' 连续匹配）的 Tm 值不一致引起困惑。`check_primer_binding` 已有 `tmBasis` 说明，建议 `design_primers` 响应也加同名 `tmBasis` 字段，并把字段名改为 `tmAnnealCore` 之类自解释命名。
4. **type IIS 酶切口提示**。`find_restriction_sites` 对 BbsI 这类切口在识别序列下游的酶，可加一句 note（"切口位于识别序列之外，type IIS 特性"），避免 Agent 误读坐标。
5. **`edit_sequence` 的 expected_old 良性开销可再降**。当前长区间替换需先 `read_sequence` 取校验串（多一次往返）。已有 mismatch 时返回 `currentContent` 的机制很好；可考虑支持 `expected_old` 省略时返回 `warning` 而非强制，把取舍留给 Agent。
6. **（保持）做得好的点**：`get_region_view` 的 `ALIGNMENT DIFFS IN REGION` 一节让测序验证零逐字分析；`design_primers` 的 `orientation`/`cdsOverlaps`/`mutation` 自检块有效纠正了负链方向困惑；`edit_sequence` 的 `removedFeatures`/`clippedFeatures` 回显、`expected_old` 乐观校验、文件优先 I/O 策略均运转良好。本轮 31 次调用零失败、零重试。
