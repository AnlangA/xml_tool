# 基线记录：0.2.0 → 0.3.0 升级起点（计划第 0 步）

- 记录日期：2026-08-16
- 基线提交：`5d8e285` fix(deps): 修复 cargo audit 报告的依赖安全漏洞
- 后续每个阶段完成后，使用与本文件相同的命令组重新记录结果，存放于 `docs/BASELINE-<阶段>.md`。

## 验证命令与结果

| 命令 | 结果 |
|---|---|
| `cargo fmt --all --check` | 通过 |
| `cargo clippy --all-targets --all-features -- -D warnings` | 通过 |
| `cargo test --all-targets` | 106 通过 / 0 失败 / 0 忽略（62 单元 + 13 夹具 + 31 集成） |
| `cargo bench --no-run` | 通过 |

原有 93 个测试全部保留（62 单元 + 31 集成）；第 0 步新增 13 个夹具验证测试（`tests/fixture_tests.rs`）。

## 代码规模

- `src/` 合计 9,031 行 Rust（含本步新增 `src/fixtures.rs` 245 行）。
- 最大文件 `src/ui/main_panel.rs` 1,984 行，仍同时承担 UI、文档状态、文件 I/O、EXI、历史记录与任务调度。
- 其他：`examples/gen_fixtures.rs` 22 行；`tests/fixture_tests.rs` 217 行；`benches/xml_benchmark.rs` 186 行。

## 直接依赖（cargo tree --depth 1，共 20 项）

| 依赖 | 版本 | 用途 |
|---|---|---|
| quick-xml | 0.41.0 | XML 解析（第 1 步将被替换） |
| erxi | git `#4148209c` | EXI 编解码（已与计划第 8 步指定提交一致） |
| eframe / egui / egui_extras | 0.33.3 | GUI |
| catppuccin-egui | 5.7.0 | 主题 |
| egui-phosphor | 0.7.3（可选特性） | 图标；全特性闭包额外拉入 egui 0.29.1，与主 UI 版本不一致（计划第 5 步对齐到 0.11） |
| rfd | 0.15.4 | 文件对话框 |
| base64 / image | 0.22.1 / 0.25.10 | 图片预览 |
| serde / serde_json | 1.0 | JSON 导出 |
| thiserror / anyhow | 2.0 / 1.0 | 错误处理 |
| log / env_logger | 0.4 / 0.11 | 日志 |
| parking_lot / lru | 0.12 / 0.18 | 并发与缓存 |
| dev: criterion 0.5.1、tempfile 3.27.0 | | 基准与临时目录 |

## 现有基准项（benches/xml_benchmark.rs，10 项）

| 基准项 | 输入规模 |
|---|---|
| parse_small_xml | 27 节点 |
| parse_medium_xml | 625 节点 |
| parse_large_xml | 3,125 节点 |
| serialize/3、serialize/4、serialize/5 | 27/64/125 节点 |
| parse_with_attributes | 3 个 item |
| parse_real_world | ~30 节点 |
| tree_search/cold_leaf_query、tree_search/warm_leaf_query | 3,125 节点 |
| highlight_large_xml | 3,125 节点对应字符串 |
| exi/encode_medium_xml、exi/decode_medium_exi | 256 节点 |

结论与计划一致：最大输入仅约 3,125 节点，无法证明 20 MiB / 200,000 节点场景；第 9 步将扩展 20 MiB 解析/序列化、200,000 节点搜索、增量编辑、展平树、可见行高亮与四种 EXI 预设基准。

## 确定性夹具（本步新增）

生成器位于 `src/fixtures.rs`，入口 `cargo run --example gen_fixtures [目录]`（默认 `test_files/`）。所有夹具为常量的纯函数，重复生成字节一致。

### 大型夹具（`test_files/generated/`，不入库）

| 夹具 | 规格 | 验证 |
|---|---|---|
| `large-bytes.xml` | 精确 20 MiB（20,971,520 字节）UTF-8；50,001 个元素（root + 12,500 条 record×4） | 尺寸、元素数、确定性均有测试锁定 |
| `large-nodes.xml` | 精确 200,000 个元素（root + 199,999 个 item）；约 7.6 MiB | 元素数、≤20 MiB、确定性均有测试锁定 |

阈值常量：`EDIT_MAX_BYTES = 20 MiB`、`EDIT_MAX_ELEMENTS = 200,000`、`OPEN_MAX_BYTES = 256 MiB`。

### 保真夹具（`test_files/fidelity/`，入库，共 6 个）

| 夹具 | 覆盖点 |
|---|---|
| `full-fidelity.xml` | XML 声明（standalone）、DOCTYPE 内部实体（含嵌套 `&amp;`）、预定义实体、PI（xml-stylesheet 与元素内 PI）、CDATA、注释、默认/前缀命名空间、xml:lang、自闭合空元素、单引号属性、非 ASCII 文本、混合内容 |
| `utf16-le-bom.xml` | UTF-16LE + BOM（FF FE），多文种文本 |
| `utf16-be-bom.xml` | UTF-16BE + BOM（FE FF），与 LE 内容一致 |
| `crlf.xml` | 纯 CRLF 换行（无裸 LF） |
| `namespaces.xml` | 默认命名空间、多前缀、前缀属性、嵌套默认命名空间重定义 |
| `mixed-content.xml` | 文本/元素/注释/CDATA/PI 在同一父节点内交错 |

全部夹具已用独立解析器（Python `xml.dom.minidom`）验证为合法 XML。

## CI

`.github/workflows/ci.yml` 的 push 分支已改为 `[master, main]`，与仓库默认分支 `master` 一致；推送到 `master` 即触发 CI。
