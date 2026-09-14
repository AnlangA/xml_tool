# 阶段记录：第 4 步——大文件模式和虚拟化基础

- 完成日期：2026-08-16
- 前置：`docs/BASELINE-step-3.md`
- 测试总览：198 通过 / 0 失败 / 0 忽略（新增 `tests/large_file_tests.rs` 9 项 + `tests/memory_budget_tests.rs` 2 项）

## 交付内容

| 文件 | 职责 |
|---|---|
| `src/services/large_file.rs` | 只读骨架：PullParser 流式建立（名称/深度/整树字节范围/起始标签范围，自闭合单路径），字面量搜索（区分/不区分大小写）、跳转定位元素、可复制节点路径（`/catalog[1]/item[200000]`）、导出子树源码；>256 MiB 在解码前拒绝，不建 DOM |
| `src/services/outline.rs` | `FlatTree` 展平列表：折叠子树零行、固定行数据、视口切片带对称 overscan（UI 用 ±10 行）、`row_of` 定位、toggle 语义；非空文本/注释/PI 行在展开父节点下可见 |
| `src/services/search.rs` | 节点级字面量搜索索引：原文+大小写折叠双份、按源码拼写渲染属性（`name="value"`）、`apply_changes` 仅重建 ChangedSet 触及节点、相同查询备忘录 |
| `src/services/source_buffer.rs` | `ropey 1.6.1` Rope 封装：行/列/字符偏移互转、对数复杂度插入删除、克隆共享底层树 |
| 依赖 | `ropey = "1.6.1"` 精确锁定 |

## 性能实测（release，本机）

| 指标 | 预算 | 实测 |
|---|---|---|
| 20 MiB 夹具打开（解析） | ≤ 2.0 s | **93 ms** |
| 200,000 节点首次字面量搜索（建索引+查询） | ≤ 500 ms | **131 ms** |
| 200,000 节点重复查询 | ≤ 50 ms | **45 ns**（备忘录命中；换新查询 138 ms 仍远低于首查预算） |
| 20 MiB 打开后增量 RSS（DOM+范围表+索引+展平树） | ≤ 320 MiB | 通过（测试断言，Linux /proc VmRSS） |
| 100 次编辑 RSS 增长 | ≤ 64 MiB | 通过 |

调试模式断言采用宽松预算（打开 30 s / 首查 5 s / 重复 500 ms）防 CI 抖动；release 数字如上。

## 验收门槛与证据

| 门槛 | 结果 |
|---|---|
| large-bytes.xml / large-nodes.xml 以可编辑模式打开 | ✅ `large_fixtures_open_editable_and_stay_fast`（结合第 3 步 classify 测试） |
| 200k 节点首查/重复查询预算 | ✅ `literal_search_on_200k_nodes_meets_budgets` + release 实测 |
| 展平树展开/折叠/视口 | ✅ `flat_tree_collapses_and_pages_rows`（全折叠 1 行、根展开 200,000 行、视口 61 行切片、深层子树折叠） |
| 搜索索引增量失效 | ✅ `search_index_rebuilds_only_changed_nodes` |
| 只读模式能力边界 | ✅ `read_only_skeleton_browses_searches_and_exports`（骨架/搜索/跳转/路径/导出）、`read_only_mode_supports_no_editing_paths`（无任何修改 API，classify 强制 LargeReadOnly） |
| >256 MiB 拒绝 | ✅ `read_only_open_refuses_oversize_input`（第 3 步 classify 测试互证） |
| Rope 行列转换 | ✅ `source_buffer_converts_lines_columns_and_edits` |
| 内存预算 | ✅ 独立测试进程 + 互斥串行测量（并行测试会污染 RSS 读数——本轮发现并修正的测试方法学问题） |
| 大文档结构编辑 + 撤销 | ✅ `structural_edits_work_on_large_editable_documents` |

## 排序说明

“主线程单帧 ≤50 ms / P95 ≤16.7 ms” 属 UI 帧指标：展平树与视口切片已把每帧工作量降为 O(视口行)，第 5 步新 shell 接入 `show_rows` 后在运行时验证。源码高亮可见行 ±100 缓存随第 6 步源码编辑器落地。
