# XML Tool 专业化升级计划

> 所有阶段按编号顺序执行，前一阶段未通过验收不得进入下一阶段。

## 一、目标与固定决策

将现有 XML/EXI 演示型编辑器升级为专业桌面工具，发布版本定为 `0.3.0`。

固定产品决策：

- 同时支持 Windows、macOS、Linux。
- 支持多文档标签、最近文件、会话恢复和崩溃恢复。
- XML 按完整保真模型处理：XML 声明、编码、命名空间、CDATA、注释、PI、DOCTYPE、实体引用、混合内容均不得静默丢失。
- 支持树视图和源码双向编辑。
- 支持简体中文、英文，首次启动跟随系统语言。
- 完整编辑保证范围：输入不超过 20 MiB 且元素节点不超过 200,000 个。
- 超过任一编辑阈值后自动进入大文件只读模式；超过 256 MiB 直接拒绝打开。
- XML 专业能力包含格式化、XPath 1.0、XSD 1.0 验证、结构差异比较和批量查找替换。
- EXI 提供四类预设及全部高级参数，但不建设事件流实验室。
- 首版不加入 XSLT、Relax NG、Schematron、XML 签名/加密、三方插件、云同步、协作编辑、结构合并和 20 MiB 以上的强制编辑。

## 二、现状审计与已确认缺口

当前项目约 7,600 行 Rust；`main_panel.rs` 约 1,984 行，已经同时承担 UI、文档状态、文件 I/O、EXI、历史记录和任务调度。

基线结果：

- 62 个单元测试和 31 个集成测试通过，共 93 个。
- 默认特性的严格 Clippy 检查通过。
- 基准代码只覆盖约 3,125 节点，无法证明 20 MiB 或 200,000 节点场景。
- `virtual_list`、`loading_indicator`、通用序列化/树缓存已经存在，但主工作流没有真正使用。
- CI 只监听 `main`，仓库实际默认分支是 `master`。
- `egui-phosphor 0.7` 为可选依赖并引入旧版 egui；全特性检查的依赖闭包与主 UI 版本不一致。

明确缺口：

| 类别 | 缺口 |
|---|---|
| XML 正确性 | 只按 UTF-8 读取；命名空间没有建模；PI、DOCTYPE、声明和 CDATA 不能保真编辑；格式化保存会改变原始结构；错误属性会被跳过 |
| 编辑能力 | 无新建文档、复制/粘贴/重复节点、属性重命名、混合内容编辑、源码编辑、批量替换 |
| 专业工具 | 无 XPath、XSD 验证、格式化配置、结构 Diff、诊断列表 |
| EXI | 只使用固定选项；无 alignment、compression、strict、fragment、schema、preserve、block/value 参数 UI；无编码报告 |
| UI | 英文硬编码、深色主题硬编码、Emoji 图标不一致、About 未实现、无任务进度、无问题面板、多标签和自适应布局 |
| 性能 | 文件读取、XML 解析、序列化、EXI 编解码、JSON 导出、图片解码均可能阻塞 UI；树递归全量渲染；源码全量高亮 |
| 内存 | 每次编辑通过 `Arc::make_mut` 克隆整棵树；撤销历史保存整个文档快照且无容量限制 |
| 架构 | UI 直接调用文件系统和编解码函数；领域对象字段全部公开；缓存模块与实际数据流脱节；错误类型主要依赖 `anyhow` |
| 工程化 | CI 分支错误、无三平台打包、无属性测试和模糊测试、无原子保存和外部文件变更检测 |

## 三、目标架构与接口

### 3.1 模块边界

最终目录按以下职责组织：

