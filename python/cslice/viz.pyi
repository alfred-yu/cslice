from typing import Dict, List, Optional

from cslice import CfgGraph, FunctionPlan, SlicePlanItem
from cslice.drafts import ReqDraft


def _head(text: str, limit: int = 56) -> str: ...


def annotated_source(source: str, plan: FunctionPlan) -> str:
    """逐行归属标注：每行前缀其归属切片 id（'.' = 不归属任何片）。"""
    ...


def slice_tree(plan: FunctionPlan) -> str:
    """切片树：缩进 = 嵌套深度（深度优先，父片先于子片出现）。"""
    ...


def slice_details(plan: FunctionPlan) -> str:
    """切片明细：树位置、行范围、类型、语义摘要事实与守卫条件。"""
    ...


def render_plan(
    source: str, plan: FunctionPlan, drafts: Optional[List[ReqDraft]] = None
) -> str:
    """单函数完整文本视图：逐行归属 + 切片树 + 明细（+ 可选草稿节）。"""
    ...


def render_source(
    source: str,
    plans: List[FunctionPlan],
    drafts: Optional[Dict[str, List[ReqDraft]]] = None,
) -> str:
    """多函数文本视图。drafts: {函数名: 草稿列表}（可选）。"""
    ...


def cfg_to_mermaid(graph: CfgGraph, *, direction: str = "TB") -> str:
    """CFG → Mermaid flowchart（节点按类型着色，假边虚线、回边加粗）。"""
    ...


def forest_to_mermaid(plan: FunctionPlan, *, direction: str = "TB") -> str:
    """切片森林 → Mermaid flowchart（parent → child 包含关系）。"""
    ...
