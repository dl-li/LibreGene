# MCP 引物设计测试记录

## 任务目标
1. 为 BlueScribe 载体克隆 mEGFP 设计带 BamHI/HindIII 酶切尾巴的 PCR 扩增引物。  
2. 推荐一对用于菌落 PCR 鉴定连接产物的引物并说明理由。  
3. 设计 mEGFP A206K（文献编号，不含起始 Met）的定点诱变引物。  
4. 将所有设计引物落库并保存为 `result.gbk`。

---

## 工作流与 MCP 调用记录

### 1. 打开项目
- 从 `BlueScribe-mEGFP.gbk` 加载模板/参考载体。确认其为 3442 bp 环状 DNA，mEGFP 位于 `complement(join(450..1163,1164..1166,1167..1169))`。

| 工具名 | 关键参数 | 成功/失败 | 结果摘要 |
|---|---|---|---|
| `open_file` | `path=BlueScribe-mEGFP.gbk` | 成功 | 3442 bp circular DNA，含 mEGFP、MCS、M13 引物等 |

### 2. 检查 BamHI/HindIII 位点（为 Q1 做准备）
- 需要确认 mEGFP 内部没有 BamHI/HindIII，且明确两个酶在 MCS 中的相对位置，以便确定引物尾巴方向。

| 工具名 | 关键参数 | 成功/失败 | 结果摘要 |
|---|---|---|---|
| `find_restriction_sites` | `enzymes=["BamHI","HindIII"]` | 成功 | BamHI 在 444..449（上游 MCS），HindIII 在 1170..1175（下游 MCS），均为单切点；mEGFP 插入区 450..1169 内部无这两个位点 |

- 由此确定：扩增 seg 取 mEGFP 完整插入区 `450..1169`；上游（BamHI 侧）给正向引物加 BamHI 尾巴，下游（HindIII 侧）给反向引物加 HindIII 尾巴，可保持与 BlueScribe-mEGFP 相同的插入方向（mEGFP 位于负链）。

### 3. Q1：设计 mEGFP 扩增引物

| 工具名 | 关键参数 | 成功/失败 | 结果摘要 |
|---|---|---|---|
| `design_primers` | `mode=amplify`, `seg={450,1169}`, `fwd_enzyme=BamHI`, `rev_enzyme=HindIII`, `target_tm=60` | 成功 | `internalSites=[]`，无内部 BamHI/HindIII；返回多组候选，采用 default 候选 |

- 选用候选：
  - **mEGFP-BamHI-F**：`GCGGGATCCTTACTTGTACAGCTCGTCCAT`（tail `GCGGGATCC` = 保护碱基 + BamHI）
  - **mEGFP-HindIII-R**：`GCGAAGCTTATGGTGAGCAAGGGCG`（tail `GCGAAGCTT` = 保护碱基 + HindIII）
- 注意：mEGFP 在负链，`design_primers` 的 orientation 说明显示产物正链 = 模板正链，引物命名按模板正链而非 CDS 编码链。

### 4. Q2：菌落 PCR 鉴定引物推荐
- 决定复用 Q1 的 mEGFP 特异性引物。
- 理由：它们只在含有 mEGFP 插入的菌落中才会扩增出约 720 bp 的条带；空载体/无插入菌落无产物，判断简单直接。
- 备选说明：项目自带的 M13 Fwd（379..395）+ M13 Rev（1221..1237）也可用于菌落 PCR，插入阳性菌落条带约 843 bp，空载体条带显著更小；但本次按“复用 Q1 引物”方案记录。

### 5. Q3：设计 A206K 诱变引物
- mEGFP CDS 为 720 bp = 240 个密码子（含终止子），编码链在负链。
- 用户所指“第 206 位 A”为文献编号（不含起始 Met），对应编码链第 207 个密码子。
- 因为 CDS 在负链，该密码子在模板正链上的位置需要反推：
  - 编码链密码子 1（Met）对应模板正链最末端密码子（1167..1169）。
  - 编码链密码子 207 对应模板正链位置 `549..551`。
- 先读取该区域确认当前模板正链为 `CGC`，其反向互补为 `GCG`（Ala），符合 A206。
- 目标改为 Lys（AAG），编码链 AAG 的反向互补为 `CTT`，因此 `mut_seq=CTT`。

| 工具名 | 关键参数 | 成功/失败 | 结果摘要 |
|---|---|---|---|
| `read_sequence` | `start=540`, `end=560` | 成功 | 得到 `TTTGCTCAGCGCGGACTGGGT`；549..551 为 `CGC`（Ala） |
| `design_primers` | `mode=mutagenesis`, `seg={549,551}`, `mut_seq=CTT`, `target_tm=60` | 成功 | 确认 `aaPositionExcludingMet=206`，`Ala(GCG)->Lys(AAG)`；返回诱变引物候选 |

- 选用候选：
  - **mEGFP-A206K-F**：`TCGTTGGGGTCTTTGCTCAGCTTGGACTGGGTGCTCAGG`
  - **mEGFP-A206K-R**：`ACTACCTGAGCACCCAGTCCAAGCTGAGCAAAGACCCCAA`