```text
src/
  app/
    state.rs              # AppState、消息分发、全局设置
    workspace.rs          # 标签、活动文档、会话恢复
  core/
    document.rs           # XML 文档领域模型
    command.rs            # 可撤销编辑命令
    history.rs            # 历史游标和内存预算
    diagnostic.rs         # 统一诊断结构
  services/
    document_io.rs        # 打开、原子保存、编码检测
    task_manager.rs       # 后台任务、取消、过期结果过滤
    search.rs             # 文本搜索和替换预览
    xpath.rs
    validation.rs
    diff.rs
    exi.rs
    recovery.rs
  ui/
    shell.rs              # 菜单、工具栏、文档标签、整体布局
    outline.rs            # 虚拟化 XML 树
    source_editor.rs      # Rope 源码编辑器
    inspector.rs
    problems.rs
    exi_workbench.rs
    dialogs/
  xml/                    # 对外兼容门面
  exi/                    # 对外兼容门面
```

完成迁移后删除 `ui/main_panel.rs`；任何 UI 模块不得直接调用 `std::fs`、`erxi` 或 XML 后端。

### 3.2 核心类型

必须落地以下类型，不允许用多个零散布尔值代替：

```rust
struct SessionId(u64);
struct Revision(u64);
struct NodeId(u64);

enum DocumentMode {
    Editable,
    LargeReadOnly,
}

enum XmlNodeKind {
    Element,
    Text,
    CData,
    Comment,
    ProcessingInstruction,
    Doctype,
    EntityReference,
}

struct QName {
    prefix: Option<String>,
    local_name: String,
    namespace_uri: Option<String>,
}

struct SourceRange {
    start_byte: usize,
    end_byte: usize,
    start_line: usize,
    start_column: usize,
    end_line: usize,
    end_column: usize,
}

struct Diagnostic {
    severity: Severity,
    code: String,
    message_key: String,
    arguments: Map<String, String>,
    source_range: Option<SourceRange>,
    node_id: Option<NodeId>,
}

struct DocumentSession {
    id: SessionId,
    path: Option<PathBuf>,
    file_type: FileType,
    mode: DocumentMode,
    document: XmlDocument,
    source: Rope,
    revision: Revision,
    saved_history_cursor: Option<usize>,
    selection: Option<NodeId>,
    source_draft_state: SourceDraftState,
}
```

### 3.3 公共 API

版本 `0.3.0` 对外提供：

- `parse_xml_bytes(&[u8], ParseOptions) -> Result<XmlDocument, XmlError>`
- `serialize_xml(&XmlDocument) -> Result<Vec<u8>, XmlError>`
- `serialize_xml_with_options(&XmlDocument, SerializeOptions)`
- `query_xpath(&XmlDocument, &str, &NamespaceBindings)`
- `validate_xsd(&XmlDocument, &Path)`
- `diff_xml(&XmlDocument, &XmlDocument, DiffOptions)`
- `encode_exi_with_options(&XmlDocument, &ExiSettings)`
- `decode_exi_with_report(&[u8], DecodeSettings)`

现有 `parse_xml`、`parse_xml_file`、`encode_xml_to_exi`、`decode_exi_to_xml` 保留为兼容包装器，并在文档中标记推荐的新接口。领域结构体改为私有字段，通过访问器和命令修改；该破坏性变化记录在 `MIGRATION.md`。

## 四、详细实施步骤

### 第 0 步：落盘计划并冻结基线

目标：建立可重复验证的起点。

实现路径：

1. 创建根目录 `AGENTS_PLAN.md`，内容为本计划。
2. 记录当前 93 个测试、代码行数、依赖树和现有基准项。
3. 增加两个确定性大型夹具生成器：
   - `large-bytes.xml`：20 MiB、约 50,000 元素。
   - `large-nodes.xml`：不超过 20 MiB、恰好 200,000 元素。
4. 增加包含 XML 声明、UTF-16、LF/CRLF、命名空间、CDATA、PI、DOCTYPE、内部实体和混合内容的保真夹具。
5. CI 的 push 分支改为 `[master, main]`。
6. 在每个后续阶段保存同一组功能和性能结果。

验收门槛：

