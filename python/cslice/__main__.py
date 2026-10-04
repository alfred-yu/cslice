"""cslice 命令行入口：可视化 C 源文件的切分结果。

用法：
    python -m cslice file.c                          # 文本视图（归属/树/明细）
    python -m cslice file.c --drafts --lang zh       # 附带中文需求草稿
    python -m cslice file.c --cfg-mermaid            # 追加 CFG 的 Mermaid 图
    python -m cslice file.c --forest-mermaid         # 追加切片森林的 Mermaid 图
    python -m cslice file.c --out report.md          # 写入文件而非 stdout
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(
        prog="python -m cslice",
        description="可视化 cslice 对 C 源文件的切分结果（纯标准库，零依赖）",
    )
    parser.add_argument("file", help="C 源文件路径")
    parser.add_argument(
        "--drafts", action="store_true", help="为每个切片生成需求草稿并显示"
    )
    parser.add_argument(
        "--lang",
        choices=["en", "zh"],
        default="en",
        help="草稿语言（默认 en；仅 --drafts 时生效）",
    )
    parser.add_argument(
        "--cfg-mermaid", action="store_true", help="追加每个函数 CFG 的 Mermaid 图"
    )
    parser.add_argument(
        "--forest-mermaid", action="store_true", help="追加切片森林的 Mermaid 图"
    )
    parser.add_argument("--out", help="写入文件而非 stdout（推荐在保存 Mermaid 时使用）")
    args = parser.parse_args(argv)

    try:
        source = Path(args.file).read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError) as exc:
        print(f"无法读取 {args.file}: {exc}", file=sys.stderr)
        return 2

    import cslice
    from cslice import viz

    plans = cslice.slice_all(source)
    if not plans:
        print(f"{args.file} 中未发现函数定义", file=sys.stderr)
        return 1

    drafts_map = None
    if args.drafts:
        from cslice.drafts import generate_drafts

        drafts_map = {
            plan.function.name: generate_drafts(
                plan.function.name, plan.items, language=args.lang
            )
            for plan in plans
        }

    sections = [viz.render_source(source, plans, drafts=drafts_map)]
    if args.cfg_mermaid:
        for plan in plans:
            f = plan.function
            graph = cslice.build_cfg(source, f.start_line, f.end_line)
            if graph is not None:
                sections.append(f"<!-- CFG: {f.name} -->\n```mermaid\n{viz.cfg_to_mermaid(graph)}\n```")
    if args.forest_mermaid:
        for plan in plans:
            sections.append(
                f"<!-- 切片森林: {plan.function.name} -->\n"
                f"```mermaid\n{viz.forest_to_mermaid(plan)}\n```"
            )

    output = "\n\n".join(sections)
    if args.out:
        Path(args.out).write_text(output + "\n", encoding="utf-8")
        print(f"已写入 {args.out}")
    else:
        # Windows 控制台可能是 GBK：统一按 UTF-8 输出，不可编码字符降级替换
        if hasattr(sys.stdout, "reconfigure"):
            sys.stdout.reconfigure(encoding="utf-8", errors="replace")
        print(output)
    return 0


if __name__ == "__main__":
    sys.exit(main())
