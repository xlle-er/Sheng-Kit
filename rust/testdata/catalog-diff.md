# 语言集合与 golden 的差异

`golden/profile-capabilities.json` 记录的是旧引擎的语言集合。新集合依据厂商公开文档重建（见
`rust/sheng-runtime/src/catalog/languages/` 下各文件中每个集合的注释）。下表逐条列出与 golden 不同的语言码，
`rust/sheng-runtime/tests/golden_catalog.rs` 会对每个 golden 用例重新计算差异并核对本表及 `catalog-diff/*.md`。

- “范围”是造成差异的集合：provider id（识别/同传语言）或文本翻译类型（`deepL`、`deepLX`）。
  识别路由在翻译时取“识别语言 ∩ 翻译语言”，所以一个集合的变化会出现在多个组合中，这里只记一次。
- 列表顺序统一按语言目录的显示顺序输出，顺序不同不算差异。
- 能力查询的 `issues` 与 golden 全部一致；校验矩阵 528 例、负例 21 例的结果码全部一致。

## 未改动、沿用 golden 的集合

| 集合 | 原因 |
|---|---|
| Apple 识别 / Apple 翻译 | 实际可用语言由本机 `SpeechTranscriber`/`Translation` 在运行时报告，目录只列可映射的语言 |
| 阿里云识别、Qwen-MT、阿里云同传 | 阿里云文档页为动态渲染，离线无法提取语言表；待人工核对 |
| OpenAI / Azure 转写与同传 | OpenAI 文档只说明语言码格式，未给出逐语言清单；`ja` 不在集合中是校验矩阵要求的结果，待确认 |
| Gemini 同传目标语言 | 同传模型的目标语言未在公开文档列出 |
| 腾讯云 | 文档页无法离线读取；`en`/`ja` 之外的源语言目标表为推定，见 questions.md |
| 火山引擎、百度、xAI（STT 与 voice）、DeepL | 与文档一致，无差异（火山的 `zh_en` 来源见下） |

## 逐条差异

