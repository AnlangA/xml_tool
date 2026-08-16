# 阶段记录：第 3 步——工作区、后台任务和安全 I/O

- 完成日期：2026-08-16
- 前置：`docs/BASELINE-step-2.md`

## 交付内容

| 文件 | 职责 |
|---|---|
| `src/services/task_manager.rs` | 后台任务：`JobId+SessionId+Revision` 三重匹配结果过滤、协作取消（`CancelFlag`）、MSRV 1.88 安全的计数信号量并发上限 |
| `src/services/document_io.rs` | `classify_bytes`（256 MiB 拒绝 / 20 MiB+200k 阈值判定，编码感知结构扫描含深度）、`document_bytes`（未编辑回放原始字节 / 编辑后按原编码再编码）、`save_bytes_atomically`（同目录临时文件→write→flush→sync→rename→目录 sync；失败清理临时文件） |
| `src/services/workspace.rs` | `WorkspaceState`/`DocumentSession`：多标签独立状态、同路径聚焦不重复开、Untitled-N 递增、Save As 赋路径、脏标签清单、关闭保他态 |
| `src/services/recovery.rs` | `RecoveryStore`：每会话 JSON 快照（源码/路径/光标/选择路径/脏态/时间戳），写后原子改名，保存/丢弃后删除，启动时加载存活快照 |
| `src/services/watcher.rs` | `notify 8.2.0` 推荐监视器 + 500 ms 去抖；Modified/Removed 分类；事件保留到可报告（修复了“轮询时丢弃未到期事件”的初版缺陷） |
| `tests/services_tests.rs` | 16 个集成测试（见下） |

## 验收门槛与证据

| 门槛 | 结果 |
|---|---|
| 过期任务结果不能覆盖新 revision | ✅ `stale_task_results_never_override_newer_revision`、`results_belonging_to_other_sessions_are_dropped` |
| 取消使结果失效 | ✅ `cancelled_jobs_have_their_results_discarded`、`cancel_session_invalidates_every_job_of_that_session` |
| 模拟保存中断后原文件校验和不变 | ✅ `interrupted_save_leaves_original_file_untouched`（before_rename 注入失败；原文件 FNV 校验一致；无 .tmp 残留） |
| 10 个文档标签互不串状态 | ✅ `ten_sessions_keep_tab_state_independent`（仅目标会话脏，光标/选择隔离，关闭他页不影响） |
| 崩溃恢复恢复源码/路径/光标/选择/脏态 | ✅ `recovery_snapshot_round_trips_everything` + 重写覆盖与损坏文件忽略 |
| 外部修改分支 | ✅ `external_modification_and_removal_are_detected`（Modified/Removed 实机 inotify 检测；干净/已脏两分支的 UI 横幅在 shell 接线时落地） |
| 阈值判定 | ✅ `threshold_documents_classify_correctly`（20 MiB=可编辑、200k=可编辑、超元素=只读）+ `oversize_input_is_refused_before_any_dom_work`（>256 MiB 拒绝，零 DOM 工作） |
| 编码感知保存 | ✅ `document_bytes_replay_originals_and_reencode_edits`（BOM 回放；UTF-16 编辑后保存仍是 UTF-16LE+BOM） |
| 同路径聚焦 / Untitled-N | ✅ `reopening_the_same_path_focuses_instead_of_duplicating`、`untitled_sessions_get_increasing_names_and_paths_on_save` |
| fmt / clippy / test / bench | ✅ 187 通过 / 0 失败 / 0 忽略；原有测试全部保留 |

## 排序说明（与计划的一致性）

本步交付全部服务层能力并达到可测验收；UI 帧循环接线（任务轮询、去抖事件、恢复选择页、退出对话框、30 秒快照定时器）随第 5 步新 UI shell 落地——第 5 步将删除 `main_panel.rs` 并按目标架构重组 UI，届时“UI 线程无文件 I/O/编解码”的静态验收随之生效（服务层已提供全部后台化原语）。
