# 阶段记录：第 10 步——质量、安全、CI 和发布

- 完成日期：2026-08-16
- 前置：`docs/BASELINE-step-9.md`
- 版本：0.2.0 → **0.3.0**
- 测试总览：253 通过 / 0 失败 / 0 忽略（新增 `tests/property_fuzz_tests.rs` 10 项）

## 交付内容

| 项 | 说明 |
|---|---|
| 属性测试 | 200 种子随机合法树：解析/序列化/重解析/编辑+撤销结构等价；100 种子随机命令序列（重命名/属性/插入/删除/重复/格式化 × 立即撤销）零 panic 且源码始终可解析 |
| 稳健性（语料回归） | 五个入口全覆盖：XML 字节（含深度 100k 递归）、EXI 流（真实流多点变异+纯噪声）、XPath 表达式（片段拼接 300 组）、Schema 文件、Base64 图像数据——全部结构化错误零 panic；Billion-Laughs 变体有界；缓存会话翻转 |
| **属性测试揪出并修复 2 个真实 bug** | ① `splice_source` 从不调整"完整包含拼接区间的祖先"的范围 end（收缩后越界）② 结束点恰在拼接点的节点被错误平移（Duplicate+undo 丢失 `</root>`）。修复后 253 测试全绿 |
| 安全门 | `cargo audit` 通过（quick-xml 两条 advisory 经分析豁免：漏洞路径 NsReader 不可达——所有输入先经 uppsala 全量校验与预算，见 `.cargo/audit.toml`）；`cargo deny` 四项全绿（advisories/bans/licenses/sources）；erxi 依赖补 `rev` 固定 |
| CI 三平台矩阵 | Ubuntu 24.04 / Windows Server 2025 / macOS 15：fmt + clippy + 全测试 + release 构建；Linux 加基准构建冒烟；独立 audit/deny job；nightly fuzz 调度（30 分钟预算） |
| 打包 | `.github/workflows/package.yml` 手动触发：.deb/AppImage、MSI（WiX 阶段产物）、.app/.dmg；签名/公证需仓库密钥，按计划发布动作需单独授权 |
| 文档 | README 全面重写（每项功能标注对应自动化测试、性能包络、安全限制表、快捷键、许可）、CHANGELOG.md、MIGRATION.md |

## ⚠ 重大发现：erxi 许可证

cargo-deny 查出 erxi（EXI 后端）许可证为 **PolyForm-Noncommercial-1.0.0**。
本应用的二进制分发将因 EXI 功能继承非商业限制（XML 功能不受影响，MIT）。
已在 README" Licensing"、CHANGELOG、MIGRATION、deny.toml 中显著声明。
**商业发布需要替换 EXI 后端或获得商业授权——这是需要产品决策的事项。**

## 验收门槛与证据

| 门槛 | 结果 |
|---|---|
| fmt / 严格 Clippy / 全测试 / bench 构建 / audit / deny | ✅ 全部通过 |
| 属性测试（随机合法树 parse/serialize/undo） | ✅ `random_legal_trees_parse_serialize_and_undo`（200 种子） |
| 模糊测试各入口零 panic / 越界 / 死循环 | ✅ 六个稳健性测试（XML/EXI/XPath/XSD/Base64/递归/实体） |
| 三平台 CI | ✅ 矩阵配置完成（实际绿灯需推送到 GitHub Actions） |
| 安装/启动/保存/卸载三平台冒烟 | ⏳ 打包工作流手动触发后执行（需平台运行器与签名凭据） |
| 0.2.0 升级兼容 | ✅ MIGRATION.md 记录行为差异；旧 XML 文件完全兼容；无损坏的持久化设置（0.2 无设置存储） |
| README 每项功能有对应测试 | ✅ 每个功能条目标注测试文件 |
| 版本 0.3.0 + changelog | ✅ |
| 未自动提交标签/发布 | ✅ 仅本地提交；发布需单独授权 |

## 最终完成定义对照

- ✅ AGENTS_PLAN.md 与实现一致（各阶段文档记录偏差）
- ✅ P0 XML 保真与安全全部解决（第 1 步验收）
- ✅ 多标签、双向源码编辑、中英双语、XML 专业套件、EXI 工作台可用
- ✅ 20 MiB/200k 功能/响应/内存门槛（第 4 步实测 + 测试锁定）
- ✅ 超阈值只读、256 MiB 拒绝
- ✅ UI 无同步重型操作（打开/保存/Apply 全部后台任务化）
- ⏳ 三平台 CI 绿灯 / 安装冒烟：需推送后由 GitHub Actions 执行
- ✅ 原 93 测试全数保留（main_panel 4 项等价迁移）；新增测试无 ignored
- ✅ 无 TODO 残留、无空 About、无未接入性能组件（阶段文档记录的排序项除外）
