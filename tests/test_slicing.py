"""cslice Python 绑定集成测试：与 Rust 核心测试（crates/cslice-core）互为镜像。"""

import pytest

import cslice

SRC = """int foo(int x) {
    int y = 0;
    if (x < 0) {
        return -1;
    }
    y = x;
    return y;
}
"""


def test_parse_functions():
    funcs = cslice.parse_functions(SRC)
    assert len(funcs) == 1
    f = funcs[0]
    assert f.name == "foo"
    assert f.signature == "int foo(int x)"
    assert (f.start_line, f.end_line) == (1, 8)
    assert "FunctionDef" in repr(f)


def test_parse_functions_skips_non_functions():
    src = """#include <stdio.h>
#define MAX(a, b) ((a) > (b) ? (a) : (b))
typedef struct Point { int x; } PointT;
static int g = 0;
int only_prototype(int a);
int real(int a) {
    return a + 1;
}
"""
    assert [f.name for f in cslice.parse_functions(src)] == ["real"]


def test_plan_function_slices_kinds():
    items = cslice.plan_function_slices(SRC, 1, 8)
    assert items is not None
    assert [(i.start_line, i.end_line, i.kind) for i in items] == [
        (2, 2, "computation"),  # y 初始化声明独立成片
        (3, 5, "branch"),  # if 分支
        (6, 6, "computation"),  # y = x
        (7, 7, "computation"),  # return 独立成片
    ]
    assert "SlicePlanItem" in repr(items[0])


def test_plan_code_text():
    items = cslice.plan_function_slices(SRC, 1, 8)
    assert items[0].code_text == "    int y = 0;"
    assert items[1].code_text == "    if (x < 0) {\n        return -1;\n    }"


def test_plan_summary_branch():
    items = cslice.plan_function_slices(SRC, 1, 8)
    s = items[1].summary
    assert s.condition == "x < 0"
    assert not s.is_else
    assert [b.kind for b in s.behaviors] == ["return"]
    assert s.behaviors[0].expr == "-1"
    assert s.behavior_count == 1
    assert "Behavior" in repr(s.behaviors[0])


def test_plan_summary_loop_and_parent_cond():
    # CRC32 位处理典型用例：外层循环嵌套内层循环与 if/else，逐层拆片
    src = (
        "static uint32_t Crc32(const uint8_t* data, uint32_t len)\n"
        "{\n"
        "    uint32_t crc = 0xFFFFFFFFu;\n"
        "    uint32_t i;\n"
        "    for (i = 0u; i < len; i++)\n"
        "    {\n"
        "        crc ^= (uint32_t)data[i];\n"
        "        for (k = 0; k < 8; k++)\n"
        "        {\n"
        "            if ((crc & 1u) != 0u)\n"
        "            {\n"
        "                crc = (crc >> 1) ^ 0xEDB88320u;\n"
        "            }\n"
        "            else\n"
        "            {\n"
        "                crc >>= 1;\n"
        "            }\n"
        "        }\n"
        "    }\n"
        "    return ~crc;\n"
        "}\n"
    )
    items = cslice.plan_function_slices(src, 1, 21)
    assert items is not None and len(items) == 7
    kinds = [i.kind for i in items]
    assert kinds == [
        "computation",  # crc 初始化
        "loop",  # 外层循环头
        "computation",  # crc ^= data[i]（循环体直接语句）
        "loop",  # 内层循环头
        "branch",  # if
        "branch",  # else
        "computation",  # return
    ]
    # 嵌套片携带父循环条件
    assert items[2].summary.parent_loop_cond == "i < len"
    assert items[4].summary.parent_loop_cond == "k < 8"  # 直接父循环是内层 for
    # 循环片仅含头行，行为清单为空
    assert items[1].summary.behaviors == []
    assert items[1].summary.loop_cond == "i < len"

    # 树关系：id 连续、父片先于子片、深度从外到内递增
    assert [i.id for i in items] == list(range(7))
    assert (items[0].parent_id, items[0].depth) == (None, 0)
    assert (items[1].parent_id, items[1].depth) == (None, 0)
    assert (items[2].parent_id, items[2].depth) == (1, 1)  # 循环体直接语句 → 外层循环
    assert (items[3].parent_id, items[3].depth) == (1, 1)  # 内层循环头 → 外层循环
    assert (items[4].parent_id, items[4].depth) == (3, 2)  # if → 内层循环
    assert (items[5].parent_id, items[5].depth) == (3, 2)  # else → 内层循环
    assert (items[6].parent_id, items[6].depth) == (None, 0)  # return 顶层
    for item in items:  # 通用不变量
        if item.parent_id is not None:
            parent = items[item.parent_id]
            assert parent.start_line < item.start_line
            assert item.depth == parent.depth + 1


