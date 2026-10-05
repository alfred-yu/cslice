"""cslice.drafts 守卫语境前缀与 lint 合规检查的端到端测试。"""

from cslice import parse_functions, plan_function_slices
from cslice.drafts import (
    LintIssue,
    LintSummary,
    generate_draft,
    lint_requirement,
    lint_summarize,
)

GUARD_SRC = (
    "int f(const int* p, int n) {\n"
    "    int s = 0;\n"
    "    if (p == 0 || n <= 0) {\n"
    "        return -1;\n"
    "    }\n"
    "    for (int i = 0; i < n; i++) {\n"
    "        s += p[i];\n"
    "    }\n"
    "    return s;\n"
    "}\n"
)


def plan(src: str, name: str):
    f = next(x for x in parse_functions(src) if x.name == name)
    return plan_function_slices(src, f.start_line, f.end_line)


def test_draft_carries_guard_condition_trailing_zh():
    items = plan(GUARD_SRC, "f")
    d = generate_draft("f", items[2], language="zh")  # 循环头片
    # 守卫条件行内后置（单一原子条件）
    assert d.description.endswith("，当 !(p == 0 || n <= 0) 时。")
    assert d.description.startswith("函数 f 应在满足 i < n 的条件下重复执行循环迭代")
    # 守卫之前的初始化片不受影响
    assert generate_draft("f", items[0], language="zh").description == "函数 f 应将局部变量 s 初始化为 0。"


def test_draft_guard_condition_trailing_en():
    items = plan(GUARD_SRC, "f")
    d = generate_draft("f", items[3], language="en")  # 循环体计算片
    # 循环语境前导，守卫条件句尾 if 后置（单条件）
    assert d.description == (
        "In each iteration of the loop where i < n holds, "
        "the f function shall execute s += p[i] if !(p == 0 || n <= 0)."
    )


def test_lint_compliant_draft_is_clean():
    items = plan(GUARD_SRC, "f")
    d = generate_draft("f", items[2], language="zh")
    issues = lint_requirement(d.description, d.verify_method, "f")
    assert issues == [], f"模板草稿应 0 告警: {issues}"


def test_lint_english_draft_is_clean():
    items = plan(GUARD_SRC, "f")
    d = generate_draft("f", items[3], language="en")
    issues = lint_requirement(d.description, d.verify_method, "f")
    assert [i.level for i in issues if i.level == "warning"] == [], f"{issues}"


def test_lint_detects_ambiguous_word():
    issues = lint_requirement("系统应尽可能及时完成处理。", "测试", None)
    assert any("尽可能" in i.message for i in issues)
    assert isinstance(issues[0], LintIssue)


def test_lint_summary_counts():
    issues = lint_requirement("", "", None)
    s = lint_summarize(issues)
    assert isinstance(s, LintSummary)
    assert s.warning_count >= 2  # 空描述 + 空验证方法
    assert s.info_count == 0
