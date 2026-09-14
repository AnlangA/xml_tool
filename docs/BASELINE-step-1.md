# 阶段记录：第 1 步——替换 XML 引擎并建立隔离层

- 完成日期：2026-08-16
- 基线对比：`docs/BASELINE-0.2.0.md`

## 交付内容

| 文件 | 职责 |
|---|---|
| `src/xml/engine.rs` | `XmlEngine` trait + 唯一生产实现 `UppsalaXmlEngine`；`ParseOptions`/`SerializeOptions`/`SecurityLimits`；`EngineDocument`（DOM + 原始字节快照 + dirty 标记 + 源码范围/行列） |
| `src/xml/encoding.rs` | 字节级编码检测（BOM + XML 1.0 Appendix F）、UTF-8/UTF-16LE/BE 解码与再编码、声明交叉校验 |
| `src/xml/error.rs` | `XmlError`：稳定错误码（`input_too_large`、`entity_budget_exceeded`、`nesting_too_deep`、`entity_undefined` 等）+ 1 起始行列位置 |
| `src/xml/parser.rs` | 兼容门面重写：`parse_xml`/`parse_xml_file` 走引擎（命名空间感知关闭以保持旧行为），转换保持旧文本规范化语义；`serialize_xml` 输出格式不变 |
| `tests/engine_acceptance_tests.rs` | 13 个验收集成测试（见下） |

依赖变化：新增 `uppsala = "=0.9.0"`（精确锁定）；**移除 `quick-xml` 直接依赖**（仅作为 `erxi` 的传递依赖残留在闭包中，符合计划措辞）。

## 保真策略

- 解析始终从字节开始：`SourceSnapshot` 保存原始字节 + 解码文本 + 编码。
- **未编辑文档序列化 = 原始字节逐字节回放**（含 BOM、CRLF、属性引号风格、实体引用写法、空元素写法）。
- 编辑后的文档重新渲染并按原编码再编码（UTF-16 文档保持 UTF-16 + BOM）。
- 引擎 DOM 完整建模：声明、DOCTYPE（原始文本，含内部子集）、CDATA、注释、PI、命名空间前缀（QName 含 URI/前缀/本地名）、混合内容顺序；内部实体按预算展开为文本。`Document::into_static` 会丢弃输入引用，因此源码范围（字节偏移）保留在 DOM 中，行列由快照文本推算（`EngineDocument::source_location`）。

## 安全限制（固定）

| 限制 | 值 | 实现位置 |
|---|---|---|
| 最大输入 | 256 MiB，超限在解码前拒绝 | `SecurityLimits::max_input_bytes` |
| 最大嵌套深度 | 512 | 传入 `Parser::with_max_depth` |
| 实体展开预算 | 16 MiB/次解析 | 传入 `Parser::with_max_entity_expansion` |
| 网络访问 | 0 次（依赖树无网络代码；`uppsala` 零依赖） | tripwire 测试 |
| 外部实体 | SYSTEM/PUBLIC 声明可解析但从不加载；引用即报 `entity_undefined` | fail-closed 测试 |

## 验收门槛与证据

| 门槛 | 结果 |
|---|---|
| MSRV Rust 1.88 可构建 | ✅ `rustup run 1.88.0 cargo check --all-targets` 通过 |
| UTF-8/UTF-16LE/UTF-16BE 夹具可打开 | ✅ `all_fidelity_fixtures_open_through_the_byte_api`、`utf16_fixtures_report_their_detected_encoding` |
| 未编辑保存字节完全一致（含 BOM/换行） | ✅ `unedited_save_replays_original_bytes_for_every_fixture`（6 个夹具逐字节断言） |
| 声明/CDATA/注释/PI/DOCTYPE/内部实体/命名空间前缀/混合内容不消失 | ✅ 4 个专项测试 + `edited_documents_keep_every_construct_after_rerender`（编辑后再渲染也不丢失） |
| 外部网络实体网络访问次数为零 | ✅ 结构性保证（零网络依赖）+ `external_file_entities_are_never_fetched` + 依赖 tripwire |
| Billion Laughs 预算内失败 | ✅ `billion_laughs_is_contained_by_entity_budget`（8 层嵌套实体，16 MiB 预算，结构化错误 + 行列） |
| quick-xml 不再是直接依赖 | ✅ Cargo.toml 移除 + `quick_xml_is_no_longer_a_direct_dependency` tripwire |
| `cargo fmt --all --check` | ✅ |
| `cargo clippy --all-targets --all-features -- -D warnings` | ✅ |
| `cargo test --all-targets` | ✅ 145 通过 / 0 失败 / 0 忽略（88 单元 + 13 夹具 + 13 引擎验收 + 31 集成）；原有 93 个全部保留 |
| `cargo bench --no-run` | ✅ |

## 与计划的已知偏差

- 计划第 7 步要求 XSD 1.0 验证；`uppsala` 提供 **XSD 1.1** 验证器（1.0 的超集，绝大多数 1.0 schema 语义一致）。将在第 7 步以 XSD 1.1 实现并在 README/MIGRATION 中注明。
- UTF-16 无 BOM 文档按 Appendix F 的 `00 3C`/`3C 00` 签名识别，符合计划要求的编码集合。
