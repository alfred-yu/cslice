# cslice — C 函数 AST 逻辑块切分

[![CI](https://github.com/alfred-yu/cslice/actions/workflows/ci.yml/badge.svg)](https://github.com/alfred-yu/cslice/actions/workflows/ci.yml)
[![PyPI](https://img.shields.io/pypi/v/cslice)](https://pypi.org/project/cslice/)
[![Python](https://img.shields.io/pypi/pyversions/cslice)](https://pypi.org/project/cslice/)

[English](https://github.com/alfred-yu/cslice/blob/main/README.md) | **中文**

`cslice` 基于 [tree-sitter](https://tree-sitter.github.io/) 解析 C 源码，把每个函数体切分为**逻辑块**——构成函数行为的原子单元：分支、循环、case、连续简单语句、条件编译块。每个切片携带行范围、块类型、代码文本与语义摘要（条件表达式、循环控制、赋值/调用/返回），可直接用于代码理解、审查清单、覆盖映射、静态分析工具、文档生成等场景。

核心为纯 Rust 实现（tree-sitter），经 PyO3 编译为 Python 原生扩展——Python 侧零解析逻辑，无外部工具依赖。

## 安装

```bash
pip install cslice
```

从源码构建（需要 Rust 工具链）：

```bash
pip install maturin
maturin develop            # 在虚拟环境中、于项目根目录执行
```

## 快速开始

```python
import cslice

source = open("driver.c").read()

# 1) 一次调用：切分文件中的全部函数
for plan in cslice.slice_all(source):
    print(plan.function.name, plan.function.signature)
    for item in plan.items:
        print(f"  L{item.start_line}-L{item.end_line} [{item.kind}]")
        print(f"    {item.code_text!r}")

# 2) 或逐函数处理
funcs = cslice.parse_functions(source)
items = cslice.plan_function_slices(source, funcs[0].start_line, funcs[0].end_line)
```

含循环与嵌套分支的函数的输出示例：

```
Crc32 L1-L21 [static uint32_t Crc32(...)]
  L3-L3   [computation]  crc = 0xFFFFFFFFu;
  L5-L5   [loop]         for (i = 0u; i < len; i++)
  L7-L7   [computation]  crc ^= data[i];
  L9-L9   [loop]         for (k = 0; k < 8; k++)
  L11-L14 [branch]       if ((crc & 1u) != 0u) ...
  L15-L18 [branch]       else ...
  L21-L21 [computation]  return ~crc;
```

## API

| 函数 | 说明 |
|---|---|
| `parse_functions(source) -> list[FunctionDef]` | 按出现顺序解析全部函数定义（名称、签名、1-based 行范围）；语法错误时容错解析。 |
| `plan_function_slices(source, start_line, end_line) -> list[SlicePlanItem] \| None` | 对行范围精确匹配的函数生成切片计划；无匹配返回 `None`。 |
| `slice_all(source) -> list[FunctionPlan]` | 便捷入口：切分全部已解析函数。 |
| `extract_lines(source, start_line, end_line) -> str` | 提取 1-based 含端点行范围的代码文本（去除行尾 `\r`，不含结尾换行）；越界抛 `ValueError`。 |

切片类型（`cslice.KINDS`）：

| kind | 覆盖范围 |
|---|---|
| `computation` | 连续简单语句组（赋值 / 调用 / return）、带初始化的声明 |
| `branch` | `if` / `else-if` / `else` 逐分支各一片 |
| `loop` | 仅循环**头**——迭代控制独立成片 |
| `case` | `switch` 的每个 case / default 各一片（switch 头并入首片） |
| `preproc` | 条件编译块整体一片 |

`SlicePlanItem.summary`（`BlockSummary`）携带从 AST 提取的事实：`condition`、`is_else`、`case_value`、`loop_header`、`loop_init/cond/update`、`preproc_directive`、`parent_loop_cond`（嵌套片携带父循环条件），以及按源码顺序排列的行为清单 `behaviors`（`init`、`assign`、`compound_assign`、`return`、`call`、`break`、`continue`）。

切片构成一棵**深度优先森林**：每个 `SlicePlanItem` 还携带 `id`（即列表下标）、`parent_id`（顶层片为 `None`，否则为包裹本片的循环头片 id）和 `depth`（顶层 0，进入循环体 +1）。输出顺序从顶向下、从外到内：父片必先于其全部子片出现，因此 `items[item.parent_id]` 加上按 `depth` 缩进即可直接重建包含树。

切片还携带**执行前置守卫条件**：`guard_conds` 按序累积本片之前卫语句的放行条件——卫语句指真/假侧之一恒提前退出（return / break / continue，由内置控制流图判定）的 `if`。在

```c
if (p == 0 || n <= 0) { log("bad"); return -1; }
```

之后，同作用域的所有切片携带 `["!(p == 0 || n <= 0)"]`（后续循环体内的切片同样继承）；假侧恒退出的卫语句（else 内 return）则给出正向条件。守卫在语句作用域内累积、不越过循环体边界；它是**必要**执行前提，不是完整路径条件。

CFG 本身也已暴露：`cslice.build_cfg(source, start_line, end_line)` 返回 `CfgGraph`（节点带语义标签——条件表达式、"返回 x"、"调用 f()"——边带 是/否/直落/循环/break/continue/退出 标签及 `style` 视觉分类）。

## 切分规则

- **精确行为边界**：签名行、大括号、注释、纯声明不归属任何切片；顶层 `return` 始终独立成片（数据准备与可观测结果分开）。
- **原子性**：循环体内的嵌套控制流递归拆分——循环头、嵌套的分支/循环/switch 以及嵌套块之间的语句各自独立成片；嵌套切片在 `summary.parent_loop_cond` 中携带父循环条件。
- 带初始化的声明独立成片；纯声明跳过（无行为可描述）。
- 空函数体与无法解析的函数退化为单个兜底切片。

## 需求草稿生成（可选）

`cslice.drafts` 在切分之上提供需求草稿生成——面向需求工程场景的可选能力，只需要代码切分的用户可完全忽略本模块。

完全离线、确定性：每个切片经内置模板生成一条草稿句（中文"应"句式 / 英文 shall 句式），验证方法按切片类型推断（条件编译 → 审查，其余 → 测试）。

```python
from cslice.drafts import generate_draft, generate_drafts

funcs = cslice.parse_functions(source)
items = cslice.plan_function_slices(source, funcs[0].start_line, funcs[0].end_line)

draft = generate_draft(funcs[0].name, items[0])                     # 英文（默认）
draft_zh = generate_draft(funcs[0].name, items[0], language="zh")
drafts = generate_drafts(funcs[0].name, items)                      # 批量
```

`calc` 函数中 `int y = 0;` 的输出示例：

- 默认（英文）：`The calc function shall initialize the local variable y as 0.`（验证方法：`Test`）
- 中文（`language="zh"`）：`函数 calc 应将局部变量 y 初始化为 0。`（验证方法：`测试`）

嵌套循环内的切片自动携带执行语境：`for (i = 0; i < len; i++)` 内语句的草稿以"在该循环（i < len）的每次迭代中，…"（英文 "In each iteration of the loop where i < len holds, …"）为前缀。守卫条件（`guard_conds`）以最外层前缀体现——"当 !(p == 0) 时，" / "When !(p == 0), "。

草稿可用 `cslice.drafts.lint_requirement(description, verify_method, func_name)` 做合规检查——"应/shall"句式、函数名主语、歧义词、空验证方法与分句数启发（`lint_summarize` 按级别计数）。

不含 LLM、无网络调用：AI 增强属调用方职责。

## 可视化（可选）

`cslice.viz` 把切分结果渲染给人看——纯标准库、零第三方依赖。

**文本视图**（逐行归属标注、切片树、含守卫的明细、可选草稿）：

```bash
python -m cslice driver.c                    # 归属标注 + 树 + 明细
python -m cslice driver.c --drafts --lang zh # 附带中文需求草稿
python -m cslice driver.c --out report.md    # 写入文件
```

**Mermaid 导出**（粘进 GitHub / VSCode / Obsidian 即可原生渲染）：

```bash
python -m cslice driver.c --cfg-mermaid      # 每个函数的 CFG 流程图
python -m cslice driver.c --forest-mermaid   # 切片森林包含树
```

编程接口：`viz.annotated_source`、`viz.slice_tree`、`viz.slice_details`、
`viz.render_plan` / `render_source`（可选草稿）、`viz.cfg_to_mermaid`、
`viz.forest_to_mermaid`。

## 许可证

MIT
