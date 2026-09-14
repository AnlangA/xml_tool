# 简体中文（zh-CN）界面字符串。键集合必须与 en-US.ftl 完全一致。

app-name = XML Tool
menu-file = 文件
menu-edit = 编辑
menu-search = 搜索
menu-xml = XML
menu-exi = EXI
menu-view = 视图
menu-help = 帮助

action-new = 新建
action-open = 打开…
action-recent = 最近文件
action-save = 保存
action-save-as = 另存为…
action-save-all = 全部保存
action-reload = 重新加载
action-close-tab = 关闭标签
action-close-others = 关闭其他
action-exit = 退出
action-undo = 撤销
action-redo = 重做
action-find = 查找
action-format = 格式化文档
action-xpath = XPath 查询…
action-validate-with = 用 Schema 验证…
action-diff = 与文件比较…
xpath-result-nodes = { $count } 个节点
action-validate = 验证…
action-about = 关于
action-exit-cancel = 取消

panel-outline = 大纲
panel-search = 搜索
panel-inspector = 检查器
panel-problems = 问题
panel-tasks = 任务
panel-source = 源码
panel-diff = 差异
panel-exi = EXI 结果
panel-empty = 未打开文档
welcome-title = 欢迎使用 XML Tool
welcome-hint = 打开一个 XML 文件，或新建文档开始编辑。

tab-untitled = 未命名-{ $number }
tab-dirty = { $name } ●
tab-close = 关闭标签页

toolbar-open-file = 打开文件
toolbar-save-file = 保存当前文件
toolbar-new-file = 新建文档
toolbar-format = 格式化文档
toolbar-show-outline = 显示大纲面板
toolbar-show-inspector = 显示检查器面板

outline-expand-all = 全部展开
outline-collapse-all = 全部折叠
outline-duplicate = 重复节点
outline-delete = 删除节点
outline-empty = 暂无内容
outline-copy-xpath = 复制 XPath
outline-copy-xml = 复制 XML 片段

inspector-qname = 名称
inspector-namespace = 命名空间 URI
inspector-attributes = 属性
inspector-add-attribute = 添加属性
inspector-remove-attribute = 移除属性
inspector-text = 文本
inspector-cdata = CDATA
inspector-comment = 注释
inspector-pi-target = 目标
inspector-pi-data = 数据
inspector-no-selection = 在大纲中选择一个节点
inspector-read-only = 只读模式：大文档不可编辑

problems-empty = 没有问题
toolbar-undo = 撤销
toolbar-redo = 重做
problems-filter-errors = 错误
problems-filter-warnings = 警告
problems-filter-infos = 信息
problems-filtered-empty = 告警已全部被过滤
problems-clear = 全部清除
problems-at-position = 第 { $line } 行，第 { $column } 列
problems-jump = 点击跳转到源码位置
source-jump-hint = 跳转目标：第 { $line } 行，第 { $column } 列
problems-count = { $count ->
    [other] { $count } 个问题
   *[other] { $count } 个问题
}

status-ready = 就绪
status-read-only = 只读
status-edited = 已修改
status-clean = 无更改
status-position = 第 { $line } 行，第 { $column } 列
status-encoding = 编码：{ $name }
status-elements = { $count } 个元素

dialog-delete-title = 删除节点？
dialog-delete-body = 删除 { $name } 及其 { $descendants ->
    [other] { $descendants } 个后代？
   *[other] { $descendants } 个后代？
}？此操作可以撤销。
dialog-confirm = 删除
dialog-cancel = 取消
dialog-run = 运行

dialog-about-title = 关于 { $name }
dialog-about-version = 版本 { $version }
dialog-about-backends = XML 后端：{ $xml } · EXI 后端：{ $exi }
dialog-about-license = 许可证：{ $license }

dialog-unsaved-title = 未保存的更改
dialog-unsaved-body = { $count } 个标签有未保存的更改。
dialog-unsaved-save-selected = 保存所选
dialog-unsaved-discard-selected = 丢弃所选
dialog-unsaved-cancel = 取消

dialog-reload-title = 文件已在磁盘上更改
dialog-reload-clean-body = { $name } 已在磁盘上更改。重新加载吗？
dialog-reload-dirty-body = { $name } 已在磁盘上更改，且有未保存的编辑。
dialog-reload-compare = 比较
dialog-reload-reload = 重新加载
dialog-reload-keep = 保留我的版本

dialog-recovery-title = 恢复文档？
dialog-recovery-body = 上次会话结束后恢复了 { $count } 个文档。
dialog-recovery-open = 恢复
dialog-recovery-discard = 丢弃

