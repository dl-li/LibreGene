# MCP 功能测试报告 — Primer Design（mEGFP 克隆 + 菌落鉴定 + A206K 诱变）

- 测试时间：2026-08-18
- 被测应用：LibreGene（Tauri v2 桌面质粒编辑器，内嵌 MCP server，Streamable HTTP @ 127.0.0.1:8766）
- 测试工具集：`mcp__libregene__*`（list_projects / open_file / get_project_overview / read_sequence / find_restriction_sites / list_primers / design_primers / check_primer_binding / close_project）
- 输入文件：`/Users/lidonglin/LibreGene/examples/tasks/Primer Design/BlueScribe-mEGFP.gbk`（只读打开，未修改）
- 输出目录：`/Users/lidonglin/LibreGene/examples/tasks/test-results/primer-design/`（primers.md / report.md）

---

## 一、任务理解

用户需求（原始 Prompt）：

> 用 BamHI + HindIII 消化过的空载 BlueScribe 载体与含 mEGFP 的未知载体作为模板：
> 1. 设计 PCR 引物扩增 mEGFP，使其经相同酶消化后能连入 BlueScribe，得到目录中的 BlueScribe-mEGFP 载体；
> 2. 给出鉴定连接后菌落的引物对；
> 3. 设计 PCR 诱变引物，将 mEGFP 第 206 位 A 突变为 K。

任务拆解与关键前提（依据任务说明）：目录中只有 `BlueScribe-mEGFP.gbk` 一个序列来源（它是"答案"，同时是唯一可用的序列与结构参考）。设计必须与之自洽。

**关键结构事实（通过 MCP 工具实测得出）：**
- BlueScribe-mEGFP = 3442 bp 环状 DNA，甲基化 Dam/Dcm/EcoKI。
- mEGFP CDS = `complement(join(449..1162,1163..1165,1166..1168))`，0-based inclusive，**720 bp / 240 aa，编码链为负链**（5' ATG 在正链坐标 1168，3' TAA 在正链坐标 449）。翻译验证与标准 EGFP 一致（`MVSKGEELFT...VTAAGITLGMDELYK*`）。
- 最终载体中酶切位点（find_restriction_sites 实测）：BamHI `443..448`（切 443/444 之间）、HindIII `1169..1174`（切 1169/1170 之间）、EcoRI `422..427`。BamHI 紧邻 mEGFP 编码链 3' 端（449），HindIII 紧邻编码链 5' 端（1168）。
- 已有引物：M13 Fwd `GTAAAACGACGGCCAGT`（378..394, +）、M13 Rev `CAGGAAACAGCTATGAC`（1220..1236, −）。

**设计策略推导：**
- 克隆引物：因 mEGFP 为负链 CDS，扩增产物插入后编码链保持负链方向才能复现参考载体结构。design_primers amplify 以正链坐标定义 seg（产物正链 = 模板正链 seg 区间），故 `seg = {449, 1168}`，fwd 引物结合正链 449（编码链 3' 端、BamHI 侧）、rev 引物结合负链 1168 端（编码链 5' 端 ATG、HindIII 侧）。这与参考载体"编码链 5' 邻 HindIII、3' 邻 BamHI"完全吻合。
- 菌落 PCR：跨连接处组合 = 骨架通用引物（M13 Fwd 区 378）+ 插入片段特异引物（mEGFP 5' 端），只有正确插入才出 ~791 bp 条带。
- 诱变：A206K。目标残基位于 `...STQS**A**LSK...`（密码子 GCG，正链 548..550 = CGC）。M=1 编号为第 207 位；不含起始 Met（或以第 2 位 V 为 +1）时为第 206 位 —— 即任务口径"第 206 位 A"。

---

## 二、完整执行流程

