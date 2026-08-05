# LibreGene MCP 功能测试报告

日期：2026-08-05
测试方式：3 个上下文独立的 DeepSeek V4 Flash 子代理，仅通过 MCP（Streamable HTTP + Bearer token 鉴权）顺序完成 `examples/tasks/` 下三项分子克隆任务。子代理全程未接触参考答案（Reference Result.gbk / Answer.txt），只操作 `_mcp_test/` 下的输入副本，LibreGene app 全程未关闭。报告中的对照核验在测试完成后由主代理进行。

## 总体结果

| 任务 | 工具调用 | 成功 | 失败 | 结果正确性（对照参考答案） |
|---|---|---|---|---|
| 1. Drosophila RNAi 质粒构建 | 10 tools/call | 10 | 0（另有 3 次会话层 406 为客户端漏 Accept 头） | ✅ 构建策略与结构正确；与参考序列仅差 2 bp（见下） |
| 2. 测序比对（BbsI 位点） | 7 tools/call | 7 | 0 | ✅ 结论与参考答案一致（样本 2、3 在 3000/3001 位突变成功） |
| 3. 引物设计（mEGFP） | 24 tools/call | 19 | 5（4 次代理导致会话失效 + 1 次脚本 bug） | ✅ 三类引物与参考答案实质一致 |

---

## 任务 1：Drosophila RNAi 质粒构建

**任务**：按 protocol 将 AS=TTAGAGGCTACCTTCTTGCTT 的 shRNA 构建进 pNP(Vm2) 载体。

**流程**：open_file → find_restriction_sites（定位 NheI 3934..3938 / EcoRI 4551..4555）→ read_sequence 读 MCS1 stuffer（617 bp）→ 按 protocol 公式手工设计 71 nt 退火寡核苷酸（ctagcagt+sense+loop+AS+gcg）→ edit_sequence 替换 stuffer → add_feature 标注 shRNA → save_file 存为 result.gbk（7895 bp）。

**正确性核验**（对照 Reference Result.gbk）：长度一致（7895 bp），全序列仅 2 bp 差异——参考的 sense 链为 `AAGCAAGAAGCTAGCCTCTCA`，子代理用的是给定 AS 的严格反向互补 `AAGCAAGAAGGTAGCCTCTAA`。子代理版本与任务 Prompt 给定的 AS 逐字互补，差异源于参考答案的 sense 序列本身与 Prompt 的 AS 不完全互补。判定：子代理忠实执行了任务要求。

**MCP 调用统计**：10 次 tools/call 全部成功（open_file ×1、find_restriction_sites ×2、read_sequence ×3、edit_sequence ×1、add_feature ×1、get_region_view ×1、save_file ×1）；会话层另有 3 次 406，根因是 curl 客户端漏带 `Accept` 头，重试即成功。

**亮点**：`edit_sequence` 的 removedFeatures/clippedFeatures 副作用回显、`save_file` 的 bytesWritten 受到好评。

---

## 任务 2：测序比对（BbsI 位点）

**任务**：判断哪个样本的 Sanger 测序显示成功突变移除了 BbsI 位点。

**流程**：open_file（pVA-MCS.dna，5214 bp）→ find_restriction_sites 定位 BbsI 唯一位点 2998..3003（GAAGAC）→ add_alignment ×3（path 直接读 .ab1）→ 分析各样本差异明细与 destroyedSites → read_sequence 核实模板上下文。

**结论**：样本 2、3 在 3000 位（0-based）发生 A→T 突变（GAAGAC→GATGAC），destroyedSites 均含 BbsI；样本 1 无此差异。**与参考答案（Answer.txt：第二、三个样本在 3001 位突变成功）完全一致**（坐标系差 1：参考答案为 1-based）。

**MCP 调用统计**：7 次 tools/call 全部成功（open_file、find_restriction_sites、add_alignment ×3、read_sequence、tools/list），0 错误。

**亮点**：`add_alignment` 直接吃 .ab1 文件、destroyedSites 字段一次调用即得出"酶切位点是否被破坏"的权威结论。

---

## 任务 3：引物设计（mEGFP 克隆 + A206K 诱变）

**任务**：设计带 BamHI/HindIII 尾巴的 mEGFP 扩增引物、菌落鉴定引物、A206K 诱变引物。

