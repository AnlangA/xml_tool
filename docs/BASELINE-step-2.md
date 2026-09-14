# 阶段记录：第 2 步——重建文档模型、索引和编辑命令

- 完成日期：2026-08-16
- 前置：`docs/BASELINE-step-1.md`

## 交付内容

| 文件 | 职责 |
|---|---|
| `src/core/document.rs` | `XmlDocument`：引擎 DOM + 当前源码 + 节点字节范围表 + QName 索引 + 文档序缓存 + revision。字段全部私有，读取走访问器。含 `NodeId(u64)`、`Revision(u64)`、`XmlNodeKind`（含 Doctype/EntityReference 预留）、`QName`、`QNameSpec`、`SourceRange`、NCName/QName 校验 |
| `src/core/command.rs` | 13 个命令 + 内部 `RestoreNode` 撤销原语。验证→提交→反向命令→`ChangedSet`。失败时字节、范围、索引、revision 均不变 |
| `src/core/history.rs` | 命令对（forward+reverse）增量历史：1,000 条 / 64 MiB 预算（先到先淘汰）、750 ms 同字段合并、保存游标脏标记、redo 失效 |
| `src/core/diagnostic.rs` | `Diagnostic` + `Severity`（消息键 + 参数，本地化就绪） |
| `tests/command_history_tests.rs` | 26 个测试：每命令 成功/验证失败/撤销/重做 四类 + 全部门槛断言 |

## 源码最小变更策略

- 文本/属性/改名命令：只重渲染受影响节点自身的字节区间，区间外字节逐字节不变（`leaf_edit_leaves_all_other_bytes_untouched` 锁定）。
- 结构命令：删除=字节区间摘除；移动=摘除后按修枝树计算目标偏移再插入（先删后算，避免过期偏移）；插入到有子父节点=在锚点兄弟边界拼接。
- 子树范围表在拼接前后通过 `take_subtree_entries` 保护，避免拼接位移破坏内部条目；撤销按 LIFO 归纳保证字节精确回放。
- `<e/>` ↔ `<e>…</e>` 展开/回缩与命令对称（插入独生子/删除独生子时重渲染父元素）。
- 仅 `FormatDocument` 与 `ReplaceWholeSource`（源码编辑器 Apply）允许整体重写；格式化第二次执行字节稳定。

## 命令清单（13 个 + 撤销原语）

`RenameElement`（含命名空间重绑定：显式 URI 自动声明 xmlns；未声明前缀拒绝）、`AddAttribute`、`RenameAttribute`、`SetAttributeValue`、`RemoveAttribute`、`SetNodeContent`（Text/CData/Comment/PI）、`InsertNode`、`DeleteNode`、`MoveNode`（禁止移入自身子树、深度≤512）、`DuplicateSubtree`、`ReplaceWholeSource`、`BatchReplace`（整批验证，一个非法全批拒绝；单 revision）、`FormatDocument`；内部 `RestoreNode`（重挂 arena 孤儿节点 + 字节精确回放）。

## 验收门槛与证据

| 门槛 | 结果 |
|---|---|
| 每命令 成功/失败/撤销/重做 四类测试 | ✅ 26 测试（命令全覆盖，含 CDATA/PI/注释/混合内容/命名空间改名） |
| 100 次叶节点编辑历史 ≤ 64 MiB | ✅ `hundred_leaf_edits_stay_within_history_budget`（每命令增量估计，无文档快照） |
| 失败命令前后字节/游标/revision 完全相同 | ✅ `failed_commands_leave_bytes_cursor_revision_untouched`（7 类失败命令） |
| 叶节点编辑区间外字节不变 | ✅ `leaf_edit_leaves_all_other_bytes_untouched`（前后段逐字节比对） |
| 属性重命名/命名空间修改/CDATA/PI/注释/混合内容可撤销重做 | ✅ 各专项测试 |
| `Arc::make_mut` 与全文档历史快照移出编辑路径 | ✅ 结构性测试扫描 `src/core` 禁止 `Arc::make_mut`；历史只存命令对 |
| fmt / clippy --all-features / test / bench --no-run | ✅ 171 通过 / 0 失败 / 0 忽略（88 单元 + 26 命令历史 + 13 夹具 + 13 引擎验收 + 31 集成）；原有 93 个全部保留 |

## 已知边界（记录在案，后续步骤消化）

- 撤销 DeleteNode 恢复字节精确；撤销 RenameElement/属性类命令在受影响元素区间内可能规范化属性引号为双引号（区间外不变）——符合“最小公共祖先”语义。
- 混合内容中元素重渲染会保留文本节点的原空白；`remap_descendants` 按文档序 + 节点类别匹配重建范围，结构分歧时安全降级为丢弃范围。
- `BatchReplace` 当前覆盖文本/CDATA/注释/属性值替换（第 7 步的批量查找替换 UI 直接复用）；元素名/属性名批量替换在第 7 步扩展。