source-apply = 应用源码
source-discard-draft = 丢弃草稿
source-draft-active = 草稿包含未应用的更改
source-jump-line = 跳转到行

exi-open-workbench = 打开 EXI 工作台
exi-dialog-title = EXI 工作台
exi-encode = 编码当前文档
exi-decode = 解码 EXI 文件…
exi-report = {preset} · {input} B → {output} B（{percent}%）· {ms} ms
exi-preset-fidelity = 保真 Bit-Packed
exi-preset-byte = 字节对齐
exi-preset-precompression = 预压缩
exi-preset-max = 最大压缩
exi-fidelity-warning = 当前选项将丢弃：{items}

search-placeholder = 搜索…
search-hits = { $count } 个结果
search-replace-placeholder = 替换为…
search-replace-all = 全部替换
search-no-hits = 无匹配
search-case-sensitive = 区分大小写

theme-system = 跟随系统
theme-light = 浅色
theme-dark = 深色
font-scale = 字体大小：{ $percent }%

view-theme = 主题
view-font-scale = 字体缩放
view-language = 语言
language-english = English
language-chinese = 简体中文

readonly-reason = 文档超过编辑阈值（20 MiB 或 200,000 个元素），已以只读模式打开。

shortcut-help = 键盘快捷键
shortcut-new = 新建文档
shortcut-open = 打开文档
shortcut-save = 保存文档
shortcut-save-as = 另存为
shortcut-close = 关闭标签
shortcut-undo = 撤销
shortcut-redo = 重做
shortcut-find = 查找
shortcut-replace = 替换
shortcut-next-match = 下一个结果
shortcut-prev-match = 上一个结果
shortcut-cycle-focus = 切换焦点
shortcut-help-key = 快捷键帮助
shortcut-tree-nav = 树：方向键导航，Enter/右箭头展开，左箭头折叠，空格切换

error-parse-failed = 文档解析失败
error-io = 文件错误：{ $message }
error-too-large = 文件超过 256 MiB 打开上限
error-readonly-edit = 此文档为只读

# Icon conversion and import
icon-title = 图标转换器
icon-import = 导入图标…
icon-drop-hint = 选择图片，或将单个图片文件拖入此窗口。
icon-choose = 选择图片…
icon-formats = PNG · JPEG · GIF · WebP · BMP · ICO | 最大 8 MiB
icon-output = 输出格式：
icon-base64 = Base64
icon-data-uri = Data URI
icon-esi = ESI 图标（十六进制）
icon-esi-hint = 生成 16×14、4bpp BMP，等比缩放并居中留透明边；洋红色（#FF00FF）表示透明。
icon-original-hint = 编码原始文件字节，不改变图片内容。
icon-file-size = 文件大小：{ $size }
icon-loading = 正在读取并转换图片…
icon-saving = 正在保存编码文本…
icon-error = 图片操作失败：{ $message }
icon-summary = { $format } | { $width }×{ $height } | { $count } 字符
icon-frame-note = 预览展示解码器选取的默认帧或图标，编码保留完整原始文件。
icon-encoded = 编码文本
icon-excerpt = 仅展示前 4096 个字符；复制和保存包含完整结果。
icon-copy = 复制
icon-save = 保存文本…
icon-requires-esi = ImageData16x14 必须使用 ESI 图标（十六进制）输出。
icon-replace-draft = 这将替换尚未应用的图标文本草稿，保留其他属性。
icon-target = 目标：<{ $name }>
icon-fill = 填入文本草稿
icon-review = 请在属性检查器中预览，再应用更改或重置。
icon-no-target = 导入时请选择简单叶子元素或其文本/CDATA 节点，然后在属性检查器中点击“导入图标”。
icon-target-changed = 目标或文档已改变。请在所需节点上重新打开“导入图标”；仍可复制或保存转换结果。
icon-copied = 已复制完整编码文本。
icon-save-cancelled = 已取消保存文本。
icon-saved = 已保存编码文本：{ $path }
icon-drop-one = 每次请只拖入一张图片。
icon-drop-invalid = 拖入的项目没有可读取的图片数据。
icon-already-compliant = 原图已符合 16×14、4bpp BMP 要求，保留原始字节。
icon-converted = 已将 { $format } { $width }×{ $height }{ $depth } 转为 16×14、4bpp BMP，预览展示最终的最多 16 色图标。
icon-single-frame = ESI 使用解码器选取的默认帧或图标。
icon-preview = 图片预览
icon-preview-loading = 正在加载图片预览…
icon-preview-draft = 正在预览尚未应用的图标文本
icon-apply = 应用更改
icon-reset = 重置