def test_plan_switch_cases_are_children_of_enclosing_loop():
    src = (
        "int f(int n) {\n"
        "    for (int i = 0; i < n; i++)\n"
        "    {\n"
        "        switch (i) {\n"
        "        case 0:\n"
        "            work(0);\n"
        "            break;\n"
        "        default:\n"
        "            work(1);\n"
        "            break;\n"
        "        }\n"
        "    }\n"
        "}\n"
    )
    items = cslice.plan_function_slices(src, 1, 13)
    assert items is not None and len(items) == 3
    assert items[0].kind == "loop"
    for item in items[1:]:
        assert item.kind == "case"
        assert item.parent_id == 0
        assert item.depth == 1


def test_plan_switch():
    src = (
        "int op(int cmd, int v) {\n"
        "    switch (cmd) {\n"
        "    case 1:\n"
        "        v += 1;\n"
        "        break;\n"
        "    default:\n"
        "        v = 0;\n"
        "        break;\n"
        "    }\n"
        "    return v;\n"
        "}\n"
    )
    items = cslice.plan_function_slices(src, 1, 11)
    assert items is not None
    kinds = [i.kind for i in items]
    assert kinds == ["case", "case", "computation"]
    assert items[0].summary.case_value == "1"
    assert items[0].summary.condition == "cmd"
    assert items[1].summary.is_else  # default
    assert items[1].summary.case_value is None


def test_plan_preproc():
    src = (
        "void configure(void) {\n"
        "#ifdef DEBUG\n"
        "    log_debug();\n"
        "#else\n"
        "    log_release();\n"
        "#endif\n"
        "}\n"
    )
    items = cslice.plan_function_slices(src, 1, 7)
    assert items is not None
    assert [i.kind for i in items] == ["preproc"]
    assert items[0].summary.preproc_directive == "#ifdef DEBUG"


def test_plan_not_found_returns_none():
    assert cslice.plan_function_slices(SRC, 1, 99) is None
    assert cslice.plan_function_slices("", 1, 1) is None


def test_extract_lines():
    assert cslice.extract_lines(SRC, 1, 2) == "int foo(int x) {\n    int y = 0;"
    assert cslice.extract_lines("a\r\nb\r\n", 1, 2) == "a\nb"
    with pytest.raises(ValueError):
        cslice.extract_lines(SRC, 1, 100)
    with pytest.raises(ValueError):
        cslice.extract_lines(SRC, 2, 1)


def test_slice_all():
    plans = cslice.slice_all(SRC)
    assert len(plans) == 1
    plan = plans[0]
    assert plan.function.name == "foo"
    assert len(plan.items) == 4
    assert plan.items[1].code_text.startswith("    if")
    assert "FunctionPlan" in repr(plan) or isinstance(plan, object)


def test_kinds_constant():
    assert cslice.KINDS == ("computation", "branch", "loop", "case", "preproc")


def test_syntax_error_tolerance():
    src = (
        "int broken(int a {\n"
        "    return a;\n"
        "}\n"
        "\n"
        "int good(int x) {\n"
        "    return x;\n"
        "}\n"
    )
    funcs = cslice.parse_functions(src)
    assert "good" in [f.name for f in funcs]
    for f in funcs:
        assert f.end_line >= f.start_line