**流程**：open_file → find_restriction_sites + read_sequence 推演克隆结构（mEGFP 在反向链 complement(449..1168)）→ design_primers amplify（fwd_enzyme=BamHI, rev_enzyme=HindIII，internalSites=[]）→ design_primers mutagenesis（mutation 自检块确认 GCG→AAG、Ala→Lys、aaPositionExcludingMet=206）→ check_primer_binding ×4 + search_sequence ×2 验证结合与唯一性。

**正确性核验**（对照 Reference Result.gbk 中的 primer_bind 特征）：
- 扩增引物：策略与参考一致（保护碱基 + BamHI/HindIII 位点 + 相同退火区，长度略异）✓
- A206K 诱变引物：与参考序列实质相同（仅 3' 端长度差 3 bp，FOR/REV 命名方向相反）✓
- 菌落鉴定：参考用 M13 Fwd + M13 Rev（按大小区分）；子代理选 M13 Fwd + 插入片段内部反向引物（正确克隆 230 bp / 空载体无条带），方案合理且判读更直接 ✓

**MCP 调用统计**：24 次 tools/call，19 成功 / 5 失败。
- 4 次失败（read_sequence 空响应 / 纯文本 404 "Session not found"）：根因是本机 `http_proxy=127.0.0.1:7890`，curl 走代理后 keep-alive 仅 4 秒，代理断开后端连接导致 MCP 会话失效。`--noproxy '*'` 直连后 13 次调用全部成功。
- 1 次失败：子代理脚本提取 session id 的 bug（非服务端问题）。

**亮点**：mutagenesis 的 mutation 自检块（密码子/氨基酸变化、正/负链上下文、反向链 CDS 自动 RC 换算）使 A206K 验证零成本；发现并正确处理了 gbk `/translation` 元数据与实际序列不一致的问题（以序列为准）。

---

## 错误汇总与根因分析

| 错误 | 次数 | 根因 | 服务端责任？ |
|---|---|---|---|
| HTTP 406 Not Acceptable | 3（任务1） | 客户端漏带 `Accept: application/json, text/event-stream` 头 | 否，但 406 响应无可读错误体，难诊断 |
| read_sequence 空响应 / 纯文本 404 Session not found | 4（任务3） | 系统代理（7890）keep-alive 4 秒断连导致 MCP 会话失效 | 部分是：会话失效错误表达不一致（空 body / 纯文本 404），无标准 JSON-RPC 错误码 |
| 脚本 SID 提取 bug | 1（任务3） | 子代理自身脚本 | 否 |

三项任务合计：**41 次 tools/call，36 成功**；所有失败均为客户端环境/用法问题，**MCP 服务端 0 个功能性错误**。

## MCP 改进建议（合并自三个子代理）

1. **错误响应可诊断性**（最高优先级）：
   - 缺 Accept 头的 406 应返回 JSON-RPC 错误体说明原因
   - 会话失效应返回结构化错误（如 `-32001 SessionNotFound`），统一替代空 body / 纯文本 404
2. **会话健壮性**：会话不应绑定 TCP 连接（或提供更长 TTL），避免代理/连接复用环境掉会话
3. **新增 `list_alignments` / `get_alignment`**：目前比对只能 add/remove，无法只读查询已添加比对的差异明细
4. **`destroyedSites` 附破坏原因**：每条加 `cause` 字段（哪个 mismatch/deletion/insertion 导致），免去手工交叉比对
5. **`check_primer_binding` 返回全部结合位点**（现只返回第一个），用于脱靶检测
6. **`design_primers` 候选附退火区模板坐标**，省去再调 check_primer_binding 推断
7. **响应体积控制**：regionView 在 content.text 与 structuredContent 中重复，多文件连续操作时冗余；建议加 `includeRegionView: false` 之类的精简参数
8. **新增 shRNA 设计模式**：`design_primers` 增加 `mode: "shrna"`（输入 AS + loop，输出两条退火寡核苷酸），覆盖 RNAi 克隆这一常见场景
9. **小问题**：`add_feature` 的 featureId 用时间戳，同毫秒理论可冲突，建议单调序号；`read_sequence` 文本标尺的 wrap 提示在无跨绕窗口也出现，略有误导

## 测试产物

- 任务 1 结果：`_mcp_test/task1_drosophila/result.gbk`（7895 bp）
- 任务 2、3 为只读分析任务，结论见上文（未保存修改）
- 原始任务目录与参考答案全程未被修改