| 范围 | 列表 | 语言码 | 变化 | 依据 |
|---|---|---|---|---|
| `deepL` | targets | `es-ES` | 新增 | DeepL supported-languages 列出目标变体 `ES-ES`（developers.deepl.com/docs/getting-started/supported-languages） |
| `deepLX` | sources | `ace` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `af` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `an` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `as` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `ay` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `az` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `ba` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `be` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `bho` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `bn` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `br` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `bs` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `ca` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `ceb` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `ckb` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `cy` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `en-GB` | 删除 | DeepL 文档中地区/书写变体只能作为目标语言，不能作为源语言 |
| `deepLX` | sources | `en-US` | 删除 | DeepL 文档中地区/书写变体只能作为目标语言，不能作为源语言 |
| `deepLX` | sources | `eo` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `es-419` | 删除 | DeepL 文档中地区/书写变体只能作为目标语言，不能作为源语言 |
| `deepLX` | sources | `eu` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `fa` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `ga` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `gl` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `gn` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `gom` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `gu` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `ha` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `hi` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `hr` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `ht` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `hy` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `ig` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `is` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `jv` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `ka` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `kk` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `kmr` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `ky` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `la` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `lb` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `lmo` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `ln` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `mai` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `mg` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `mi` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `mk` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `ml` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `mn` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `mr` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `ms` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `mt` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `my` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `ne` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `oc` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `om` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `pa` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `pag` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `pam` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `prs` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `ps` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `pt-BR` | 删除 | DeepL 文档中地区/书写变体只能作为目标语言，不能作为源语言 |
| `deepLX` | sources | `pt-PT` | 删除 | DeepL 文档中地区/书写变体只能作为目标语言，不能作为源语言 |
| `deepLX` | sources | `qu` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `sa` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `scn` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `sq` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `sr` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `st` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `su` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `sw` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `ta` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `te` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `tg` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `th` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `tk` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `tl` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `tn` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `ts` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `tt` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `ur` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `uz` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `wo` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `xh` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `yi` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `yue` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | sources | `zh_tw` | 删除 | DeepL 文档中地区/书写变体只能作为目标语言，不能作为源语言 |
| `deepLX` | sources | `zu` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该源语言 |
| `deepLX` | targets | `ace` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `af` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `an` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `as` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `ay` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `az` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `ba` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `be` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `bho` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `bn` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `br` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `bs` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `ca` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `ceb` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `ckb` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `cy` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `eo` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `es-ES` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `eu` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `fa` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `ga` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `gl` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `gn` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `gom` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `gu` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `ha` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `hi` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `hr` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `ht` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `hy` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `ig` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `is` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `jv` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `ka` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `kk` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `kmr` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `ky` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `la` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `lb` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `lmo` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `ln` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `mai` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `mg` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `mi` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `mk` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `ml` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `mn` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `mr` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `ms` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `mt` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `my` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `ne` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `oc` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `om` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `pa` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `pag` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `pam` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `prs` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `ps` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `qu` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `sa` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `scn` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `sq` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `sr` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `st` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `su` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `sw` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `ta` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `te` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `tg` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `th` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `tk` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `tl` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `tn` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `ts` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `tt` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `ur` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `uz` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `wo` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `xh` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `yi` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `yue` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `deepLX` | targets | `zu` | 新增 | providers/text.md：DeepLX 语言列表与 DeepL 相同；DeepL 文档列出该目标语言 |
| `googleGeminiLive` | sources | `ak` | 新增 | Gemini Live API “Supported languages”列出该语言（ai.google.dev/gemini-api/docs/live-guide） |
| `googleGeminiLive` | sources | `ar-EG` | 删除 | Gemini Live API “Supported languages”未列出该语言（只列基础语言或未收录） |
| `googleGeminiLive` | sources | `cy` | 新增 | Gemini Live API “Supported languages”列出该语言（ai.google.dev/gemini-api/docs/live-guide） |
| `googleGeminiLive` | sources | `en-GB` | 删除 | Gemini Live API “Supported languages”未列出该语言（只列基础语言或未收录） |
| `googleGeminiLive` | sources | `en-US` | 删除 | Gemini Live API “Supported languages”未列出该语言（只列基础语言或未收录） |
| `googleGeminiLive` | sources | `es-419` | 删除 | Gemini Live API “Supported languages”未列出该语言（只列基础语言或未收录） |
| `googleGeminiLive` | sources | `eu` | 新增 | Gemini Live API “Supported languages”列出该语言（ai.google.dev/gemini-api/docs/live-guide） |
| `googleGeminiLive` | sources | `ga` | 新增 | Gemini Live API “Supported languages”列出该语言（ai.google.dev/gemini-api/docs/live-guide） |
| `googleGeminiLive` | sources | `jv` | 删除 | Gemini Live API “Supported languages”未列出该语言（只列基础语言或未收录） |
| `googleGeminiLive` | sources | `ln` | 删除 | Gemini Live API “Supported languages”未列出该语言（只列基础语言或未收录） |
| `googleGeminiLive` | sources | `lo` | 新增 | Gemini Live API “Supported languages”列出该语言（ai.google.dev/gemini-api/docs/live-guide） |
| `googleGeminiLive` | sources | `mi` | 新增 | Gemini Live API “Supported languages”列出该语言（ai.google.dev/gemini-api/docs/live-guide） |
| `googleGeminiLive` | sources | `om` | 新增 | Gemini Live API “Supported languages”列出该语言（ai.google.dev/gemini-api/docs/live-guide） |
| `googleGeminiLive` | sources | `ps` | 新增 | Gemini Live API “Supported languages”列出该语言（ai.google.dev/gemini-api/docs/live-guide） |
| `googleGeminiLive` | sources | `qu` | 新增 | Gemini Live API “Supported languages”列出该语言（ai.google.dev/gemini-api/docs/live-guide） |
| `googleGeminiLive` | sources | `rw` | 新增 | Gemini Live API “Supported languages”列出该语言（ai.google.dev/gemini-api/docs/live-guide） |
| `googleGeminiLive` | sources | `si` | 新增 | Gemini Live API “Supported languages”列出该语言（ai.google.dev/gemini-api/docs/live-guide） |
| `googleGeminiLive` | sources | `sq` | 新增 | Gemini Live API “Supported languages”列出该语言（ai.google.dev/gemini-api/docs/live-guide） |
| `googleGeminiLive` | sources | `st` | 新增 | Gemini Live API “Supported languages”列出该语言（ai.google.dev/gemini-api/docs/live-guide） |
| `googleGeminiLive` | sources | `ta` | 新增 | Gemini Live API “Supported languages”列出该语言（ai.google.dev/gemini-api/docs/live-guide） |
| `googleGeminiLive` | sources | `tk` | 新增 | Gemini Live API “Supported languages”列出该语言（ai.google.dev/gemini-api/docs/live-guide） |
| `googleGeminiLive` | sources | `tn` | 新增 | Gemini Live API “Supported languages”列出该语言（ai.google.dev/gemini-api/docs/live-guide） |
| `googleGeminiLive` | sources | `ur` | 新增 | Gemini Live API “Supported languages”列出该语言（ai.google.dev/gemini-api/docs/live-guide） |
| `googleGeminiLive` | sources | `wo` | 新增 | Gemini Live API “Supported languages”列出该语言（ai.google.dev/gemini-api/docs/live-guide） |
| `googleGeminiLive` | sources | `yue` | 删除 | Gemini Live API “Supported languages”未列出该语言（只列基础语言或未收录） |
| `googleGeminiLive` | sources | `zh_tw` | 新增 | Gemini Live API “Supported languages”列出该语言（ai.google.dev/gemini-api/docs/live-guide） |
| `googleGeminiLive` | sources | `zu` | 新增 | Gemini Live API “Supported languages”列出该语言（ai.google.dev/gemini-api/docs/live-guide） |
| `tencentCloud` | targets | `ru` | 新增 | 仅 source 不受支持时的回退目标列表（各源语言目标的并集）；腾讯云文档无法离线核对，待确认 |
| `tencentCloud` | targets | `zh_en` | 删除 | 仅 source 不受支持时的回退目标列表（各源语言目标的并集）；腾讯云文档无法离线核对，待确认 |
| `volcanoEngine` | targets | `zh_en` | 删除 | 仅 source 不受支持时的回退目标列表；火山 S2T 文档的目标语种不含中英混说 |
