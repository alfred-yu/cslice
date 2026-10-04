"""cslice：C 函数逻辑块切分引擎（基于 tree-sitter 的 Rust 实现，PyO3 绑定）。

把 C 函数体按"可独立描述行为的逻辑块"切分为切片计划，每个切片携带
行范围、块类型与语义摘要，可用于代码分析、逆向需求生成等场景。
"""

from cslice._native import (
    KINDS,
    Behavior,
    BlockSummary,
    FunctionDef,
    FunctionPlan,
    SlicePlanItem,
    __version__,
    extract_lines,
    parse_functions,
    plan_function_slices,
    slice_all,
)

__all__ = [
    "Behavior",
    "BlockSummary",
    "FunctionDef",
    "FunctionPlan",
    "KINDS",
    "SlicePlanItem",
    "__version__",
    "extract_lines",
    "parse_functions",
    "plan_function_slices",
    "slice_all",
]