### 6. 引物落库

| 工具名 | 关键参数 | 成功/失败 | 结果摘要 |
|---|---|---|---|
| `add_primer` | `name=mEGFP-BamHI-F`, `type=fwd`, `seq=GCGGGATCCTTACTTGTACAGCTCGTCCAT` | 成功 | 1 个结合位点 443..470，Tm 67.8 °C |
| `add_primer` | `name=mEGFP-HindIII-R`, `type=rev`, `seq=GCGAAGCTTATGGTGAGCAAGGGCG` | 成功 | 1 个结合位点 1154..1175，Tm 65.5 °C |
| `add_primer` | `name=mEGFP-A206K-F`, `type=fwd`, `seq=TCGTTGGGGTCTTTGCTCAGCTTGGACTGGGTGCTCAGG` | 成功 | 1 个结合位点 552..567，Tm 59.8 °C |
| `add_primer` | `name=mEGFP-A206K-R`, `type=rev`, `seq=ACTACCTGAGCACCCAGTCCAAGCTGAGCAAAGACCCCAA` | 成功 | 1 个结合位点 532..549，Tm 60.2 °C |

### 7. 保存结果

| 工具名 | 关键参数 | 成功/失败 | 结果摘要 |
|---|---|---|---|
| `save_file` | `path=result.gbk` | 成功 | 写入 11581 字节，包含原序列及新增的 4 条引物 |

---

## 卡顿点 / 需要额外思考的地方

1. **mEGFP 在负链，seg 方向容易混淆**
   - 现象：`get_project_overview` 显示 mEGFP 为 `complement(join(...))`。
   - 解读：扩增引物设计时，`design_primers` 以模板正链为准给出 `orientation` 说明，最终产物正链与模板正链一致，因此只要按 BamHI 上游、HindIII 下游的方向加尾巴，就能复现原载体中 mEGFP 的负链定位。

2. **A206 在模板正链上的坐标需要手动反推**
   - 现象：工具不会自动把“第 206 位（不含 Met）”转换成模板正链坐标。
   - 解读：由于 CDS 在负链，编码密码子顺序与模板正链相反。手动计算得编码链第 207 个密码子对应模板正链 `549..551`，并用 `read_sequence` 验证当前为 `CGC` → 编码链 `GCG`（Ala）。若直接用 206×3 从 450 起算会得到错误位置。

3. **`design_primers` 返回的 Tm 是退火核心 Tm，不是全长引物 Tm**
   - 现象：含酶切尾巴的扩增引物和诱变引物的 `tm` 值低于 `add_primer` 重算后的全长结合 Tm。
   - 解读：`design_primers` 的 Tm 仅用于候选筛选；最终报告使用 `add_primer` 返回的全长结合 Tm（BamHI-F 67.8 °C 等）。

4. **诱变引物 `mut_seq` 必须按正链给出**
   - 现象：`design_primers(mutagenesis)` 要求 `mut_seq` 是“突变后 seg 的正链内容”。
   - 解读：目标 Lys 密码子 AAG 在编码链上，而 seg 在模板正链，因此需要传入其反向互补 `CTT`，工具随后正确报告 `codonAfter=AAG`。

---

## 最终引物清单

| 名称 | 序列（5'→3'） | 全长结合 Tm | 用途 |
|---|---|---|---|
| mEGFP-BamHI-F | `GCGGGATCCTTACTTGTACAGCTCGTCCAT` | 67.8 °C | mEGFP 扩增正向引物，5' 加 BamHI 尾巴（GCG 保护碱基 + GGATCC） |
| mEGFP-HindIII-R | `GCGAAGCTTATGGTGAGCAAGGGCG` | 65.5 °C | mEGFP 扩增反向引物，5' 加 HindIII 尾巴（GCG 保护碱基 + AAGCTT） |
| mEGFP-A206K-F | `TCGTTGGGGTCTTTGCTCAGCTTGGACTGGGTGCTCAGG` | 59.8 °C | A206K 定点诱变正向引物 |
| mEGFP-A206K-R | `ACTACCTGAGCACCCAGTCCAAGCTGAGCAAAGACCCCAA` | 60.2 °C | A206K 定点诱变反向引物 |

- 菌落 PCR 推荐：复用 **mEGFP-BamHI-F + mEGFP-HindIII-R**，预期阳性条带约 720 bp；空载体/无插入菌落无产物。
- A206K 突变验证：`design_primers` 报告 `aaPositionExcludingMet=206`，`Ala(GCG) → Lys(AAG)`，符合用户需求。

---

## MCP 调用统计

| 工具 | 成功 | 失败 | 合计 |
|---|---|---|---|
| `open_file` | 1 | 0 | 1 |
| `find_restriction_sites` | 1 | 0 | 1 |
| `design_primers` | 2 | 0 | 2 |
| `read_sequence` | 1 | 0 | 1 |
| `add_primer` | 4 | 0 | 4 |
| `save_file` | 1 | 0 | 1 |
| **合计** | **10** | **0** | **10** |

---

## 产物文件

`/Users/lidonglin/LibreGene/examples/tasks/mcp-test-round2/primer-design/result.gbk`
