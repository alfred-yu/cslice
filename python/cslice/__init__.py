"""cslice：C 函数逻辑块切分引擎（基于 tree-sitter 的 Rust 实现，PyO3 绑定）。

把 C 函数体按"可独立描述行为的逻辑块"切分为切片计划，每个切片携带
行范围、块类型与语义摘要，可用于代码理解、审查清单、覆盖映射等场景。
"""

from cslice._native import (
    KINDS,
    Behavior,
    BlockSummary,
    CfgEdge,
    CfgGraph,
    CfgNode,
    FunctionDef,
    FunctionPlan,
    SlicePlanItem,
    __version__,
    build_cfg,
    extract_lines,
    parse_functions,
    plan_function_slices,
    slice_all,
)

__all__ = [
    "Behavior",
    "BlockSummary",
    "CfgEdge",
    "CfgGraph",
    "CfgNode",
    "FunctionDef",
    "FunctionPlan",
    "KINDS",
    "SlicePlanItem",
    "__version__",
    "build_cfg",
    "extract_lines",
    "parse_functions",
    "plan_function_slices",
    "slice_all",
]
