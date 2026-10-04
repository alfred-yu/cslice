from typing import List, Optional

from cslice import SlicePlanItem

VerifyMethod = str


class ReqDraft:
    """需求草稿：描述/验证方法两要素。"""

    description: str
    verify_method: str  # 条件编译 → "审查"/"Review"，其余 → "测试"/"Test"

    def __repr__(self) -> str: ...


def generate_draft(
    func_name: str, item: SlicePlanItem, language: str = "en"
) -> ReqDraft:
    """从单个切片计划项生成需求草稿（模板路径，离线确定）。
    切片的守卫条件（guard_conds）自动作为执行语境前缀。
    language 默认 "en"（英文 shall 句式）；传 "zh" 得中文"应"句式，其他值按中文处理。"""
    ...


def fallback_draft(
    func_name: str, kind: str, start_line: int, end_line: int, language: str = "en"
) -> ReqDraft:
    """兜底草稿：语义提取失败（空函数体/无法解析的行为）时使用。
    kind 传切片类型字符串，未知值按核心口径回退为 computation。"""
    ...


def generate_drafts(
    func_name: str, items: List[SlicePlanItem], language: str = "en"
) -> List[ReqDraft]:
    """批量：对同一函数的多个切片计划项生成草稿（按列表顺序）。"""
    ...


class LintIssue:
    """单条 lint 检查结果。"""

    level: str  # "warning"（应修复）| "info"（供参考）
    message: str

    def __repr__(self) -> str: ...


class LintSummary:
    """lint 检查结果汇总。"""

    warning_count: int
    info_count: int

    def __repr__(self) -> str: ...


def lint_requirement(
    description: str, verify_method: str, func_name: Optional[str] = None
) -> List[LintIssue]:
    """对需求描述与验证方法做合规检查：空描述、"应/shall"句式、函数名主语、
    歧义词、空验证方法、原子性启发。func_name 未关联切片时传 None。"""
    ...


def lint_summarize(issues: List[LintIssue]) -> LintSummary:
    """汇总 lint 检查结果（按级别计数）。"""
    ...
