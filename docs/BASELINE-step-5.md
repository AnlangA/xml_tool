# 阶段记录：第 5 步——专业 UI 与中英双语

- 完成日期：2026-08-16
- 前置：`docs/BASELINE-step-4.md`
- 测试总览：208 通过 / 0 失败 / 0 忽略（新增 shell 单元 5 项 + UI/快照 9 项；main_panel 的 4 项测试按等价语义迁移至 shell，原 93 项测试保持全数保留）

## 交付内容

| 文件 | 职责 |
|---|---|
| `src/ui/shell.rs`（780 行） | AppShell：菜单（File/Edit/Search/XML/EXI/View/Help）、工具栏、文档标签、后台任务轮询（打开/保存）、快捷键（Ctrl+N/O/S/Shift+S/W/Z/Y/F/H、F3/Shift+F3、F6、F1）、响应式布局（≥1100 内联 Inspector；≥850 内联 Outline；更窄为覆盖式抽屉）、New/Open/Recent 占位/Save/Save As/Save All/Reload/Close Tab/Close Others/Exit |
| `src/ui/panels.rs`（689 行） | 顶栏、标签栏、Outline（FlatTree + `show_rows` 固定行高虚拟化、搜索框、展开/折叠全部）、中央源码视图、Problems、状态栏 |
| `src/ui/inspector.rs`（276 行） | 检查器：QName/命名空间/属性增删改、Text/CDATA/Comment/PI 编辑、节点插入按钮、删除二次确认（显示 QName 与后代数量）——全部经 Command 提交 |
| `src/ui/dialogs.rs`（248 行） | About（版本/后端/许可证）、删除确认、退出未保存（Save/Discard/Cancel）、外部修改横幅（Reload/Compare/Keep）、崩溃恢复选择页、快捷键帮助 |
| `src/ui/localization.rs` | Fluent 双语运行时（en-US/zh-CN 编译内置），系统语言检测，缺键可见回退 |
| `src/ui/theme_prefs.rs` | System/Light/Dark 三态主题 + 80–180% 步进 10% 字体缩放，即时生效 |
| `src/ui/fonts.rs` + `assets/fonts/` | 内置 Noto Sans SC（8.3MB，SIL OFL 1.1 许可随附）+ Phosphor 图标字体，一次性安装 |
| `src/ui/icons.rs` | 纯 Phosphor 图标常量（移除 Emoji 混用）；`egui-phosphor =0.11.0` 与 egui 0.33.3 对齐（0.7 可选特性及旧版 egui 闭包随之移除） |
| `assets/i18n/*.ftl` | 中英资源文件，键集合测试强制一致 |
| 删除 | `main_panel.rs`（1,984 行）及孤儿模块 status_bar/search_bar/shortcuts_panel/loading_indicator/file_dialog |

依赖新增：`fluent-bundle 0.16`、`unic-langid`、`sys-locale`、`ropey`（第 4 步）、`egui-phosphor =0.11.0`、dev：`egui_kittest 0.33.3`（snapshot+wgpu）。

## 交互接线（相对第 3 步声明的兑现）

- 打开文档：后台任务读盘→分类→解析，UI 线程零 I/O；失败进 Problems 面板。
- 保存：后台任务（编码再编码 + 原子写盘）；保存启动时的 revision，完成后仅当 revision 未前进才置干净。
- 同路径重复打开聚焦；Untitled-N 递增；Save As 即时赋路径。
- 外部修改横幅三分支（干净 Reload/Ignore；脏 Compare/Reload/Keep）。
- 启动恢复选择页（存在快照时）。

## 验收门槛与证据

| 门槛 | 结果 |
|---|---|
| 中英文翻译键集合完全相同 | ✅ `locales_have_identical_key_sets`（HashSet 差集断言） |
| 12 组 UI 快照（3 尺寸 × 2 语言 × 2 主题） | ✅ `ui_renders_in_all_twelve_combinations`（kittest 像素快照，基线入库 `tests/snapshots/`；无渲染器环境自动降级为结构断言） |
| 800×500 无不可操作项 | ✅ `narrow_windows_keep_controls_in_bounds`（控件在界内、抽屉可达） |
| 中文 CJK 渲染 | ✅ `chinese_menu_renders_with_cjk_font`（内置 Noto Sans SC） |
| 键盘主流程 | ✅ `ctrl_n_creates_a_tab_keyboard_only`、`f1_opens_shortcut_help`、`menu_click_new_creates_a_document`（含指针点击流程） |
| 图标按钮本地化工具提示 | ✅ 工具栏四个 Phosphor 图标均带 `on_hover_text`（本地化） |
| `main_panel.rs` 删除，新 UI 文件 ≤800 行 | ✅ shell 780 / panels 689 / inspector 276 / dialogs 248 |
| fmt / clippy --all-features / test / bench / MSRV 1.88 | ✅ 208 测试全绿；`rustup run 1.88.0 cargo check` 通过 |

## 与计划的偏差记录

- Recent Files 菜单项：设置持久化（含最近文件）属第 10 步打包前的设置存储，当前会话内 Recent 入口未接数据源——第 6 步接线（无 TODO 注释残留在代码中）。
- Close Others/Reload 菜单项随标签操作落地（Close Others 在标签栏上下文；Reload 经外部修改横幅）。
- 快照像素比对在无 GPU/软渲染不可用的环境降级为结构断言（CI Linux 有软渲染时自动启用像素比对）。
