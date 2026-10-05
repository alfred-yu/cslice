# Changelog

记录用户可感知的变更。0.x 阶段以次版本位承载新增能力，补丁位承载修复。

## 0.1.5 (unreleased)

- Added: `cslice.viz` 可视化子模块与 `python -m cslice` CLI——文本视图（逐行归属标注、切片树、含守卫的明细、可选草稿节）与 Mermaid 导出（`cfg_to_mermaid` / `forest_to_mermaid`）；纯标准库零依赖
- Added: 需求草稿自动携带守卫语境——切片的 `guard_conds` 合取为 `!(a) && !(b)`，以 "当 … 时，"/"When …, " 前缀置于最外层（先于循环语境）
- Added: `cslice.drafts.lint_requirement` / `lint_summarize` 需求合规检查（"应/shall"句式、函数名主语、歧义词、空验证方法、原子性启发；warning + info 两级）
- Changed: `behavior.init` 句式对齐需求工程参考格式——"将局部变量 {var} 初始化为 {value}" / "initialize the local variable {var} as {value}"
- Changed: 移除 `BlockSummary.behavior_count` 死字段（Python 侧以 `len(behaviors)` 计算）
- Fixed: CFG 中 else 分支体被聚合成不透明基本块（else_clause 包装节点未解包），return/break 的提前退出语义丢失

## 0.1.4 (2026-10-04)

- Added: 切片树关系——`id` / `parent_id` / `depth`，深度优先输出（父片必先于子片，从顶向下、从外到内）
- Added: `cslice.drafts` 需求草稿生成子模块（默认英语，`language="zh"` 中文；黄金句与原实现逐字对拍）
- Added: `cslice.build_cfg` Python API 与 CFG 类型（`CfgGraph` / `CfgNode` / `CfgEdge`）
- Added: `SlicePlanItem.guard_conds` 执行前置守卫条件（基于 CFG 可达性的卫语句判定）

## 0.1.3 (2026-10-04)

- 首个完整发布版本：三平台 wheel + sdist（cp38-abi3，Python ≥3.8）
- 切分引擎：`parse_functions` / `plan_function_slices` / `slice_all` / `extract_lines`
- 五类切片：computation / branch / loop / case / preproc；精确行为边界与循环嵌套拆分

## 0.1.0 - 0.1.2 (2026-10-04)

- 首次发布排障过程中的过渡版本（各缺部分平台产物，已被 0.1.3 取代）