1. `list_projects` — 发现残留项目 `examples/BlueScribe-mEGFP.gbk`（dirty:true，先前会话遗留），active 状态。
2. `open_file` 打开任务目录的 `BlueScribe-mEGFP.gbk` → 3442 bp 环状，获得完整 features/primers/enzymes 概览。
3. `find_restriction_sites(BamHI/HindIII/EcoRI)` — 确认最终载体中三个位点位置（见上）。
4. `read_sequence(440..480)` + `read_sequence(1130..1190)` — 确认 mEGFP 两端与 MCS 序列（终止密码子区 `CGGGGATCC...`、起始区 `...CACCATAAGCTTTTG...`）。
5. `list_primers` — 获取 M13 Fwd/Rev 序列与结合位点。
6. `read_sequence(540..570)` — 定位 A206 密码子区（正链 548..550 = CGC）。
7. `design_primers(mode=amplify, seg={449,1168}, fwd_enzyme=BamHI, rev_enzyme=HindIII, protect_bases=3, target_tm=62)` — 克隆引物，`internalSites=[]`（无内部酶切位点）。
8. `read_sequence(449..1168)` — 读完整 CDS 正链；本地脚本反互补翻译验证（240 aa，与标准 EGFP 一致，A206 上下文 `...STQSALSK...`）。
9. `design_primers(mode=amplify, seg={378,1168}, target_tm=60)` — 菌落 PCR 引物（跨 M13 骨架区→mEGFP 5' 端）。
10. `design_primers(mode=mutagenesis, seg={548,550}, mut_seq=TTT, target_tm=62)` — 诱变引物；mutation 自检块确认 GCG→AAA（Ala→Lys）、`aaPositionExcludingMet=206`。
11. `check_primer_binding` — 6 条引物一次批量验证，全部 binds=true、单一结合位点。
12. 写 `primers.md`（最终答案）与 `report.md`（本报告）。
13. `close_project` 清理打开的项目。

---

## 三、MCP 工具调用记录表

| # | 工具 | 关键参数 | 结果 | 关键返回值摘要 |
|---|---|---|---|---|
| 1 | list_projects | — | ✅ | activeId=`examples/BlueScribe-mEGFP.gbk`（dirty:true，遗留）；唯一项目 |
| 2 | open_file | path=`tasks/Primer Design/BlueScribe-mEGFP.gbk` | ✅ | ok:true，3442 bp circular；mEGFP CDS complement(join(449..1162,1163..1165,1166..1168))；M13 Fwd 378..394 / M13 Rev 1220..1236；82 单切酶 |
| 3 | find_restriction_sites | enzymes=[BamHI,HindIII,EcoRI] | ✅ | BamHI rec 443..448 (top-cut 444)；HindIII rec 1169..1174 (cut 1170)；EcoRI rec 422..427 (cut 423)；均 unique、methylationBlocked:false |
| 4 | read_sequence | start=440,end=480 | ✅ | 正链 `CGGGGATCCTTACTTGTACAGCTCGTCCATGCCGAGAGTGA`（BamHI 位点 443..448、CDS 3' 端） |
| 5 | read_sequence | start=1130,end=1190 | ✅ | 正链 `CACCACCCCGGTGAACAGCT...CACCATAAGCTTTTGTTCCCTTTAGTGA`（HindIII 位点 1169..1174、CDS 5' 端） |
| 6 | list_primers | — | ✅ | M13 Fwd `GTAAAACGACGGCCAGT`（378..395, +）；M13 Rev `CAGGAAACAGCTATGAC`（1220..1237, −）；均 1 位点 |
| 7 | read_sequence | start=540,end=570 | ✅ | 正链 548..550 = CGC（= 编码链 GCG，Ala） |
| 8 | design_primers | mode=amplify, seg={449,1168}, fwd_enzyme=BamHI, rev_enzyme=HindIII, protect_bases=3, target_tm=62 | ✅ | Fwd default `GCGGGATCCTTACTTGTACAGCTCGTCCATG`（Tm 60.8）；Rev default `GCGAAGCTTATGGTGAGCAAGGGCGA`（Tm 61.8）；internalSites=[] |
| 9 | read_sequence | start=449,end=1168 | ✅ | 完整 CDS 正链 720 bp；本地翻译确认 240 aa 标准 EGFP 序列，`aa206=S / aa207=A(GCG)`（M=1） |
| 10 | design_primers | mode=amplify, seg={378,1168}, target_tm=60 | ✅ | Fwd default `GTAAAACGACGGCCAGTG`（Tm 59.9, M13 区）；Rev default `ATGGTGAGCAAGGGCG`（Tm 60.4, mEGFP 5' 端）；internalSites=[] |
| 11 | design_primers | mode=mutagenesis, seg={548,550}, mut_seq=TTT, name1/name2 | ✅ | Mutation-Fwd default `TCGTTGGGGTCTTTGCTCAGTTTGGACTGGGTGCTCAGGT`（Tm 61.6）；Mutation-Rev default `ACTACCTGAGCACCCAGTCCAAACTGAGCAAAGACCCCAACG`（Tm 62.1）；自检：codonBefore GCG(Ala)→codonAfter AAA(Lys)，aaPosition1Based=207 / aaPositionExcludingMet=206，strand=−，featureId=mEGFP_449；warning"all 3 bases replaced"（预期） |
| 12 | check_primer_binding | primers=6 条（cloning×2, colony×2, mutagenesis×2） | ✅ | 全部 binds=true、bindingSiteCount=1（无脱靶）。cloning-Fwd: +442..470 annealLen29/misTail2 Tm68.6；cloning-Rev: −1152..1174 annealLen23/misTail3 Tm66.3；colony-Fwd: +378..395 annealLen18；colony-Rev: −1153..1168 annealLen16；Mut-Fwd: +551..567 annealLen17/misTail23 Tm61.6；Mut-Rev: −529..547 annealLen19/misTail23 Tm62.1 |
| 13 | Write | primers.md | ✅ | 4365 bytes |
| 14 | Write | report.md | ✅ | 本文件 |
| 15 | close_project | tasks 目录项目 | ✅ | 清理 |
| 16 | close_project | examples 遗留项目 | ✅ | 清理 |

