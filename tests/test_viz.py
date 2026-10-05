"""cslice.viz 与命令行入口测试。"""

import cslice
from cslice import __main__ as cli
from cslice import viz

SRC = (
    "int f(const int* p, int n) {\n"
    "    int s = 0;\n"
    "    if (p == 0 || n <= 0) {\n"
    '        log("bad");\n'
    "        return -1;\n"
    "    }\n"
    "    for (int i = 0; i < n; i++)\n"
    "    {\n"
    "        s += p[i];\n"
    "    }\n"
    "    return s;\n"
    "}\n"
)


def plan():
    return cslice.slice_all(SRC)[0]


def test_annotated_source_marks_owners():
    text = viz.annotated_source(SRC, plan())
    # 签名行与守卫自身分支行可见，逐行带归属标记
    assert " 1  . | int f(const int* p, int n) {" in text
    assert " 7  2 |     for (int i = 0; i < n; i++)" in text
    assert "函数 f：L1-L12，5 个切片" in text


def test_slice_details_includes_guards():
    text = viz.slice_details(plan())
    assert "守卫=[!(p == 0 || n <= 0)]" in text
    # 守卫自身的片不带守卫
    assert "片#1  (深度0, 顶层) L3-L6 [branch]" in text


def test_render_plan_with_drafts():
    from cslice.drafts import generate_drafts

    items = plan().items
    drafts = generate_drafts("f", items, language="en")
    text = viz.render_plan(SRC, plan(), drafts=drafts)
    assert "—— 切片需求草稿 ——" in text
    assert "[Test]" in text


def test_cfg_to_mermaid():
    g = __import__("cslice").build_cfg(SRC, 1, 12)
    text = viz.cfg_to_mermaid(g)
    assert text.startswith("flowchart TB")
    assert ":::branch" in text and ":::loop" in text
    assert "-->|是|" in text or "-->|否|" in text
    assert "linkStyle" in text  # 假边虚线 / 回边加粗
    # 消毒：不含裸引号
    assert '"' not in text.split("flowchart TB")[1].split("classDef")[0].replace('["', "").replace('"]', "")


def test_forest_to_mermaid_parent_edges():
    text = viz.forest_to_mermaid(plan())
    assert "subgraph fn[\"f\"]" in text
    assert "s2 --> s3" in text  # 循环头 → 体内计算片
    assert "classDef loop" in text


def test_cli_text_and_drafts(tmp_path, capsys):
    src_file = tmp_path / "demo.c"
    src_file.write_text(SRC, encoding="utf-8")
    out_file = tmp_path / "report.md"
    rc = cli.main([str(src_file), "--drafts", "--lang", "zh", "--out", str(out_file)])
    assert rc == 0
    report = out_file.read_text(encoding="utf-8")
    assert "—— 切片树" in report
    assert "—— 切片需求草稿 ——" in report
    assert "函数 f 应将局部变量 s 初始化为 0。" in report


def test_cli_cfg_mermaid_stdout(tmp_path, capsys):
    src_file = tmp_path / "demo.c"
    src_file.write_text(SRC, encoding="utf-8")
    rc = cli.main([str(src_file), "--cfg-mermaid", "--forest-mermaid"])
    assert rc == 0
    out = capsys.readouterr().out
    assert "```mermaid" in out
    assert out.count("flowchart TB") == 2  # CFG + 切片森林
    assert "subgraph fn[\"f\"]" in out


def test_cli_missing_file(tmp_path):
    rc = cli.main([str(tmp_path / "nope.c")])
    assert rc == 2


def test_cli_no_functions(tmp_path, capsys):
    src_file = tmp_path / "empty.c"
    src_file.write_text("int x;\n", encoding="utf-8")
    rc = cli.main([str(src_file)])
    assert rc == 1
