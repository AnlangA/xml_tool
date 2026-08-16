# 阶段记录：第 9 步——性能收尾、缓存治理与观测

- 完成日期：2026-08-16
- 前置：`docs/BASELINE-step-8.md`
- 测试总览：243 通过 / 0 失败 / 0 忽略（新增缓存 2 项 + 帧观测 2 项单元）

## 交付内容

| 项 | 说明 |
|---|---|
| `services/session_cache.rs` | `DocumentSessionCache`：搜索索引与展平大纲统一治理；键=SessionId+Revision（大纲再加展开集摘要）；128 MiB 字节预算 LRU 淘汰（容忍 110%）；分类命中/未命中计数；`invalidate_session` / `invalidate_revisions`；shell 全量接入（提交/撤销/重做按 revision 失效，状态栏/大纲/搜索全部走缓存） |
| `services/frame_observer.rs` | 主线程分段观测：>16 ms debug 日志、>50 ms warning、256 环样本、最慢段查询；shell 各帧段接入（background-poll/top-bar/panels/status-bar） |
| 基准扩展（benches/xml_benchmark.rs） | `parse_20_mib`、`serialize_20_mib`、`search_200k_first`+`search_200k/cached`、`edits_20_mib/edit_and_undo`、`flat_tree/build_expanded_200k`、`highlight/visible_200_lines`、`exi_presets/{四预设}` |
| 旧 cache.rs | 仅剩 `xml_tree.rs` 遗留测试使用的 SearchCache；新代码一律走 DocumentSessionCache |

## 验收门槛与证据

| 门槛 | 结果 |
|---|---|
| 缓存键含 SessionId+Revision+参数 | ✅ `Kind::Search{session,revision}` / `Kind::Outline{session,revision,expansion}`；`session_cache_hits_and_invalidates_by_revision` |
| 同一搜索重复执行观察到命中；修改后仅失效受影响项 | ✅ misses=1→hits=1；revision 变更重建（misses=2），其他会话条目保留 |
| 缓存占用不超预算 110% | ✅ `evict_over_budget` 硬淘汰 + `within_budget`（110% 容忍）+ 微预算淘汰测试 |
| >16ms debug / >50ms warning | ✅ FrameObserver 阈值与记录测试 |
| 基准项扩展（6 组） | ✅ bench 编译通过（CI 冒烟）；JSON 保存与 15% 回归门在 CI 侧配置（第 10 步 CI 矩阵一并落地） |
| fmt / clippy / test / bench / MSRV 1.88 | ✅ 243 全绿 |

## 排序说明

- “冷启动 ≤1.5 s / 空闲零重绘 / P95 帧”属运行时验收：帧观测器与虚拟化已把每帧工作降为 O(视口)，最终数字随第 10 步打包冒烟在真机记录。
- 图片纹理 64 MiB 单独预算：base64_image 预览面板在第 5 步后未接回主工作流（其 20 项单元测试保留），纹理预算随其回归时生效。
