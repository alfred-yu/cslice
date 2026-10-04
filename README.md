# cslice — AST-based logical-block slicing for C functions

**English** | [中文](https://github.com/alfred-yu/cslice/blob/main/README.zh-CN.md)

`cslice` parses C source code with [tree-sitter](https://tree-sitter.github.io/)
and splits each function body into **logical blocks** — the atomic pieces of
behavior a function is made of: branches, loops, switch cases, runs of simple
statements, conditional-compilation blocks. Each slice carries its line range,
block kind, extracted code text, and a semantic summary (conditions, loop
control, assignments / calls / returns), ready for downstream use: code
comprehension, review checklists, coverage mapping, static-analysis tooling,
or documentation.

The heavy lifting is a pure-Rust core compiled to a native Python extension
(PyO3) — no Python-side parsing, no external tools.

## Install

```bash
pip install cslice
```

Build from source (requires Rust toolchain):

```bash
pip install maturin
maturin develop            # inside a virtualenv, from the project root
```

## Quick start

```python
import cslice

source = open("driver.c").read()

# 1) One call: plan every function in the file
for plan in cslice.slice_all(source):
    print(plan.function.name, plan.function.signature)
    for item in plan.items:
        print(f"  L{item.start_line}-L{item.end_line} [{item.kind}]")
        print(f"    {item.code_text!r}")

# 2) Or work function by function
funcs = cslice.parse_functions(source)
items = cslice.plan_function_slices(source, funcs[0].start_line, funcs[0].end_line)
```

Example output for a function containing a loop with nested branches:

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

| Function | Description |
|---|---|
| `parse_functions(source) -> list[FunctionDef]` | All function definitions in order (name, signature, 1-based line range). Fault-tolerant on syntax errors. |
| `plan_function_slices(source, start_line, end_line) -> list[SlicePlanItem] \| None` | Slice plan for the function whose range exactly matches; `None` if no match. |
| `slice_all(source) -> list[FunctionPlan]` | Convenience: plan every parsed function. |
| `extract_lines(source, start_line, end_line) -> str` | 1-based inclusive line snapshot (strips `\r`, no trailing newline). Raises `ValueError` out of range. |

Slice kinds (`cslice.KINDS`):

| kind | covers |
|---|---|
| `computation` | Runs of simple statements (assign / call / return), initialized declarations |
| `branch` | One `if` / `else-if` / `else` arm each |
| `loop` | The loop **header only** — iteration control is its own slice |
| `case` | One `switch` case / default each (switch head folds into the first case) |
| `preproc` | A conditional-compilation block as a whole |

`SlicePlanItem.summary` (a `BlockSummary`) carries the extracted facts:
`condition`, `is_else`, `case_value`, `loop_header`, `loop_init/cond/update`,
`preproc_directive`, `parent_loop_cond` (nested-slice context), and `behaviors` —
an ordered list of `Behavior` facts (`init`, `assign`, `compound_assign`,
`return`, `call`, `break`, `continue`).

Slices form a **depth-first forest**: each `SlicePlanItem` also carries `id`
(its index in the list), `parent_id` (`None` at function-body top level,
otherwise the id of the enclosing loop-header slice), and `depth` (0 at top
level, +1 inside a loop body). Items are emitted top-down, outer-before-inner:
a parent slice always appears before all of its children, so
`items[item.parent_id]` and an indentation by `depth` reconstruct the
containment tree directly.

## Slicing rules

- **Exact behavior boundaries**: signature lines, braces, comments and bare
  declarations belong to no slice; a top-level `return` is always its own slice
  (data preparation vs. observable result).
- **Atomicity**: nested control flow inside a loop body is split recursively —
  loop header, nested branches/loops/switches and the statements between them
  each get their own slice. Nested slices carry the parent loop condition in
  `summary.parent_loop_cond`.
- Initialized declarations are standalone slices; bare declarations are skipped
  (nothing to describe).
- Empty bodies and unparseable functions degrade to a single fallback slice.

## Draft generation (optional)

`cslice.drafts` adds requirement-draft generation on top of slicing — an
optional, domain-oriented capability (requirement engineering for C code);
users who only need slicing can ignore it entirely.

It is fully offline and deterministic: each slice is turned into a
template-based draft sentence (Chinese "应" style or English "shall" style),
with a verify method inferred from the slice kind (conditional compilation →
review, all others → test).

```python
from cslice.drafts import generate_draft, generate_drafts

funcs = cslice.parse_functions(source)
items = cslice.plan_function_slices(source, funcs[0].start_line, funcs[0].end_line)

draft = generate_draft(funcs[0].name, items[0])                     # en (default)
draft_zh = generate_draft(funcs[0].name, items[0], language="zh")
drafts = generate_drafts(funcs[0].name, items)                      # batch
```

Example output for `int y = 0;` inside `calc`:

- en (default): `The calc function shall initialize y to 0.` (verify method: `Test`)
- zh (`language="zh"`): `函数 calc 应将 y 初始化为 0。` (verify method: `测试`)

Slices inside nested loops carry their execution context: the draft for a
statement inside `for (i = 0; i < len; i++)` is prefixed with
"在该循环（i < len）的每次迭代中，…" / "In each iteration of the loop where
i < len holds, …".

No LLM, no network: `cslice.drafts` never makes external calls; AI-based
enhancement belongs to the caller.

## License

MIT
