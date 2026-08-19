# LibreGene MCP 功能测试报告：pNP 转基因 RNAi 质粒 in silico 构建

- **测试任务**：根据给定 AS 序列设计 shRNA 发夹，按 pNP Transgenic RNAi System 实验手册（Wang et al., 2019, Bio-protocol 9(3): e3158）完成 Primer-F/Primer-R 推导，并在 pNP(Vm2) 载体上进行 EcoRI + NheI 双酶切克隆的 in silico 模拟，产出 pNP-GOI 质粒。
- **给定靶向序列（AS，反义链，21 nt）**：`TTAGAGGCTACCTTCTTGCTT`
- **测试时间**：2026-08-18
- **产物文件**：`/Users/lidonglin/LibreGene/examples/tasks/test-results/rnai/pNP-GOI.gbk`（7895 bp circular，26044 字节）

---

## 1. 任务理解

### 1.1 实验原理（来自 protocol.txt Procedure A.1 单发夹构建）

pNP 载体为转基因 RNAi 载体（含 UAS/DSCP 启动子、vermilion 标记、attB、MCS1/MCS2）。单发夹克隆流程：

1. **发夹设计**：给定 21 nt 反义序列 "As"；其反向互补 "As re-com" 即发夹的有义链（sense）。
2. **引物设计**（Protocol 原式）：
   - `Primer-F: ctagcagt “As re-com” tagttatattcaagcata “As” gcg`
   - `Primer-R: aattcgc “As re-com” tatgcttgaatataacta “As” actg`
   - 5' 端 `ctag` / `aatt` 为 4 nt 突出，分别与 NheI / EcoRI 的粘性末端互补；`cagt`/`actg`、`gcg`/`cgc` 为配对连接区；`tagttatattcaagcata` 为发夹 loop。
3. **退火**：Primer-F + Primer-R 95°C 变性后缓慢降温，形成双链发夹插入片段（两端各带 4 nt 5' 突出）。
4. **酶切**：pNP 载体 EcoRI-HF + NheI-HF 双酶切，胶回收 ~8 kb 大片段（切除 MCS1 中间小片段）。
5. **连接**：T4 连接酶将退火产物连入线性化载体。
6. **鉴定**：U-F + Ftz 引物 PCR 验证正确克隆（protocol 预期 ~750 bp 条带，具体长度随载体而异）。

### 1.2 序列推导（本地计算，构建/验证均走 MCP）

```
As (antisense, 21 nt)          = TTAGAGGCTACCTTCTTGCTT
As re-com (sense, 21 nt)       = AAGCAAGAAGGTAGCCTCTAA   （即 rc(As)，逐位校验通过）
loop (18 nt)                   = TAGTTATATTCAAGCATA
loop rc                        = TATGCTTGAATATAACTA

Primer-F (71 nt) = ctagcagt + As re-com + loop + As + gcg
  CTAGCAGTAAGCAAGAAGGTAGCCTCTAATAGTTATATTCAAGCATATTAGAGGCTACCTTCTTGCTTGCG
Primer-R (71 nt) = aattcgc + As re-com + loop rc + As + actg
  AATTCGCAAGCAAGAAGGTAGCCTCTAATATGCTTGAATATAACTATTAGAGGCTACCTTCTTGCTTACTG

配对验证：
  Primer-F 5' 粘端 CTAG ↔ NheI 切口突出；Primer-R 5' 粘端 AATT ↔ EcoRI 切口突出
  Primer-F CAGT…GCG 与 Primer-R …ACTG/CGC 精确互补（rc 逐段比对通过）
  发夹折叠：sense(21) - loop(18) - antisense(21)，两臂互补，全长 60 nt
```

### 1.3 克隆位点逻辑（"插入后 EcoRI/NheI 位点去留"）

载体 MCS1 中 **NheI GCTAGC@3934..3939**（切口 3934|3935）与 **EcoRI GAATTC@4551..4556**（切口 4551|4552）。双酶切切除两切口之间 617 bp 小片段（3935..4551，含 attL1 序列）。连接后：

- **左端**：载体保留 NheI 位点第 1 碱基 `G` + 插入片段顶链 `CTAGCAGT…` → `GCTAGCAGT…`，**NheI 位点 GCTAGC 重构保留**；
- **右端**：插入片段末端 `…GCG` + 载体保留的 EcoRI 位点 `AATTC` → `…GCGAATTC`，**EcoRI 位点 GAATTC 重构保留**。