> 注：read_sequence 之后另用本地 node 脚本对 720 bp 正链序列做反互补 + 翻译（纯计算验证，非 MCP 操作）。

---

## 四、统计

| 类别 | 数量 |
|---|---|
| MCP 工具调用总次数 | 12（+2 次 close_project） |
| — list_projects | 1 |
| — open_file | 1 |
| — find_restriction_sites | 1 |
| — read_sequence | 4 |
| — list_primers | 1 |
| — design_primers | 3（amplify×2 + mutagenesis×1） |
| — check_primer_binding | 1（一次批量验证 6 条） |
| — close_project | 2 |
| 失败/报错调用次数 | **0** |
| 需重试的调用 | 0 |
| 输出文件 | 2（primers.md, report.md） |

---

## 五、错误原因与分析

全程无调用失败、无错误重试。仅有两条**警告**（均属预期行为，非错误）：

1. **mutagenesis 的 warning："all 3 bases of seg 548..550 are replaced; confirm mut_seq is the PLUS-strand sequence at the right location (mind the CDS strand)"**
   - 原因：A206K 是整密码子替换（正链 CGC→TTT），3 个碱基全部改变，工具据此怀疑"可能选错链/位置"而给出警告。
   - 分析：该警告对**整密码子替换**（A→K 恰好三碱基全换）是误报性质——通过 mutation 自检块（codonBefore GCG→codonAfter AAA、minusContext `CACCCAGTCC[GCG]CTGAGCAAAG`、plusContext `CTTTGCTCAG[CGC]GGACTGGGTG`）可确认位置与方向正确。但首次接触的用户易被误导。
2. **check_primer_binding 的克隆引物 annealLen/Tm 高于设计值**
   - 原因：克隆 Fwd 尾巴含 `GGATCC`、Rev 尾巴含 `AAGCTT`，而模板 443..448 / 1169..1174 恰好就是 BamHI/HindIII 位点（引物尾巴的前 4/3 个碱基与模板真实位点前段相同），check 按"3' 端连续匹配"把尾巴部分计入（annealLen 22→29、17→23；Tm 60.8→68.6、61.8→66.3）。
   - 分析：这是工具文档已声明的口径差异（design 报设计退火区，check 报实际 3' 连续匹配），并非设计错误；PCR 实际退火以引物 3' 端 22/17 nt 退火区为准。但对酶切尾巴恰好匹配模板同一位点的场景，check 的 Tm 数值会显著偏离设计值，可能造成困惑。

---

## 六、对 MCP 工具/描述的改进建议

