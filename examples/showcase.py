"""全场景演示：用 cslice.viz 渲染 all_in_one.c 的切分结果。

文本视图已产品化为 `cslice.viz` 与 `python -m cslice` CLI——本脚本退化为
调用 viz 的薄示例，展示"切分 → 渲染 → 草稿"的最小集成方式。

用法：在项目根目录执行 `python examples/showcase.py`
（需先 `maturin develop` 或 `pip install cslice`）。
"""

from pathlib import Path

import cslice
from cslice import viz
from cslice.drafts import generate_drafts

SRC = Path(__file__).with_name("all_in_one.c")


def main() -> None:
    source = SRC.read_text(encoding="utf-8")
    plans = cslice.slice_all(source)
    print(f"解析到 {len(plans)} 个函数\n")
    for plan in plans:
        drafts = generate_drafts(plan.function.name, plan.items, language="zh")
        print(viz.render_plan(source, plan, drafts=drafts))
        print()


if __name__ == "__main__":
    main()
