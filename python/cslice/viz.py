"""cslice.viz：切片结果可视化（可选能力，纯标准库、零第三方依赖）。

两类输出：
- 文本视图：逐行归属标注、切片树（缩进 = 嵌套深度）、切片明细
  （含语义摘要与守卫条件），可选附带需求草稿节；
- Mermaid 图导出：CFG 流程图与切片森林包含树——GitHub / VSCode /
  Obsidian 等 Markdown 环境可直接渲染，本模块只负责生成文本。

命令行入口：`python -m cslice file.c [--drafts] [--lang en|zh]
[--cfg-mermaid] [--forest-mermaid] [--out FILE]`（见 cslice.__main__）。
本模块不依赖 cslice.drafts；草稿节由调用方传入。
"""

from __future__ import annotations

from typing import Dict, List, Optional

from cslice import CfgGraph, FunctionPlan, SlicePlanItem

__all__ = [
    "annotated_source",
    "cfg_to_mermaid",
    "forest_to_mermaid",
    "render_plan",
    "render_source",
    "slice_details",
    "slice_tree",
]

_KIND_WORDS = {
    "computation": "计算",
    "branch": "分支",
    "loop": "循环",
    "case": "分派",
    "preproc": "条件编译",
}


def _head(text: str, limit: int = 56) -> str:
    """源码首行压缩文本，超长截断。"""
    first = text.split("\n")[0].strip()
    if len(first) > limit:
        return first[:limit] + "…"
    return first


def annotated_source(source: str, plan: FunctionPlan) -> str:
    """逐行归属标注：每行前缀其归属切片 id（'.' = 不归属任何片，
    如签名行 / 大括号 / 注释 / 纯声明）。"""
    lines = source.split("\n")
    f = plan.function
    owner: Dict[int, int] = {}
    for item in plan.items:
        for ln in range(item.start_line, item.end_line + 1):
            owner[ln] = item.id
    width = len(str(f.end_line))
    out = [
        f"函数 {f.name}：L{f.start_line}-L{f.end_line}，{len(plan.items)} 个切片",
        f"签名：{f.signature}",
    ]
    for ln in range(f.start_line, f.end_line + 1):
        mark = str(owner.get(ln, ".")).rjust(2)
        text = lines[ln - 1] if ln - 1 < len(lines) else ""
        out.append(f" {ln:>{width}} {mark} | {text}")
    return "\n".join(out)


def slice_tree(plan: FunctionPlan) -> str:
    """切片树：缩进 = 嵌套深度（深度优先，父片先于子片出现）。"""
    out = ["—— 切片树（缩进 = 嵌套深度）——"]
    for item in plan.items:
        indent = "│   " * item.depth
        out.append(
            f"{indent}#{item.id:<2} [{item.kind}] "
            f"L{item.start_line}-L{item.end_line}  {_head(item.code_text)}"
        )
    return "\n".join(out)


def slice_details(plan: FunctionPlan) -> str:
    """切片明细：树位置、行范围、类型、语义摘要事实与守卫条件。"""
    out = ["—— 切片明细 ——"]
    for item in plan.items:
        s = item.summary
        facts: List[str] = []
        if s.condition:
            facts.append(f"条件={s.condition}")
        if s.case_value:
            facts.append(f"case值={s.case_value}")
        if s.is_else:
            facts.append("无条件分支")
        if s.loop_cond:
            facts.append(f"循环条件={s.loop_cond}")
        if s.preproc_directive:
            facts.append(f"指令={s.preproc_directive}")
        if s.behaviors:
            facts.append("行为=[" + ",".join(b.kind for b in s.behaviors) + "]")
        if item.guard_conds:
            facts.append("守卫=[" + " && ".join(item.guard_conds) + "]")
        parent = f"父片=#{item.parent_id}" if item.parent_id is not None else "顶层"
        out.append(
            f"片#{item.id:<2} (深度{item.depth}, {parent}) "
            f"L{item.start_line}-L{item.end_line} [{item.kind}] {'  '.join(facts)}"
        )
    return "\n".join(out)