1. **design_primers（amplify）应说明负链 CDS 的方向语义**：当目标 CDS 为负链（complement）时，seg 传正链坐标会导致"fwd 引物在编码链 3' 端、rev 引物在 5' 端"的反直觉命名，用户容易误判为设计错误。建议在响应中附带 `cds.strand` 提示或注明"产物正链 = 模板正链 seg 区间"。
2. **mutagenesis 的"all bases replaced"警告分级**：整密码子替换（如 A→K 三碱基全换）与"疑似选错链/位置"应区分对待；可仅当替换改变密码子翻译或位于 CDS 之外时才给 warning，或在 warning 文案中提示"整密码子替换属预期时无需担心"。
3. **check_primer_binding 报告酶切尾巴的"意外扩展匹配"**：当尾巴（如 GGATCC/AAGCTT）与模板上真实同一位点部分匹配导致 annealLen/Tm 跳升时，可在结果中加标记（如 `tailExtendsAnneal: true`），避免与脱靶/异常结合混淆；同时建议文档给出"设计 Tm 与 check Tm 差异的常见场景清单"。
4. **read_sequence 的 text 标尺**：小窗口（<100 bp）也按 60 bp/行排版，标尺占据近半输出；可对小窗口改用紧凑标尺或可关闭 ruler。
5. **A206K 编号口径值得正面文档化**：mutation 自检块的 `aaPosition1Based` / `aaPositionExcludingMet` 双编号非常有用，建议在工具描述中显式说明"氨基酸位置含/不含起始 Met 两种口径"，帮助用户对齐自己任务中的编号习惯。

（另：本任务涉及的坐标约定、mutation 自检、internalSites、check 口径等在 AGENTS.md 中已有文档，实际行为与文档一致，无偏差。）

---

## 七、最终答案（三部分引物）

### 1. 克隆引物（扩增 mEGFP 完整 CDS，BamHI/HindIII 双酶切克隆）

| 引物 | 序列 (5'→3') | 说明 |
|---|---|---|
| **mEGFP-cloning-Fwd** | `GCGGGATCCTTACTTGTACAGCTCGTCCATG` | GCG 保护 + **GGATCC (BamHI)** + 22 nt 退火（覆盖 3' 终止密码子区，模板 449..470）；Tm 60.8 °C |
| **mEGFP-cloning-Rev** | `GCGAAGCTTATGGTGAGCAAGGGCGA` | GCG 保护 + **AAGCTT (HindIII)** + 17 nt 退火（含 5' ATG 起始区，模板 1152..1168）；Tm 61.8 °C |

理由：mEGFP 为负链 CDS（720 bp/240 aa），该对引物扩增产物 720 bp，产物内无 BamHI/HindIII 位点（internalSites=[]）；双酶切后连入 BamHI/HindIII 消化的 BlueScribe，插入后 mEGFP 编码链为负链、5' 端邻 HindIII（1169..1174）、3' 端邻 BamHI（443..448），与参考载体 BlueScribe-mEGFP 结构完全一致。两引物 check 验证单一结合位点。

### 2. 菌落 PCR 鉴定引物（跨连接处：骨架通用 + 插入片段特异）

| 引物 | 序列 (5'→3') | 说明 |
|---|---|---|
| **colony-pcr-Fwd** | `GTAAAACGACGGCCAGTG` | M13 Fwd 区（骨架通用，模板 378..395）；Tm 59.9 °C |
| **colony-pcr-Rev** | `ATGGTGAGCAAGGGCG` | mEGFP 5' 端特异（模板 1153..1168）；Tm 60.4 °C |

理由：Fwd 位于载体骨架（空载体也存在），Rev 的靶序列仅在 mEGFP 插入后才存在，且两者之间跨过 BamHI 连接处。预期产物 ~791 bp：正确插入 → 有该条带；空载体自连 → 无条带；反向插入 → 无条带（Rev 方向不匹配）。二者无酶切尾巴，可直接用于菌落 PCR。

### 3. 诱变引物（mEGFP A206K：第 206 位 A→K）

| 引物 | 序列 (5'→3') | 说明 |
|---|---|---|
| **Mutation-Fwd** | `TCGTTGGGGTCTTTGCTCAGTTTGGACTGGGTGCTCAGGT` | 40 nt，Tm 61.6 °C；含 TTT 突变（编码链 GCG→AAA），3' 端 17 nt 退火（551..567） |
| **Mutation-Rev** | `ACTACCTGAGCACCCAGTCCAAACTGAGCAAAGACCCCAACG` | 39 nt，Tm 62.1 °C；含 AAA（与正链 TTT 互补），3' 端 19 nt 退火（529..547） |

理由：目标残基位于 `...STQS**A**LSK...`（经典 A206K 单体化位点）；design_primers 自检块确认密码子 GCG(Ala)→AAA(Lys)、`aaPositionExcludingMet=206`（即任务所述第 206 位；M=1 数法为第 207 位）、CDS 负链、featureId=mEGFP_449。两引物为 QuikChange 式（5' 端携带突变、3' 端完整配对），check 验证单一结合位点、3' 退火核心 annealLen 17/19、mismatchedTail 23（含 3 个预期突变碱基）。
