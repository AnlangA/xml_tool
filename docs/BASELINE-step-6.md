# 阶段记录：第 6 步——树/源码双向编辑

- 完成日期：2026-08-16
- 前置：`docs/BASELINE-step-5.md`
- 测试总览：215 通过 / 0 失败 / 0 忽略（新增 source_editor 单元 4 项 + shell 验收 3 项）

## 交付内容

| 项 | 说明 |
|---|---|
| `src/services/source_editor.rs` | `SourceDraft`（Rope 缓冲 + 基线 revision）、`parse_draft`（工作线程执行，纯函数）、`ApplyOutcome`（Parsed/Invalid + 1 起始行列）、`line_column_of` |
| 源码面板 | 可编辑 TextEdit；首击键 fork Draft（基线=当前 revision），后续击键仅更新缓冲；Draft 存在时显示 Apply Source / Discard Draft 与警示条 |
| 树编辑禁用 | `AppShell::commit` 在 Draft 存在时拒绝全部命令（问题面板提示），revision 不变 |
| Apply 流程 | 后台任务（APPLY_SESSION 通道）解析 Draft → 成功：一条 `ReplaceWholeSource` 命令原子替换（单次撤销覆盖整个 Apply），清空 Draft，缓存失效；失败：保留 Draft，Problems 增加带行列参数的诊断；revision 前进的陈旧结果丢弃 |
| 一致性 | 树命令的最小源码拼接（第 2 步）保证树编辑后源码未修改区域逐字节不变；Draft 未应用时切标签/关闭按未保存处理（`is_dirty` 含 Draft） |

## 验收门槛与证据

| 门槛 | 结果 |
|---|---|
| 源码改名并 Apply 后大纲/检查器同源 | ✅ `apply_source_renames_show_up_and_undo_covers_apply`（文档 QName 与源码同步；单次撤销恢复 Apply 前源码） |
| 非法 Apply 不改 DOM、保留 Draft、定位准确行列 | ✅ `invalid_apply_keeps_draft_and_reports_position`（revision/源码逐字节不变；诊断含 line/column 参数） |
| Draft 存在时树编辑入口全部禁用 | ✅ `draft_disables_all_tree_edits`（命令拒绝 + draft-active 诊断 + revision 不变；检查器 UI 经 `add_enabled_ui` 同步禁用） |
| 树修改后源码一致且未修改区域字节不变 | ✅ 第 2 步 `leaf_edit_leaves_all_other_bytes_untouched` 持续有效 |
| UTF-16 文档编辑保存保持原编码 | ✅ 第 3 步 `document_bytes_replay_originals_and_reencode_edits` 持续有效 |
| Undo/Redo 覆盖源码输入与树操作 | ✅ Apply=单命令；树命令=命令对（第 2 步全套四类测试） |
| fmt / clippy / test / bench | ✅ 215 全绿 |

## 与计划的偏差记录

- 行号栏/当前行高亮/诊断下划线/匹配高亮的渲染细节与“仅高亮可见行 ±100 行”缓存：源码面板当前为等宽 TextEdit；10,000 行滚动的 P95 帧预算依赖渲染端缓存，并入第 9 步性能收尾（届时接入 syntax_highlighter 可见窗口缓存与缓存键 revision+行号+主题）。
- 源码内查找/替换全部/跳转行/复制全部：树侧搜索已可用（SearchIndex + F3）；源码侧操作随第 7 步批量替换 UI 一并接线。
