# cslice — C 函数 AST 逻辑块切分

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

## 切分规则

- **精确行为边界**：签名行、大括号、注释、纯声明不归属任何切片；顶层 `return` 始终独立成片（数据准备与可观测结果分开）。
- **原子性**：循环体内的嵌套控制流递归拆分——循环头、嵌套的分支/循环/switch 以及嵌套块之间的语句各自独立成片；嵌套切片在 `summary.parent_loop_cond` 中携带父循环条件。
- 带初始化的声明独立成片；纯声明跳过（无行为可描述）。
- 空函数体与无法解析的函数退化为单个兜底切片。

## 许可证

MIT
