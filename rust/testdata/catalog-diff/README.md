# 步骤 4 子步骤的语言差异

每个子步骤在本目录新建 `step4<字母>.md`，记录本子步骤修改语言集合后与 golden 的差异，不改上一级的 `catalog-diff.md`。

`rust/sheng-runtime/tests/golden_catalog.rs` 读取 `catalog-diff.md` 加上本目录所有 `*.md` 文件中的表格行。
行格式与 `catalog-diff.md` 的“逐条差异”表相同：`| 范围 | 列表 | 语言码 | 变化 | 依据 |`，变化只能是“新增”或“删除”。
本说明文件不含表格行，不影响测试。
