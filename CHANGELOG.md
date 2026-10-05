# Changelog

记录用户可感知的变更。0.x 阶段以次版本位承载新增能力，补丁位承载修复。

## 0.1.5 (unreleased)

- Added: `cslice.viz` 可视化子模块与 `python -m cslice` CLI——文本视图（逐行归属标注、切片树、含守卫的明细、可选草稿节）与 Mermaid 导出（`cfg_to_mermaid` / `forest_to_mermaid`）；纯标准库零依赖
- Added: 需求草稿自动携带守卫语境——切片的 `guard_conds` 合取为 `!(a) && !(b)`，以 "当 … 时，"/"When …, " 前缀置于最外层（先于循环语境）
- Added: `cslice.drafts.lint_requirement` / `lint_summarize` 需求合规检查（"应/shall"句式、函数名主语、歧义词、空验证方法、原子性启发；warning + info 两级）
- Added: 自增/自减语句（`i++` / `--i`，独立语句形态）行为提取与自然语言句式——"increment/decrement the local variable {var} by 1" / "将局部变量 {var} 加 1/减 1"；嵌在更大表达式（赋值右侧、初始化值等）内部时不单列
- Added: case 分派条件并入结构化条件清单——case 取值渲染为 "{cond} is equal to {value}"，default 为 "{cond} does not match any case value"；不再使用句首 case 前缀
- Changed: break 短语措辞明确化——"terminate the innermost enclosing loop" / "终止所在（最内层）循环"；Branch 类切片放开 break/continue 短语（switch 体不拆分支片，语义安全），Case 类放开 continue（指向所在循环）
- Added: 结构化条件块——多条件、复合条件（含顶层 `||`/`&&`）或多行为清单时，条件以 "when:"/"当：" 引出编号清单：顶层项 `-AND-` 连接，复合项拆 1a/1b 子项加括号并以 `-AND-`/`-OR-` 连接（按括号深度 0 拆分，`!(…)` 原子不拆）；单一简单条件保持行内
- Added: 条件中的比较运算符自然语言化——`==`→is equal to/等于、`!=`→is not equal to/不等于、`>`→is greater than/大于、`<`→is less than/小于、`>=`→is greater than or equal to/大于等于、`<=`→is less than or equal to/小于等于；`>>`/`<<`（移位）与 `->` 不受影响，`&&`/`||` 保留符号
- Changed: 条件连接词语义化——单一简单条件行内用 `if`（"… shall X if {cond}."），多条件/复合条件的结构化 `when:` 块保持 `when`（"shall X when:"）
- Changed: if 分支条件后置——由句首 "When {condition}, " 改为句尾 "when {condition}."（zh "，当 {condition} 时"），多行为清单在 "in order"/"按顺序执行" 之后插入条件以覆盖全部行为；守卫语境（"When !(…)、"）与循环语境（"in each iteration … where …"）保持前导
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