即连接产物两端重建出完整的 EcoRI/NheI 位点，克隆后质粒仍可被 EcoRI、NheI 单切（与 protocol 多 shRNA 构建中"发夹片段可被释放/移动"的设计一致）。XbaI@3897..3902 位于 NheI 上游、SpeI@4850..4855 位于 MCS2，均不受影响。

---

## 2. 执行流程（完整记录）

1. 读取 `protocol.txt`（524 行），提取 Primer-F/R 设计规则与克隆流程。
2. 本地推导（Python 脚本）As re-com、Primer-F/R、插入序列，写入 `derivation.txt` 备查。
3. MCP `open_file` 打开 `pNP(Vm2).dna` → 8441 bp circular。
4. MCP `find_restriction_sites`（EcoRI/NheI）→ 定位切口。
5. MCP `read_sequence`（3920..4570）+ `find_restriction_sites`（SpeI/XbaI/HindIII）→ 确认 MCS 上下文与被移除片段边界。
6. MCP `edit_sequence` 替换 3935..4551（617 bp）为插入序列（71 bp，= Primer-F）→ **第一次因 expected_old 不匹配被拒**，用引擎返回的 `currentContent` 作为校验串重试 → 成功，新长度 7895 bp。
7. MCP `read_sequence`（3910..4060）+ `find_restriction_sites`（EcoRI/NheI/SpeI/XbaI）→ 插入区序列与酶切位点重构验证。
8. MCP `add_feature` 标注 `shRNA-GOI hairpin`（3943..4002，0-based）。
9. MCP `check_primer_binding`：U-F/Ftz（protocol 验证引物）+ Primer-F/R（设计引物）→ 全部完全结合。
10. MCP `save_file` → `pNP-GOI.gbk`（26044 字节）。
11. MCP `open_file` 重新打开落盘 gbk → 独立确认产物完整。
12. MCP `close_project` 清理全部项目。

---

## 3. MCP 工具调用记录表

| # | 工具 | 关键参数 | 结果 | 关键返回值摘要 |
|---|------|----------|------|----------------|
| 1 | `open_file` | path=`pNP(Vm2).dna` | ✅ 成功 | 8441 bp circular；projectId 返回；overview 显示 MCS1@3934..4550、U-F@3789..3808、Ftz@4929..4949 |
| 2 | `find_restriction_sites` | enzymes=[EcoRI, NheI] | ✅ 成功 | EcoRI GAATTC@4551..4556（top 切口 4551\|4552，unique）；NheI GCTAGC@3934..3939（top 切口 3934\|3935，unique） |
| 3 | `read_sequence` | start=3920, end=4570 | ✅ 成功 | 651 bp MCS 区域；确认 NheI 前 32 bp 为 XbaI、被切片段含 attL1 区 |
| 4 | `find_restriction_sites` | enzymes=[SpeI, XbaI, HindIII] | ✅ 成功 | XbaI@3897..3902、SpeI@4850..4855（均在被移除区之外，保留） |
| 5 | `edit_sequence` | start=3935, end=4551, replacement=71nt, expected_old（手工拼接） | ❌ 失败 | `expected_old mismatch at index 1`；返回 currentContent/expectedContext 与 ±20 bp 上下文（引擎乐观校验正确拦截） |
| 6 | `edit_sequence`（重试） | 同上，expected_old=引擎返回的 currentContent | ✅ 成功 | `Replaced [3935..4551] (617 bp) with 71 bp; new length 7895 (was 8441)`；clippedFeatures=[MCS1 3934..4550→3934..4005]；removedFeatures=[attL1 3998..4097] |
| 7 | `read_sequence` | start=3910, end=4060 | ✅ 成功 | 插入区序列逐段比对全部吻合（见 §5） |
| 8 | `find_restriction_sites` | enzymes=[EcoRI, NheI, SpeI, XbaI] | ✅ 成功 | EcoRI@4005..4010、NheI@3934..3939（**均重构保留**，unique）；SpeI/XbaI 未受影响 |
| 9 | `add_feature` | name=shRNA-GOI hairpin, location=3944..4003 (1-based) | ✅ 成功 | featureId=`feature_1787061583984_0`；落位 3943..4002（0-based） |
| 10 | `check_primer_binding` | primers=[U-F, Ftz, Primer-F, Primer-R] | ✅ 成功 | 4 条全部 binds=true：U-F@3789..3808 (+，Tm 60.9)、Ftz@4383..4403 (−，Tm 60.7)、**Primer-F annealLen=71 完全匹配**、**Primer-R annealLen=71 完全匹配** |
| 11 | `save_file` | path=`test-results/rnai/pNP-GOI.gbk` | ✅ 成功 | 26044 bytes；7895 bp circular；所有特征坐标正确平移（Ftz 4929..4949→4383..4403） |
| 12 | `open_file` | path=`pNP-GOI.gbk`（落盘文件） | ✅ 成功 | 7895 bp circular；shRNA-GOI hairpin 特征保留 —— 落盘独立验证通过 |
| 13 | `close_project` | projectId=`pNP(Vm2).dna` | ✅ 成功 | Closed project |
| 14 | `close_project` | projectId=`pNP-GOI.gbk` | ✅ 成功 | Closed project |

