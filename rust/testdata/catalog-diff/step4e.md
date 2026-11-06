# 步骤 4e 语言差异（Gemini Live）

本步骤没有修改 `catalog/languages/gemini.rs`，Gemini 的语言集合仍与 golden 一致，因此本文件没有差异行。

语言码只在发往服务时映射（`services/gemini/language.rs`）：`zh` → `zh-Hans`，`zh_tw` → `zh-Hant`，`tl` → `fil`，`pt` → `pt-BR`，
其余原样发送。

待实机验收：同传模型实际支持的目标语言范围。Live Translate 文档现已公布支持的目标语言表，可先与目录逐项比对，
确认后再在这里按 `| 范围 | 列表 | 语言码 | 变化 | 依据 |` 的格式补充差异行。
