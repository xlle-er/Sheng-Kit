# 步骤 4g 语言差异（腾讯云）

按腾讯云「实时语音翻译（WebSocket）」文档（<https://cloud.tencent.com/document/product/1093/127565>，2026-05-27）的
`source`/`target` 参数重建了 `catalog/languages/tencent.rs`。golden 的全部腾讯云用例（源 `en`、`zh`、`ja`、`auto`）与新集合**完全一致**，
本文件没有差异行。

合并者请注意：`catalog-diff.md` 中两行腾讯云差异（`tencentCloud | targets | ru | 新增` 与 `tencentCloud | targets | zh_en | 删除`）
描述的是旧推定实现，现已过时，可以删除（测试只检查未记录的差异，保留它们不会失败）。

不在 golden 中、按文档变化的组合（validate 结果会随之改变，golden 没有覆盖）：

- `zh`、`zh_en`、`yue` → `ru`：由允许变为不允许。
- `yue` → `id`、`th`：由允许变为不允许。
- `zh_en` → `zh_en`：新增允许。
