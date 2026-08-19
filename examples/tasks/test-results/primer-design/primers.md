# Primer Design — 最终引物序列

来源模板：`/Users/lidonglin/LibreGene/examples/tasks/Primer Design/BlueScribe-mEGFP.gbk`（3442 bp 环状 DNA）
mEGFP CDS：`complement(join(449..1162,1163..1165,1166..1168))`（0-based inclusive），720 bp / 240 aa，编码链为负链方向
（编码链 5' ATG 位于正链坐标 1168，3' TAA 位于正链坐标 449）。

载体上的酶切位点（最终产物 BlueScribe-mEGFP 中实测）：
- BamHI：443..448（GGATCC），紧邻 mEGFP 编码链 3' 端
- HindIII：1169..1174（AAGCTT），紧邻 mEGFP 编码链 5' 端

---

## 1. 克隆引物（BamHI + HindIII 双酶切，扩增 mEGFP 完整 CDS 连入 BlueScribe）

扩增产物 = 720 bp mEGFP CDS + 两侧酶切尾巴；产物内无 BamHI/HindIII 识别位点（design_primers 返回 `internalSites: []`）。

| 引物 | 序列 (5'→3') | 长度 | 结构 | 设计 Tm | 结合位点 (0-based) |
|---|---|---|---|---|---|
| mEGFP-cloning-Fwd | `GCGGGATCCTTACTTGTACAGCTCGTCCATG` | 31 nt | GCG 保护 + GGATCC(BamHI) + 22 nt 退火区 | 60.8 °C | + 链 449..470（覆盖终止密码子 TAA 区） |
| mEGFP-cloning-Rev | `GCGAAGCTTATGGTGAGCAAGGGCGA` | 26 nt | GCG 保护 + AAGCTT(HindIII) + 17 nt 退火区 | 61.8 °C | − 链 1152..1168（含起始密码子 ATG） |

- Fwd 退火区对应编码链 3' 端（终止密码子区）；Rev 退火区对应编码链 5' 端（ATG 起始区）。
- 因 mEGFP 为负链 CDS，插入后 mEGFP 编码链在质粒上保持负链方向，与参考载体 BlueScribe-mEGFP 的结构（BamHI 邻接 3' 端、HindIII 邻接 5' 端）完全一致。
- `check_primer_binding` 验证：两条均单一结合位点、binds=true。Fwd 报告的 `annealLen=29`（442..470）/ Rev `annealLen=23`（1152..1174）比设计值长，是因为酶切尾巴 `GGATCC`/`AAGCTT` 恰好与模板上真实的 BamHI（443..448）/HindIII（1169..1174）位点部分匹配，check 按 3' 连续匹配口径把尾巴计入（Tm 报告 68.6/66.3 °C）。PCR 实际退火以引物 3' 端设计退火区为准（Tm ≈ 60–62 °C），不影响克隆。

## 2. 菌落 PCR 鉴定引物（跨连接处：骨架通用引物 + 插入片段特异引物）

| 引物 | 序列 (5'→3') | 长度 | 结合位点 (0-based) |
|---|---|---|---|
| colony-pcr-Fwd | `GTAAAACGACGGCCAGTG` | 18 nt | + 链 378..395（M13 Fwd 位点，骨架区） |
| colony-pcr-Rev | `ATGGTGAGCAAGGGCG` | 16 nt | − 链 1153..1168（mEGFP 5' 端，插入片段特异） |

- 预期产物约 **791 bp**（跨越 BamHI 连接处 443..448 与 mEGFP 起始区）。
- 原理：Fwd 是载体骨架通用引物（M13 Fwd 区域，空载体也结合）；Rev 的靶序列（mEGFP 起始区）仅在 mEGFP 插入后才存在。
  - 插入正确（mEGFP 负链方向）→ 出现 ~791 bp 条带；
  - 未插入（空载体自连）→ Rev 无结合位点，无条带；
  - 反向插入 → Rev 方向不匹配，无条带（或条带大小不同）。
- 两条均无酶切尾巴（`tailLen: 0`），`check_primer_binding` 验证：单一结合位点、binds=true、mismatchedTail=0。

## 3. 诱变引物（mEGFP A206K：第 206 位 A→K）

突变位点（design_primers mutation 自检块确认）：
- 编码链密码子 615..617（0-based）：**GCG（Ala）→ AAA（Lys）**；正链坐标 548..550：**CGC → TTT**（3 碱基全替换）
- 工具自检：`aaPosition1Based=207`、`aaPositionExcludingMet=206`（即任务所说"第 206 位"，不含起始 Met 编号；M=1 数法为 207），上下文 `...STQS[ALSK...`（经典 A206K 单体化突变位点）

| 引物 | 序列 (5'→3') | 长度 | 设计 Tm | 结合位点 (0-based) |
|---|---|---|---|---|
| Mutation-Fwd | `TCGTTGGGGTCTTTGCTCAGTTTGGACTGGGTGCTCAGGT` | 40 nt | 61.6 °C | + 链 551..567（3' 端退火区；5' 端含 TTT 突变，错配 3 bp） |
| Mutation-Rev | `ACTACCTGAGCACCCAGTCCAAACTGAGCAAAGACCCCAACG` | 39 nt | 62.1 °C | − 链 529..547（3' 端退火区；5' 端含 AAA 互补突变） |

- `check_primer_binding` 验证：两条均单一结合位点、binds=true；Fwd `annealLen=17`/`mismatchedTail=23`，Rev `annealLen=19`/`mismatchedTail=23` —— 3' 端退火核心完整配对，5' 端携带突变碱基（预期行为）。
- 诱变方案：两引物反向互补配对覆盖含突变的双链区，为 QuikChange 式全质粒诱变设计；PCR 后 DpnI 消化模板、转化即可。