---

## 4. 统计

**总调用次数**：14
**成功**：13 ｜ **失败**：1（`edit_sequence` 第 1 次尝试，expected_old 乐观校验拒绝）
**错误率**：7.1%（仅 1 次、预期内的防护性拒绝，重试后成功）

按工具统计：

| 工具 | 调用次数 | 成功 | 失败 |
|------|---------|------|------|
| open_file | 2 | 2 | 0 |
| find_restriction_sites | 3 | 3 | 0 |
| read_sequence | 2 | 2 | 0 |
| edit_sequence | 2 | 1 | 1 |
| add_feature | 1 | 1 | 0 |
| check_primer_binding | 1 | 1 | 0 |
| save_file | 1 | 1 | 0 |
| close_project | 2 | 2 | 0 |

---

## 5. 错误原因与分析

**唯一错误**：`edit_sequence` 的 `expected_old` mismatch。

- **现象**：`expected_old mismatch at index 1`，`expectedContext='CATGGATGTTTTCCCAGTCAC'` vs `currentContext='CTAGCATGGATGTTTTCCCAG'`，错误信息附 ±20 bp 上下文。
- **根因**：属**测试方操作失误**，非工具缺陷。我手工构造 617 bp 校验串时把开头 `CTAGC…` 笔误抄为 `CATGG…`（第 1 位即错）。引擎的乐观校验（`expected_old` 与实际内容大小写不敏感比对）正确识别并拒绝，防止了在错误前提上的编辑。
- **处理**：直接用引擎错误响应中返回的 `currentContent`（权威值）作为 `expected_old` 重试，一次成功。
- **经验**：长校验串应从 `read_sequence` 的 `sequence` 字段或引擎回显中程序化截取，避免手工转录；本案例中 MCP 错误体已自带可复制的 `currentContent` 字段，使恢复路径非常顺畅。

---

## 6. 对 MCP 工具 / 描述 / 文档的改进建议

1. **`edit_sequence` 的 expected_old mismatch 已非常好**（返回差异索引 + 两侧 ±20 bp 上下文 + 完整 currentContent 可复制），建议在工具描述中明示"mismatch 时响应含 `currentContent` 可直接用于重试"，让 Agent 减少一次手工校验串构造。
2. **`add_feature`/`update_feature` 的 location 采用 GenBank 1-based 字符串**，而其余所有工具坐标为 0-based——建议在描述开头加粗"location 为 1-based（与 digest 坐标不同）"，本测试未踩坑但易混淆。
3. **区域酶切全景获取路径不直观**：`find_restriction_sites` 按名称查询（设计如此），若需"某区域有哪些酶切位点"得先知道酶名。`get_region_view(compact=false)` 可列区域切口，建议在 `find_restriction_sites` 描述中指引一句"区域切口全景可用 get_region_view(compact:false)"。
4. **`read_sequence` 行宽固定 60 bp/10 bp 标尺**，对逐段人工比对（21 nt 段）友好度一般；建议响应保留机器可读 `sequence` 字段（现有设计已如此），保持即可。
5. **clone 场景辅助**：本次克隆靠 `edit_sequence` + 手动推导连接后序列完成，全部正确。若未来增加"双酶切+插入"级别的高级工具需谨慎（粘性末端语义复杂），当前原语集合已足够，可维持现状。

---

## 7. 最终产物关键序列验证结果

### 7.1 插入区序列（`read_sequence` 3910..4060 实测，0-based）

```
3930 AGCCGCTAGC AGTAAGCAAG AAGGTAGCCT CTAATAGTTA
3970 TATTCAAGCA TATTAGAGGC TACCTTCTTG CTTGCGAATT
```

逐段比对（顶链）：

