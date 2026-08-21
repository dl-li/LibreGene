# RNAi 质粒构建任务日志

## 任务目标
在 pNP(Vm2) 出发载体基础上，按 protocol 策略构建靶向 AS 序列 `TTAGAGGCTACCTTCTTGCTT` 的 RNAi 质粒，最终保存为 `result.gbk`。

## Protocol 构建策略摘要
1. shRNA 设计：AS（antisense）= 用户给定 21 nt；sense = AS 的反向互补。
2. 合成两条退火引物：
   - Primer-F: `ctagcagt` + sense + `tagttatattcaagcata` + AS + `gcg`
   - Primer-R: `aattcgc` + sense + `tatgcttgaatataacta` + AS + `actg`
   退火后两端形成 NheI/EcoRI 黏性末端。
3. 载体酶切：用 EcoRI/NheI 双酶切 pNP(Vm2)，回收约 8 kb 大片段。
4. 连接：将退火产物连入线性化载体。
5. 最终产物预期：NheI 与 EcoRI 切口之间替换为 71 bp 的 shRNA 表达盒，并恢复两个酶切位点。

## MCP 工具调用记录

| 工具名 | 关键参数 | 成功/失败 | 结果摘要或错误信息 |
|---|---|---|---|
| open_file | path=`pNP(Vm2).dna` | 成功 | 打开 circular DNA，8441 bp，projectId 为文件路径 |
| find_restriction_sites | enzymes=[EcoRI,NheI] | 成功 | EcoRI 唯一切点在 4552^4553；NheI 唯一切点在 3935^3936 |
| read_sequence | 3920..4570 | 成功 | 确认 MCS1 区域含 GCTAGC(NheI) 与 GAATTC(EcoRI) |
| read_sequence | 3936..4552 | 成功 | 读取将被替换的 617 bp 片段，用于 expected_old 校验 |
| edit_sequence | start=3936,end=4552,replacement=71 bp shRNA insert, expected_old | 成功 | 替换 617 bp 为 71 bp；产物长度 7895 bp；MCS1 被裁剪为 3935..4006；attL1 被移除 |
| add_feature | start=3936,end=4006,name=shRNA-GOI,ftype=misc_feature,strand=+ | 待调用 | 标注插入的 shRNA 表达盒 |
| add_feature | start=3936,end=4006,name=shRNA-GOI,ftype=misc_feature,strand=+ | 成功 | 新增 shRNA-GOI feature，id=feature_1787112204237_0 |
| read_sequence | 3920..4030 | 成功 | 确认连接处序列：GCTAGC(NheI) + shRNA insert + GAATTC(EcoRI) 连续正确 |
| search_sequence | GCTAGC | 成功 | 唯一命中 3935..3940（NheI 位点恢复） |
| search_sequence | GAATTC | 成功 | 唯一命中 4006..4011（EcoRI 位点恢复） |
| search_sequence | TTAGAGGCTACCTTCTTGCTT | 成功 | 正链 3983..4003 命中 AS；反链 3944..3964 命中其反向互补（sense） |
| search_sequence | TAGTTATATTCAAGCATA | 成功 | 唯一命中 3965..3982（loop 区） |
| find_restriction_sites | EcoRI,NheI | 成功 | EcoRI 位点 4006^4011；NheI 位点 3935^3940；均为 unique |
| save_file | path=`result.gbk` | 待调用 | 保存最终产物 |
| save_file | path=`/Users/lidonglin/LibreGene/examples/tasks/mcp-test-round2/rnai/result.gbk` | 成功 | 文件写入 26036 bytes，产物长度 7895 bp |

## 产物自查结果

- **产物文件**: `/Users/lidonglin/LibreGene/examples/tasks/mcp-test-round2/rnai/result.gbk`
- **产物长度**: 7895 bp（出发载体 8441 bp - 被替换片段 617 bp + 71 bp insert）
- **插入区间**: 3936..4006（71 bp，已添加 misc_feature `shRNA-GOI`）
- **关键序列验证**:
  - NheI 位点恢复：`GCTAGC` 位于 3935..3940，unique。
  - EcoRI 位点恢复：`GAATTC` 位于 4006..4011，unique（最后一个 G 来自 insert，AATTC 来自载体）。
  - AS 序列 `TTAGAGGCTACCTTCTTGCTT` 正链位于 3983..4003。
  - Sense 序列 `AAGCAAGAAGGTAGCCTCTAA` 在反链 3944..3964 命中。
  - Loop `TAGTTATATTCAAGCATA` 位于 3965..3982。
  - 连接处上下文（3920..4030）: `...CAGCCGCTAGCAGTAAGCAAGAAGGTAGCCTCTAATAGTTATATTCAAGCATATTAGAGGCTACCTTCTTGCTTGCGAATTCAGGCGAGACATCG...`，符合 protocol 预期结构。

## 卡顿点清单

1. **引物方向与黏性末端的对应关系**：protocol 给出的 Primer-F/Primer-R 格式较抽象，最初不确定哪条链应作为最终产物 top strand、EcoRI/NheI 切口如何恢复。通过读取载体切口两侧序列并手工推导后才确认：top strand 应为 Primer-F 序列（`ctagcagt`...`gcg`），NheI 位点由载体 G + insert CTAGC 恢复，EcoRI 位点由 insert 最后一个 G + 载体 AATTC 恢复。
2. **Loop 长度与碱基核对**：protocol 中 loop 文本 `tagttatattcaagcata` 需逐字计数（18 nt），防止多写/漏写 T；实际编辑后通过 `search_sequence` 验证 loop 唯一命中，确认无误。
3. **edit_sequence 的 expected_old 获取**：由于替换区域 617 bp 较长，使用 `read_sequence` 单独读取 3936..4552 作为乐观校验，避免手抄错误。

## MCP 调用统计

| 工具 | 调用次数 | 成功 | 失败 |
|---|---|---|---|
| open_file | 1 | 1 | 0 |
| read_sequence | 3 | 3 | 0 |
| find_restriction_sites | 2 | 2 | 0 |
| edit_sequence | 1 | 1 | 0 |
| add_feature | 1 | 1 | 0 |
| search_sequence | 4 | 4 | 0 |
| save_file | 1 | 1 | 0 |
| **合计** | **13** | **13** | **0** |
