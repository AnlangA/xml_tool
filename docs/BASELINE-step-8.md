# 阶段记录：第 8 步——专业 EXI 工作台

- 完成日期：2026-08-16
- 前置：`docs/BASELINE-step-7.md`
- 测试总览：239 通过 / 0 失败 / 0 忽略（新增 `tests/exi_workbench_tests.rs` 9 项，44 组合夹具）

## 交付内容

| 项 | 说明 |
|---|---|
| `services/exi_workbench.rs` | `ExiSettings`（对齐/压缩/strict/fragment/preserve 全开关/self-contained/schema id 三选/block size/value max length/value partition capacity）+ 四预设 + `validate()`（strict×preserve、compression×PreCompression、block size=0、self-contained×PreCompression 全部运行前拒绝并给出原因）+ `dropped_items()`（Fidelity Warning 数据源） |
| `ExiReport` | 输入/输出字节、压缩率、耗时、吞吐量、生效选项（解码从 EXI header 读取并回填报告）、schema id、SHA-256（sha2 依赖）、丢弃的信息类型 |
| 编解码 | 编码吃文档源码快照（不观测后续编辑）；解码先做 512 MiB EXI 输入预算与 256 MiB 输出 XML 预算检查；全部错误结构化字符串返回，无 panic |
| UI | EXI 菜单 → 工作台对话框：四预设单选 + 编码按钮 + 报告行 + 非保真预设的 Fidelity Warning（先警示后运行） |

erxi 依赖仍锁定在计划指定提交 `4148209c`（第 0 步起未变）。

## 验收门槛与证据

| 门槛 | 结果 |
|---|---|
| 四个预设 XML→EXI→XML 测试 | ✅ `all_four_presets_round_trip` |
| Fidelity 预设保留注释/PI/DTD/前缀/词法值/空白 | ✅ `fidelity_preset_preserves_every_information_item`（注释/PI/前缀/文本逐项断言；CDATA 内容以转义文本语义保留——见偏差） |
| 非保真预设丢失内容与报告一致 | ✅ `non_fidelity_presets_report_their_losses`（dropped 列表与实际丢失互证） |
| strict×preserve、compression×pre-compression、block size=0 运行前拒绝 | ✅ `conflicting_options_are_rejected_before_running`（含编码入口拒绝） |
| ≥40 组合夹具 | ✅ `forty_setting_combinations_round_trip`（4 预设 × strict × 3 block size + schema id × value limits × capacity = 44 组） |
| 截断/随机/畸形/超限输入结构化错误不 panic | ✅ `malformed_exi_inputs_fail_with_structured_errors`（多点截断、随机 256B、空输入）+ `exi_budgets_are_enforced` |
| 旧兼容包装器 | ✅ `legacy_exi_wrappers_still_work`（原基准与集成测试同步有效） |
| 报告字段完整 | ✅ `encode_uses_the_snapshot_not_live_document_state`（字节/比率/耗时/吞吐） |
| fmt / clippy / test / bench | ✅ 239 全绿 |

## 与计划的偏差记录

- **fragment 模式**：erxi 编码器接受 fragment 选项但解码器无法回读（PrematureEndOfStream / InvalidCompactId）——上游限制。设置项保留并透传，组合矩阵以 schema id / 值限制替代 fragment 维度；上游修复后可直接并入矩阵。
- **self-contained QName 列表**：erxi 的 QName 为内部驻留 id 类型，库外无法按名字构造；启用 self-containment 时按 erxi 语义包裹全部元素（空列表语义）。
- **流式 API（BitPacked/ByteAlignment 用流式、压缩用批处理）**：erxi 公共入口为批处理 `decode`/`encode`；流式 `decode_iter_with_options` 存在但 encode 侧无公共流式入口。20 MiB 编解码的后台化由任务管理器承担（不阻塞 UI 帧），内存预算在上边界强制。
- **CDATA**：EXI 标准无 CDATA 保留项（对应 Preserve 无该标志）；内容以转义文本保真，报告不将其列为丢失。
- UI 高级参数面板（全部开关逐一可调）：对话框当前提供预设 + 编码 + 报告 + 警示；逐项开关面板随第 10 步收尾（服务层全量就绪且有测试）。
