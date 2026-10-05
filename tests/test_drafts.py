"""cslice.drafts 端到端测试：切分 → 草稿，与 LLRWorkBench 原实现黄金句逐字对拍。"""

from cslice import parse_functions, plan_function_slices
from cslice.drafts import ReqDraft, fallback_draft, generate_draft, generate_drafts


def plan(src: str, name: str):
    funcs = parse_functions(src)
    f = next(f for f in funcs if f.name == name)
    return plan_function_slices(src, f.start_line, f.end_line)


def test_branch_single_return_zh():
    src = "int foo(int x) {\n    int y = 0;\n    if (x < 0) {\n        return -1;\n    }\n    y = x;\n    return y;\n}\n"
    d = generate_draft("foo", plan(src, "foo")[1], language="zh")
    assert isinstance(d, ReqDraft)
    assert d.description == "函数 foo 应返回 -1，当 x < 0 时。"
    assert d.verify_method == "测试"
    assert "ReqDraft" in repr(d)


def test_computation_init_assign_return():
    src = "int calc(int x) {\n    int y = 0;\n    y = x * 2;\n    return y;\n}\n"
    items = plan(src, "calc")
    assert generate_draft("calc", items[0], language="zh").description == "函数 calc 应将局部变量 y 初始化为 0。"
    assert generate_draft("calc", items[1], language="zh").description == "函数 calc 应将 y 赋值为 x * 2。"
    assert generate_draft("calc", items[2], language="zh").description == "函数 calc 应返回 y。"


def test_loop_with_control_and_body_context():
    src = "int sum(int n) {\n    int acc = 0;\n    for (int i = 0; i < n; i++) {\n        acc += i;\n    }\n    return acc;\n}\n"
    items = plan(src, "sum")
    header = generate_draft("sum", items[1], language="zh")
    assert "函数 sum 应在满足 i < n 的条件下重复执行循环迭代" in header.description
    assert "（循环控制：int i = 0；i < n；i++）" in header.description
    body = generate_draft("sum", items[2], language="zh")
    assert body.description == "在该循环（i < n）的每次迭代中，函数 sum 应执行 acc += i。"


def test_case_and_default():
    src = "int op(int cmd, int v) {\n    switch (cmd) {\n    case 1:\n        v += 1;\n        break;\n    default:\n        v = 0;\n        break;\n    }\n    return v;\n}\n"
    items = plan(src, "op")
    assert generate_draft("op", items[0], language="zh").description == "当 cmd 的取值等于 1 时，函数 op 应执行 v += 1。"
    assert generate_draft("op", items[1], language="zh").description == "当 cmd 的取值不等于任何指定取值时，函数 op 应将 v 赋值为 0。"


def test_preproc_verify_method_is_review():
    src = "void configure(void) {\n    int a = 0;\n#ifdef DEBUG\n    log_debug();\n#endif\n    a = 1;\n}\n"
    items = plan(src, "configure")
    d = generate_draft("configure", items[1], language="zh")
    assert d.verify_method == "审查"
    assert "#ifdef DEBUG" in d.description


def test_nested_loop_parent_context():
    src = "static uint32_t Crc32(const uint8_t* data, uint32_t len)\n{\n    uint32_t crc = 0xFFFFFFFFu;\n    uint32_t i;\n    uint32_t k;\n    for (i = 0u; i < len; i++)\n    {\n        crc ^= (uint32_t)data[i];\n        for (k = 0; k < 8; k++)\n        {\n            if ((crc & 1u) != 0u)\n            {\n                crc = (crc >> 1) ^ 0xEDB88320u;\n            }\n            else\n            {\n                crc >>= 1;\n            }\n        }\n    }\n    return ~crc;\n}\n"
    items = plan(src, "Crc32")
    # 内层循环头：外层循环上下文前缀 + 自身迭代控制
    inner = generate_draft("Crc32", items[3], language="zh")
    assert inner.description.startswith(
        "在外层循环（i < len）的每次迭代中，函数 Crc32 应在满足 k < 8 的条件下重复执行循环迭代"
    )
    # 循环内 if 分支：内层循环上下文前导 + 条件后置
    branch = generate_draft("Crc32", items[4], language="zh")
    assert branch.description == (
        "在该循环（k < 8）的每次迭代中，函数 Crc32 应将 crc 赋值为 (crc >> 1) ^ 0xEDB88320u，"
        "当 (crc & 1u) != 0u 时。"
    )
    # 顶层循环头无父上下文
    outer = generate_draft("Crc32", items[1], language="zh")
    assert "外层循环" not in outer.description


def test_empty_function_fallback_zh_en():
    src = "void nop(void) {\n}\n"
    d = generate_draft("nop", plan(src, "nop")[0], language="zh")
    assert "请人工补充" in d.description
    d_en = generate_draft("nop", plan(src, "nop")[0], language="en")
    assert "template fallback" in d_en.description
    assert d_en.verify_method == "Test"


def test_english_branch():
    src = "int grade(int s) {\n    if (s >= 60) {\n        return 1;\n    } else {\n        return 0;\n    }\n}\n"
    items = plan(src, "grade")
    assert generate_draft("grade", items[0], language="en").description == "The grade function shall return 1 if s >= 60."
    assert generate_draft("grade", items[1], language="en").description == "When none of the above conditions holds, the grade function shall return 0."


def test_invalid_language_falls_back_to_zh():
    src = "int foo(int x) {\n    return x;\n}\n"
    d = generate_draft("foo", plan(src, "foo")[0], language="fr")
    assert d.description == "函数 foo 应返回 x。"


def test_fallback_draft_by_kind():
    d = fallback_draft("foo", "preproc", 10, 20, language="zh")
    assert d.verify_method == "审查"
    assert fallback_draft("foo", "preproc", 10, 20).verify_method == "Review"  # 默认英语
    assert "L10-L20" in d.description
    d = fallback_draft("foo", "computation", 10, 20, language="zh")
    assert d.verify_method == "测试"
    # 未知 kind 按核心口径回退 computation
    d = fallback_draft("foo", "unknown", 10, 20, language="zh")
    assert d.verify_method == "测试"


def test_generate_drafts_batch():
    src = "int calc(int x) {\n    int y = 0;\n    y = x * 2;\n    return y;\n}\n"
    items = plan(src, "calc")
    drafts = generate_drafts("calc", items, language="zh")
    assert [d.description for d in drafts] == [
        "函数 calc 应将局部变量 y 初始化为 0。",
        "函数 calc 应将 y 赋值为 x * 2。",
        "函数 calc 应返回 y。",
    ]
    drafts_en = generate_drafts("calc", items, language="en")
    assert drafts_en[0].description == "The calc function shall initialize the local variable y as 0."


def test_default_language_is_english():
    src = "int calc(int x) {\n    int y = 0;\n    y = x * 2;\n    return y;\n}\n"
    items = plan(src, "calc")
    assert generate_draft("calc", items[0]).description == "The calc function shall initialize the local variable y as 0."
    assert generate_draft("calc", items[0]).verify_method == "Test"
    assert generate_drafts("calc", items)[1].description == "The calc function shall set y to x * 2."
