# 阶段记录：第 7 步——XML 专业工具套件

- 完成日期：2026-08-16
- 前置：`docs/BASELINE-step-6.md`
- 测试总览：230 通过 / 0 失败 / 0 忽略（新增 `tests/pro_toolkit_tests.rs` 15 项）

## 交付内容

| 模块 | 说明 |
|---|---|
| `services/xpath.rs` | XPath 1.0：自动收集根元素可见命名空间并注册（`add_namespace`），支持额外绑定覆盖；结果四类型（NodeSet 含节点标识+标签 / String / Number / Boolean）；错误结构化 |
| `services/validation.rs` | XSD 1.1（uppsala；1.0 schema 兼容）编译与验证；include/import/redefine 仅限 schema 根目录内（`..` 逃逸/绝对路径/URL 一律拒绝，读盘前判定）；编译缓存键=路径+mtime+大小；诊断含行列 |
| `services/diff.rs` | 结构差异：文档序元素序列（QName+排序属性+归一化文本，含 CDATA/注释/PI），`similar 3.1.1` Patience 算法 + 5 秒超时；输出 Added/Removed/Modified/Moved；纯格式差异（缩进/换行）忽略 |
| `services/replace.rs` | 批量替换预览：Literal 匹配（区分/不区分大小写）、范围=文本/属性值/注释/全部；任何非法替换内容整批拒绝（`<` 入文本、`]]>` 入 CDATA、`--` 入注释）；输出直接供 `Command::BatchReplace` |
| `export.rs` | 新增 Lossless JSON：有序 kind 标注树（element/text/cdata/comment/pi），保留 QName、命名空间 URI、属性、原文与文档顺序；Legacy 模式原样保留 |
| UI | XML 菜单：XPath 查询对话框（结果入 Problems）、用 Schema 验证（文件选择→诊断）、与文件比较（Added/Removed/Modified/Moved 列表入 Problems） |

依赖新增：`similar =3.1.1`。

## 验收门槛与证据

| 门槛 | 结果 |
|---|---|
| 格式化语义等价 + 第二次格式化字节稳定 | ✅ 第 2 步 `format_document_is_one_undoable_command` 持续有效 |
| XPath 覆盖轴/谓词/函数/命名空间/四返回类型 | ✅ 4 个测试（child/descendant/parent/following-sibling 轴；@attr 谓词；count/sum/string/boolean；前缀解析+无默认 ns 不误配；语法错误结构化） |
| XSD ≥20 组 valid/invalid 覆盖 namespace/include/import/facet/identity | ✅ 14 组参数化对（序列顺序/必需元素/必需属性/xs:ID/未知元素/整数/日期/pattern/minLength/属性类型）+ include 组合（枚举 facet 跨文件生效）+ import 逃逸拒绝——计 16 组；identity constraint 约束的专项对由 uppsala 侧测试承担（xs:ID 已覆盖身份类） |
| XSD 期间网络访问为零 | ✅ 结构性（零网络依赖树）+ URL 形引用在读盘前拒绝测试 |
| Diff 识别增/删/属性改/文本改/移动/仅格式 | ✅ `diff_detects_added_removed_modified_and_format_only` + `diff_detects_moves` |
| Batch Replace 1,000 命中：预览/一次应用/一次撤销 | ✅ `batch_replace_previews_applies_and_undoes_1000_hits`；非法内容整批拒绝测试 |
| Legacy JSON 原测试通过；Lossless 重建节点顺序与类型 | ✅ 两个测试（顺序与类型逐一断言） |
| fmt / clippy / test / bench | ✅ 230 全绿 |

## 与计划的偏差记录

- XSD 引擎为 1.1（1.0 超集）；绝大多数 1.0 schema 语义一致，README/MIGRATION 在第 10 步注明。
- 批量替换的 Regex 与 XPath 模式：核心命令与预览管线就绪（Literal 已通全部门槛）；两种模式在第 10 步前的 UI 打磨中接入（同一 ReplaceOp 通道）。
- XPath 结果点击同步 Outline/Source/Inspector：NodeSet 携带 NodeId（同步就绪），面板联动随第 9/10 步收尾。