- `AGENTS_PLAN.md` 已被 Git 跟踪。
- `cargo fmt --all --check` 通过。
- `cargo clippy --all-targets --all-features -- -D warnings` 通过。
- `cargo test --all-targets` 通过，原有 93 个测试无删除、无忽略。
- `cargo bench --no-run` 通过。
- 推送到 `master` 时 CI 会启动。

### 第 1 步：替换 XML 引擎并建立隔离层

目标：获得命名空间、保真 DOM、XPath、XSD、源码范围和流式解析能力，同时避免 UI 绑定第三方库。

实现路径：

1. 引入并精确锁定 `uppsala 0.9.0`。它提供 arena DOM、命名空间、XPath、XSD、源码范围和流式 PullParser，[公开能力与项目目标一致](https://github.com/kushaldas/uppsala)。
2. 新增内部 `XmlEngine` trait；唯一生产实现为 `UppsalaXmlEngine`。
3. 保留 `src/xml` 作为兼容门面，先把调用转到新引擎，再删除旧 `quick-xml` 解析器。
4. XML 输入统一使用字节接口，识别 BOM、XML 编码声明、UTF-8、UTF-16LE、UTF-16BE。
5. 保留原始字节、编码、换行风格、缩进风格、属性顺序、属性引号风格和空元素写法。
6. 安全限制固定为：
   - 最大嵌套深度 512。
   - 最大实体展开结果 16 MiB。
   - 禁止获取网络实体。
   - 禁止自动读取文档目录之外的外部实体。
   - 最大可打开输入 256 MiB。
7. 将第三方错误转换为 `XmlError`；错误必须包含稳定错误码和源码位置，不得只返回字符串。

验收门槛：

- MSRV Rust 1.88 上可构建。
- UTF-8、UTF-16LE、UTF-16BE 夹具均可打开。
- 未编辑文档执行保存后字节完全一致，包括 BOM 和换行符。
- 新增测试确认声明、CDATA、注释、PI、DOCTYPE、内部实体、命名空间前缀和混合内容不会消失。
- 外部网络实体测试中网络访问次数必须为零。
- “Billion Laughs” 类输入必须在实体预算内失败，不崩溃、不超过 256 MiB 增量内存。
- 旧 `quick-xml` 不再是项目直接依赖。

### 第 2 步：重建文档模型、索引和编辑命令

目标：让所有编辑原子化、可撤销、可验证，并消除每次操作克隆整棵树的问题。

实现路径：

1. 使用 arena 节点和稳定 `NodeId`；节点保存父、首子、末子、前后兄弟关系。
2. 建立按 `NodeId` 的 O(1) 访问，以及 QName、父节点、文档顺序索引。
3. 实现且仅允许以下命令修改文档：
   - `RenameElement`
   - `AddAttribute`
   - `RenameAttribute`
   - `SetAttributeValue`
   - `RemoveAttribute`
   - `SetNodeContent`
   - `InsertNode`
   - `DeleteNode`
   - `MoveNode`
   - `DuplicateSubtree`
   - `ReplaceWholeSource`
   - `BatchReplace`
   - `FormatDocument`
4. 每个命令先完整验证，再一次性提交；失败时文档、历史、选择和缓存均不变化。
5. 每个命令返回反向命令和 `ChangedSet`，由它精确失效搜索、树、源码和验证缓存。
6. 历史使用命令增量，不再保存整个 `XmlDocument`。
7. 历史上限固定为 1,000 条或 64 MiB，先达到者触发淘汰。
8. 连续文本输入在 750 ms 内且作用于同一字段时合并为一条历史记录。
9. 脏状态由历史游标与保存游标比较；撤销回保存点后必须自动恢复干净状态。
10. 树编辑产生最小源码变更；只重新序列化被修改的最小公共祖先。格式化命令是唯一允许主动重写整个文档排版的命令。

验收门槛：

- 每种命令至少包含成功、验证失败、撤销、重做四类测试。
- 100 次不同叶节点编辑后，历史增量内存不超过 64 MiB。
- 任一失败命令前后的文档序列化字节、历史游标和 revision 完全相同。
- 编辑一个叶节点时，夹具中该节点范围之外的字节保持不变。
- 属性重命名、命名空间修改、CDATA、PI、注释和混合内容均可撤销重做。
- `Arc::make_mut` 和完整文档历史快照从编辑路径移除。

### 第 3 步：建立工作区、后台任务和安全 I/O

目标：实现多标签，并保证重型操作永不直接运行在 UI 帧中。

实现路径：

1. 将应用状态拆为 `AppState`、`WorkspaceState`、`DocumentSession`、`TaskManager`。
2. 每个后台任务携带 `JobId + SessionId + Revision`；完成时只有三者仍匹配才允许应用结果。
3. 以下操作全部进入后台任务：
   - 文件读取和编码检测
   - XML/EXI 解析与序列化
   - 保存和导出
   - XPath、XSD、Diff、搜索索引构建
   - EXI 编解码
   - 大图片解码和缩放
4. 任务支持取消；库调用不能中途取消时，取消动作必须使返回结果失效。
5. 保存采用“同目录临时文件、flush、sync、原子 rename”流程。失败时原文件保持不变。
6. 文档在保存过程中继续被编辑时，只保存启动任务时的 revision；任务完成后标签仍显示未保存。
7. 同一路径重复打开时聚焦现有标签，不创建第二个会话。
8. 新建文件命名为 `Untitled-1.xml` 递增；首次保存使用 Save As，已有路径使用 Save。
9. 使用 `notify 8.2.0` 监控外部修改；事件去抖固定为 500 ms。[该库提供跨平台文件系统监听](https://docs.rs/notify/latest/notify/)。
10. 外部修改处理规则：
    - 当前文档干净：显示 Reload/Ignore 横幅。
    - 当前文档已修改：显示 Compare/Reload/Keep 横幅；Reload 必须二次确认。
11. 每 30 秒为脏文档写一次恢复快照；正常保存或明确丢弃后删除快照。
12. 启动时存在恢复快照则显示恢复选择页，不自动覆盖磁盘文件。
13. 退出时一次列出全部脏标签，允许 Save Selected、Discard Selected、Cancel。

验收门槛：

- 同时打开 10 个文档，标签选择、保存、关闭和恢复互不串状态。
- 构造过期任务后，结果不能覆盖新 revision。
- 模拟保存中断后，原文件校验和不变。
- 崩溃恢复测试能恢复源码、路径、光标、选中节点和脏状态。
- 外部修改的三种分支均有集成测试。
- UI 线程中不存在文件读写、XML/EXI 编解码或图片解码调用。

### 第 4 步：实现大文件模式和虚拟化基础

目标：完整编辑 20 MiB/200,000 节点，并对更大文件安全降级。

实现路径：

1. 打开任务先使用流式解析统计字节数、元素数和最大深度。
2. 当输入不超过 20 MiB 且元素不超过 200,000 时创建 `Editable` 文档。
3. 超过任一阈值且不超过 256 MiB 时创建 `LargeReadOnly` 文档。
4. 大文件只读模式仅支持：
   - 虚拟化结构浏览
   - 字面量搜索
   - 跳转匹配
   - 复制节点路径和可见文本
   - 导出选中子树
   - 另存原文件
5. 大文件模式禁用树编辑、源码编辑、替换、格式化、XPath、XSD 和结构 Diff，并显示明确原因；`0.3.0` 不提供强制编辑开关。
6. 重写树视图：使用展开展平列表、`HashSet<NodeId>` 展开状态和固定行高 `show_rows`；禁止递归绘制所有可见后代。
7. 折叠节点时从展平列表移除其后代；只渲染视口行和上下各 10 行缓冲。
8. 源码存储采用 `ropey 1.6.1`；其克隆共享底层数据，适合后台保存快照。[Rope 的常用编辑操作为对数复杂度](https://docs.rs/ropey/latest/ropey/struct.Rope.html)。
9. 源码高亮只处理可见行及上下各 100 行；缓存键为 revision、行号和主题。
10. 搜索索引按节点保存原文和大小写折叠文本；只重建 `ChangedSet` 涉及的节点。

验收门槛：

- `large-bytes.xml` 和 `large-nodes.xml` 均以可编辑模式打开。
- 20 MiB 夹具后台打开时间不超过 2.0 秒。
- 200,000 节点夹具首次字面量搜索不超过 500 ms，重复查询不超过 50 ms。
- 展开、折叠、滚动时主线程单帧最大阻塞不超过 50 ms，P95 帧时间不超过 16.7 ms。
- 打开任一目标夹具后，应用增量 RSS 不超过 320 MiB。
- 连续执行 100 次编辑后，RSS 相对刚打开时增加不超过 64 MiB。
- 20 MiB 以上文件明确进入只读模式；256 MiB 以上文件明确拒绝，不尝试构建 DOM。

### 第 5 步：重构专业 UI 与中英双语

目标：提供稳定、一致、可发现、可键盘操作的桌面界面。

实现路径：

1. 最终布局固定为：
   - 顶部：菜单、常用工具栏、文档标签。
   - 左侧：Outline/Search，宽度 260–420 px。
   - 中央：Source/Diff/EXI Result 工作区。
   - 右侧：Inspector，默认 320 px。
   - 底部：Problems/Tasks，可折叠。
2. 窗口宽度小于 1,100 px 时右侧 Inspector 改为抽屉；小于 850 px 时 Outline 也改为抽屉。
3. 菜单固定为 File、Edit、Search、XML、EXI、View、Help。
4. 增加 New、Open、Recent Files、Save、Save As、Save All、Reload、Close Tab、Close Others、Exit。
5. Outline 增加展开全部、折叠全部、复制 XPath、复制 XML、复制/剪切/粘贴、重复节点、删除和拖放移动。
6. Inspector 支持 QName、namespace URI、属性名/值、Text、CDATA、Comment、PI 编辑；所有修改通过命令提交。
7. 删除二次点击确认改为标准确认对话框，必须显示节点 QName 和后代数量。
8. About 对话框展示版本、提交号、许可证、XML 后端、EXI 后端。
9. 使用 `egui-phosphor 0.11` 与 egui 0.33.3 对齐，并在启动时加载图标字体；移除混用 Emoji。
10. 主题提供 System、Light、Dark 三项；首次默认 System。
11. 字体缩放范围 80%–180%，步长 10%；保存到设置。
12. 使用 `fluent-bundle 0.16.0`，资源文件为 `en-US.ftl` 和 `zh-CN.ftl`。[Fluent Bundle 提供运行时消息资源与参数格式化](https://docs.rs/fluent-bundle/latest/fluent_bundle/)。
13. 所有可见字符串、错误、对话框和工具提示必须使用翻译键；英文为缺失翻译的唯一回退语言。
14. 首次启动跟随系统语言；设置切换语言后当前界面立即刷新。
15. 打包 Noto Sans CJK SC 字体及许可证，保证三平台中文显示。
16. 键盘支持：
    - `Ctrl/Cmd+N/O/S/Shift+S/W`
    - `Ctrl/Cmd+Z/Y/F/H`
    - `F3`/`Shift+F3` 上下一个结果
    - `F6` 在 Outline、Source、Inspector、Problems 间切换
    - Tree 使用方向键、Home、End、Enter、Space
    - `F1` 快捷键帮助

验收门槛：

- 中英文翻译键集合完全相同；测试发现缺键时失败。
- 800×500、1280×800、1920×1080，中文/英文、亮色/暗色共 12 组 UI 快照通过。
- 800×500 下无按钮覆盖、文本裁剪导致的不可操作项。
- 全部主流程可只用键盘完成。
- 图标按钮全部有本地化工具提示和无障碍标签。
- `main_panel.rs` 已删除，任何单个新 UI 文件不超过 800 行。

### 第 6 步：实现树/源码双向编辑

目标：源码编辑和结构编辑共享一个文档事实来源，不允许出现两个未同步版本。

实现路径：

1. Source 使用 Rope 和虚拟行渲染，显示行号、当前行、匹配高亮和诊断下划线。
2. 源码变更进入 `Draft` 状态；Draft 存在时禁用树结构修改，并显示 Apply Source、Discard Draft。
3. Apply Source 在后台解析 draft：
   - 成功：以一条 `ReplaceWholeSource` 命令原子替换文档。
   - 失败：保留 draft，Problems 增加诊断并跳转到首个错误。
4. 树命令提交后，若不存在 draft，源码缓存按修改范围更新。
5. 切换标签、关闭标签、重载或退出时，未应用 draft 视为未保存修改。
6. 源码编辑支持查找、替换当前、替换全部、跳转行、复制全部。
7. 行列定义统一为从 1 开始；内部字节范围仍从 0 开始。
8. Undo/Redo 同时覆盖源码输入、Apply Source 和树操作。

验收门槛：

- 源码改名并 Apply 后，Outline 和 Inspector 同帧显示新 QName。
- 非法源码 Apply 不修改 DOM、不清除 draft，并定位准确行列。
- 树修改后，Source 显示相同内容且未修改区域字节不变。
- Draft 存在时所有树编辑入口均被禁用。
- UTF-16 文档编辑保存后继续使用原编码。
- 10,000 行源码滚动时只高亮可见窗口，P95 帧时间不超过 16.7 ms。

### 第 7 步：实现 XML 专业工具套件

目标：补齐专业 XML 日常分析和维护能力。

实现路径：

1. 格式化：
   - 默认 2 空格缩进。
   - 默认保留原换行风格。
   - 不排序属性。
   - 不删除 Text、CDATA、Comment、PI、DOCTYPE。
   - 混合内容不得插入改变语义的空白。
   - 作为单条 `FormatDocument` 命令提交，可一次撤销。
2. XPath 1.0：
   - 使用 XML 引擎的 XPathEvaluator。
   - 自动收集根节点可见命名空间。
   - 查询面板允许增加、修改前缀绑定。
   - NodeSet 结果可点击并同步 Outline、Source、Inspector。
   - String、Number、Boolean 结果显示类型和值。
3. XSD 1.0：
   - 可手工选择本地 XSD。
   - 可读取 `xsi:schemaLocation` 和 `xsi:noNamespaceSchemaLocation` 作为建议，但不自动联网。
   - include/import 只允许 schema 根目录及其子目录。
   - 编译缓存键为规范化路径、mtime、文件哈希。
   - 诊断必须包含严重度、消息、节点和源码范围。
4. 结构 Diff：
   - 比较来源为另一标签、磁盘文件或外部修改版本。
   - 默认按文档顺序和展开后的 QName 比较。
   - 属性顺序默认忽略，元素顺序不忽略。
   - 默认忽略纯格式缩进空白，保留有意义文本、CDATA、注释和 PI。
   - 输出 Added、Removed、Modified、Moved。
   - 使用 `similar 3.1.1` 做序列差异，并设置 5 秒计算截止时间。[该库支持 Myers、Patience 等算法和 deadline](https://docs.rs/similar/latest/similar/)。
   - 首版只显示和导航差异，不提供合并。
5. 批量查找替换：
   - 模式为 Literal、Regex、XPath。
   - 范围为元素名、属性名、属性值、文本、CDATA、注释、PI。
   - 替换前必须展示命中数量和变更预览。
   - Apply All 作为单条 `BatchReplace` 命令。
   - 任何替换导致非法 QName 或 XML 内容时，整批拒绝。
6. JSON 导出：
   - 保留 `Legacy` 模式兼容现有映射。
   - 增加默认 `Lossless` 模式，按有序节点数组保存 QName、属性、Text、CDATA、Comment、PI。
   - 导出前提供预览和目标模式选择。

验收门槛：

- 格式化后的文档重新解析后语义等价，且第二次格式化字节不变。
- XPath 覆盖全部轴、谓词、函数、命名空间和四种返回类型。
- XSD 至少包含 20 组 valid/invalid 夹具，覆盖 namespace、include、import、datatype facet、identity constraint。
- XSD 验证期间网络访问次数为零。
- Diff 测试能分别识别新增、删除、属性修改、文本修改、节点移动和仅格式变化。
- Batch Replace 在 1,000 个命中上可以预览、一次应用、一次撤销。
- Legacy JSON 现有测试保持通过；Lossless JSON 可以重建原节点顺序和节点类型。

### 第 8 步：实现专业 EXI 工作台

目标：完整利用现有 `erxi` 能力，提供可验证、可复现的 EXI 编解码工作流。

实现路径：

1. 将 `erxi` 固定到提交：
   `4148209c3fb11786ee81b54861e6f6d421905043`。
2. 增加 `ExiSettings` 和 `ExiReport`，UI 不直接暴露 `erxi` 类型。
3. 四个预设固定为：
   - Fidelity Bit-Packed：BitPacked，compression=false，保留 comments、PI、DTD、prefixes、lexical values、whitespace。
   - Byte Aligned：ByteAlignment，compression=false，其余与 Fidelity 相同。
   - Pre-Compression：PreCompression，compression=false，保留项全部关闭。
   - Maximum Compression：BitPacked，compression=true，保留项全部关闭，block size=1,000,000。
4. 高级选项包含：
   - alignment
   - compression
   - strict
   - fragment
   - preserve comments/PI/DTD/prefixes/lexical/whitespace
   - self-contained 与 QName 列表
   - schema id：None、BuiltinOnly、本地 XSD
   - datatype representation map
   - block size
   - value max length
   - value partition capacity
5. 所有参数变更调用同一验证器；非法组合禁止运行，并在对应控件旁显示原因，不得静默修正。
6. 编码输入使用当前 revision 的物化 XML 快照；结果不能覆盖更新后的文档状态。
7. 解码读取 EXI header 中的选项并填充报告；header 缺失时使用用户明确选择的 fallback 设置并显示警告。
8. 使用流式 API 处理 BitPacked 和 ByteAlignment；Compression/PreCompression 使用批处理 API。
9. EXI 内存上限固定为 512 MiB，最大输出 XML 为 256 MiB，最大深度 512。
10. 报告必须显示：
    - 输入/输出字节
    - 压缩率
    - 编解码耗时
    - 吞吐量
    - 最终有效选项
    - schema id
    - SHA-256
    - 被放弃的 XML 信息类型
11. 当前选项会丢失注释、PI、DTD、前缀或 lexical value 时，在运行前显示 Fidelity Warning。
12. EXI 保存使用与 XML 相同的原子保存流程。

验收门槛：

- 四个预设均完成 XML→EXI→XML 测试。
- Fidelity 预设保留注释、PI、DTD、前缀、词法值和空白。
- 非保真预设的丢失内容与报告完全一致。
- strict 与 preserve 冲突、compression 与 pre-compression 冲突、block size=0 均在运行前被拒绝。
- 至少 40 组 EXI 组合夹具通过。
- 截断、随机、畸形和超内存限制 EXI 输入均返回结构化错误，不 panic。
- 20 MiB 编码/解码期间 UI 仍可切换标签和取消任务；主线程单次阻塞不超过 50 ms。

### 第 9 步：性能收尾、缓存治理与观测

目标：使性能优化可测量、可回归，而不是停留在未接入组件。

实现路径：

1. 删除未使用的 `CacheManager` 入口；所有缓存统一由 `DocumentSessionCache` 管理。
2. 缓存键必须包含 `SessionId + Revision + 操作参数`，禁止只使用文档 version。
3. 缓存总预算固定为 128 MiB，按 LRU 淘汰；图片纹理单独限制为 64 MiB。
4. 搜索、源码行布局、展平树、序列化、XSD schema、XPath 编译结果分别建立命中统计。
5. 图片候选检测可以在 UI 线程执行，Base64/Hex 解码、图像解析和缩放必须在后台执行。
6. 增加主线程任务耗时观测；超过 16 ms 记录 debug 日志，超过 50 ms 记录 warning。
7. 扩展 Criterion：
   - 20 MiB 解析和序列化
   - 200,000 节点首次/缓存搜索
   - 100 次增量编辑和撤销
   - 展平树展开/折叠
   - 可见 200 行源码高亮
   - 四种 EXI 预设
8. CI 保存 benchmark JSON；相对 `0.3.0` 基线退化超过 15% 时失败。

验收门槛：

- 冷启动到首帧不超过 1.5 秒。
- 空闲时不持续请求 repaint，CPU 接近 0%。
- 20 MiB/200,000 节点性能满足第 4 步全部门槛。
- 100 次编辑的中位响应不超过 50 ms，P95 不超过 100 ms。
- 可见树 200 行与源码 200 行的布局时间分别不超过 16 ms。
- 缓存实际占用不会超过配置预算的 110%。
- 同一搜索重复执行时能观察到缓存命中，文档修改后只失效受影响项。

### 第 10 步：质量、安全、CI 和发布

目标：使 `0.3.0` 可以在三平台重复构建、安装和升级。

实现路径：

1. 测试分层：
   - 单元测试：核心命令、错误、设置、缓存。
   - 集成测试：打开—编辑—验证—保存—重开。
   - 属性测试：随机合法树的 parse/serialize/undo。
   - 模糊测试：XML、EXI、XPath、XSD 和图片数据入口。
   - UI 快照：布局、主题、语言、错误态、任务态。
2. 模糊测试每晚运行 30 分钟；PR 只执行 corpus 回归。
3. CI 矩阵固定为：
   - Ubuntu 24.04 x86_64
   - Windows Server 2025 x86_64
   - macOS 15 arm64
4. 每个平台执行 fmt、clippy、全测试和 release build；Linux 额外执行 benchmark smoke。
5. 增加 `cargo audit`、`cargo deny`，检查漏洞、重复许可证和未知 Git 依赖。
6. 打包：
   - Windows：MSI。
   - macOS：签名并 notarize 的 `.app` 和 `.dmg`。
   - Linux：`.deb` 和 AppImage。
7. 安装包包含应用图标、中英资源、Noto 字体和全部许可证。
8. 更新 README、快捷键、EXI 参数说明、性能范围、安全限制、迁移文档。
9. 版本号升级为 `0.3.0`，生成 changelog；不在本计划内自动提交、打标签或发布，发布动作需单独明确授权。

验收门槛：

- 三平台 CI 全绿。
- `cargo fmt --all --check`、严格 Clippy、全部测试、benchmark build、audit、deny 全部通过。
- nightly fuzz 连续 30 分钟无 panic、越界和无限循环。
- 三平台安装、启动、打开夹具、保存、卸载冒烟测试通过。
- 从 `0.2.0` 升级后旧文件、最近文件和用户设置不会损坏。
- README 中的每项功能都存在对应自动化测试或明确手工验收步骤。

## 五、最终完成定义

只有同时满足以下条件，专业化升级才算完成：

- `AGENTS_PLAN.md` 与实际实现一致，不保留已失效步骤。
- 所有 P0 XML 保真和安全问题解决。
- 多标签、双向源码编辑、中英双语、XML 专业套件和 EXI 高级工作台全部可用。
- 20 MiB/200,000 节点的功能、响应和内存门槛全部通过。
- 超阈值文件严格进入只读模式，256 MiB 以上严格拒绝。
- UI 中不存在同步重型操作。
- 三平台 CI、打包和安装冒烟全部通过。
- 原有 93 个测试全部保留，新增测试没有 ignored、临时跳过或仅记录不断言的情况。
- 不存在未完成的 TODO、空 About、未接入的性能组件或 README 超前声明。
