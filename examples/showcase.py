"""打印 cslice 对全场景演示函数的切分效果。

用法：在项目根目录执行 `python examples/showcase.py`
（需先 `maturin develop` 或 `pip install cslice`）。
"""
from pathlib import Path

import cslice

SRC = Path(__file__).with_name("all_in_one.c")


def main() -> None:
    source = SRC.read_text(encoding="utf-8")
    plans = cslice.slice_all(source)
    print(f"解析到 {len(plans)} 个函数\n")
    for plan in plans:
        show(plan, source)


def show(plan: "cslice.FunctionPlan", source: str) -> None:
    f = plan.function
    print(f"函数 {f.name}：L{f.start_line}-L{f.end_line}，共 {len(plan.items)} 个切片")
    print(f"签名：{f.signature}\n")

    # 逐行标注：每个源码行归属哪个切片（'.' = 不归属任何片）
    lines = source.split("\n")
    owner: dict[int, int] = {}
    for item in plan.items:
        for ln in range(item.start_line, item.end_line + 1):
            owner[ln] = item.id
    width = len(str(f.end_line))
    for ln in range(f.start_line, f.end_line + 1):
        mark = str(owner.get(ln, ".")).rjust(2)
        print(f" {ln:>{width}} {mark} | {lines[ln - 1]}")

    # 切片树：缩进 = 嵌套深度（从顶向下、从外到内）
    print("\n—— 切片树（缩进 = 嵌套深度）——")
    for item in plan.items:
        indent = "│   " * item.depth
        head = item.code_text.split("\n")[0].strip()[:56] if item.code_text else ""
        print(f"{indent}#{item.id:<2} [{item.kind}] L{item.start_line}-L{item.end_line}  {head}")

    # 切片明细（含语义摘要）
    print("\n—— 切片明细 ——")
    for item in plan.items:
        s = item.summary
        facts = []
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
        parent = f"父片=#{item.parent_id}" if item.parent_id is not None else "顶层"
        print(
            f"片#{item.id:<2} (深度{item.depth}, {parent}) "
            f"L{item.start_line}-L{item.end_line} [{item.kind}] {'  '.join(facts)}"
        )


if __name__ == "__main__":
    main()