def render_plan(
    source: str,
    plan: FunctionPlan,
    drafts: Optional[List[object]] = None,
) -> str:
    """单函数完整文本视图：逐行归属 + 切片树 + 明细（+ 可选草稿节）。

    drafts 为该函数的草稿列表（与 plan.items 等长，元素需有
    description / verify_method 属性，如 cslice.drafts.ReqDraft）。
    """
    sections = [annotated_source(source, plan), slice_tree(plan), slice_details(plan)]
    if drafts is not None:
        out = ["—— 切片需求草稿 ——"]
        for item, d in zip(plan.items, drafts):
            indent = "│   " * item.depth
            out.append(f"{indent}#{item.id:<2} [{d.verify_method}] {d.description}")
        sections.append("\n".join(out))
    return "\n\n".join(sections)


def render_source(
    source: str,
    plans: List[FunctionPlan],
    drafts: Optional[Dict[str, List[object]]] = None,
) -> str:
    """多函数文本视图。drafts: {函数名: 草稿列表}（可选）。"""
    sections = []
    for plan in plans:
        ds = drafts.get(plan.function.name) if drafts else None
        sections.append(render_plan(source, plan, ds))
    return "\n\n".join(sections)


def _mmd(text: str) -> str:
    """Mermaid 节点文本消毒：引号转义、换行折叠（引号内 `|` 可原样保留）。"""
    return text.replace('"', "&quot;").replace("\n", " ").strip()


def _mmd_label(text: str) -> str:
    """Mermaid 边标签消毒：边标签定界符是 `|`，竖线需替换。"""
    return _mmd(text).replace("|", "/")


def cfg_to_mermaid(graph: CfgGraph, *, direction: str = "TB") -> str:
    """CFG → Mermaid flowchart。节点按类型着色；条件不成立边虚线、
    循环回边/跳出加粗。渲染交给 Markdown 环境（GitHub / VSCode 等）。"""
    out = [f"flowchart {direction}"]
    for n in graph.nodes:
        out.append(f'    n{n.id}["{_mmd(n.label)}"]:::{n.kind}')
    false_idx: List[int] = []
    back_idx: List[int] = []
    for i, e in enumerate(graph.edges):
        label = f"|{_mmd_label(e.label)}|" if e.label else ""
        out.append(f"    n{e.src} -->{label} n{e.dst}")
        if e.style == "false":
            false_idx.append(i)
        elif e.style == "back":
            back_idx.append(i)
    out.append("    classDef entry fill:#c8e6c9,stroke:#2e7d32")
    out.append("    classDef exit fill:#ffcdd2,stroke:#c62828")
    out.append("    classDef return fill:#ffcdd2,stroke:#c62828")
    out.append("    classDef branch fill:#fff9c4,stroke:#f9a825")
    out.append("    classDef loop fill:#e1bee7,stroke:#6a1b9a")
    out.append("    classDef switch fill:#ffe0b2,stroke:#ef6c00")
    out.append("    classDef case fill:#ffe0b2,stroke:#ef6c00")
    out.append("    classDef join fill:#eceff1,stroke:#607d8b")
    out.append("    classDef block fill:#ffffff,stroke:#90a4ae")
    if false_idx:
        out.append(f"    linkStyle {','.join(map(str, false_idx))} stroke-dasharray:4 3")
    if back_idx:
        out.append(f"    linkStyle {','.join(map(str, back_idx))} stroke-width:3px")
    return "\n".join(out)


def forest_to_mermaid(plan: FunctionPlan, *, direction: str = "TB") -> str:
    """切片森林 → Mermaid flowchart：parent → child 包含关系，
    节点文本含类型与源码首行，守卫条件并入节点文本。"""
    out = [f"flowchart {direction}"]
    out.append(f'    subgraph fn["{plan.function.name}"]')
    for item in plan.items:
        guard = " / 守卫: " + " && ".join(item.guard_conds) if item.guard_conds else ""
        text = _mmd(f"#{item.id} {_KIND_WORDS.get(item.kind, item.kind)}: {_head(item.code_text, 40)}{guard}")
        out.append(f'        s{item.id}["{text}"]:::{item.kind}')
    for item in plan.items:
        if item.parent_id is not None:
            out.append(f"        s{item.parent_id} --> s{item.id}")
    out.append("    end")
    for kind, color, stroke in [
        ("computation", "#ffffff", "#90a4ae"),
        ("branch", "#fff9c4", "#f9a825"),
        ("loop", "#e1bee7", "#6a1b9a"),
        ("case", "#ffe0b2", "#ef6c00"),
        ("preproc", "#e0f7fa", "#00838f"),
    ]:
        out.append(f"    classDef {kind} fill:{color},stroke:{stroke}")
    return "\n".join(out)