| 段 | 预期 | 实测位置 | 结果 |
|----|------|----------|------|
| NheI 重构位点 GCTAGC | G@3934 + CTAG(粘端) + C | 3934..3939 | ✅ |
| 连接区 CAGT | 3939..3942 | 3939..3942 | ✅ |
| As re-com（sense）AAGCAAGAAGGTAGCCTCTAA | 3943..3963 | 3943..3963 | ✅ |
| loop TAGTTATATTCAAGCATA | 3964..3981 | 3964..3981 | ✅ |
| As（antisense）TTAGAGGCTACCTTCTTGCTT | 3982..4002 | 3982..4002 | ✅ |
| 连接区 GCG + EcoRI 重构 AATTC | 4003..4005 + 4006..4010 | GAATTC@4005..4010 | ✅ |

### 7.2 酶切位点变化（`find_restriction_sites`，克隆前 → 后）

| 酶 | 克隆前（pNP） | 克隆后（pNP-GOI） | 说明 |
|----|--------------|-------------------|------|
| NheI | 3934..3939（unique） | 3934..3939（unique） | 重构保留 |
| EcoRI | 4551..4556（unique） | 4005..4010（unique） | 重构保留（位置前移因 MCS 缩短） |
| XbaI | 3897..3902（unique） | 3897..3902（unique） | 不受影响 |
| SpeI | 4850..4855（unique） | 4304..4309（unique） | 不受影响（随 -546 平移） |

### 7.3 引物结合验证（`check_primer_binding`）

| 引物 | 结合 | 位点（0-based） | annealLen | Tm | 结论 |
|------|------|----------------|-----------|-----|------|
| U-F（protocol 验证引物） | + 链 | 3789..3808 | 20/20 | 60.9 | ✅ 正常结合 |
| Ftz（protocol 验证引物） | − 链 | 4383..4403 | 21/21 | 60.7 | ✅ 正常结合（原 4929..4949 平移 −546） |
| Primer-F（设计引物，71 nt） | + 链 | 3935..4005 | **71/71** | 76.0 | ✅ 插入区完整无错配 |
| Primer-R（设计引物，71 nt） | − 链 | 3939..4009 | **71/71** | 75.3 | ✅ 插入区完整无错配 |

### 7.4 其他关键数据

- 载体长度：8441 bp → **7895 bp**（8441 − 617 + 71 = 7895，与预期精确一致）。
- 落盘文件：`pNP-GOI.gbk` 26044 字节，重新 `open_file` 独立读回验证通过（7895 bp circular，`shRNA-GOI hairpin`@3943..4002 特征保留）。
- 特征副作用回显（edit_sequence 自动处理）：MCS1 裁剪为 3934..4005；attL1（位于被切片段内）移除；MCS1 之后全部特征坐标 −546 平移。

### 7.5 与 protocol 湿实验对照

- 退火产物 = Primer-F/Primer-R 双链（71 nt/71 nt，两端 CTAG/AATT 5' 突出）✅ 与 Protocol A.1.b 一致
- 载体双酶切位点 = EcoRI + NheI ✅ 与 Protocol A.1.c 一致
- 连接后 EcoRI/NheI 位点均重构保留 → 与 pNP 系统"发夹片段两侧保留限制性位点、支持后续多 shRNA 组装"的设计自洽 ✅
- U-F/Ftz PCR 引物在克隆质粒上结合正常，扩增子 ≈ 576 bp（本载体 MCS 比 protocol 原载体短，条带小于其 750 bp，属载体差异而非错误）

---

## 8. 结论

**构建成功**。已完成从给定 21 nt AS 序列出发的完整 pNP 转基因 RNAi 质粒 in silico 克隆：

- **产物**：`/Users/lidonglin/LibreGene/examples/tasks/test-results/rnai/pNP-GOI.gbk`（7895 bp circular）
- **插入内容**：shRNA 发夹（sense-loop-antisense = AAGCAAGAAGGTAGCCTCTAA · TAGTTATATTCAAGCATA · TTAGAGGCTACCTTCTTGCTT，60 nt），位于 3943..4002，已加特征标注 `shRNA-GOI hairpin`
- **验证三重交叉确认**：① read_sequence 逐段比对 ② find_restriction_sites 酶切位点重构 ③ check_primer_binding 设计引物 71 nt 完全匹配（外加 U-F/Ftz 结合位点平移 −546 佐证坐标正确性）
- **全部构建与验证操作均通过 MCP 工具完成**（14 次调用，13 成功），无原始输入文件被修改，所有打开项目已关闭。
