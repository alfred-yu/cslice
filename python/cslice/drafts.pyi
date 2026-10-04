from typing import List, Optional

from cslice import SlicePlanItem

VerifyMethod = str


class ReqDraft:
    """需求草稿：描述/验证方法两要素。"""

    description: str
    verify_method: str  # 条件编译 → "审查"/"Review"，其余 → "测试"/"Test"

    def __repr__(self) -> str: ...


def generate_draft(
    func_name: str, item: SlicePlanItem, language: str = "zh"
) -> ReqDraft:
    """从单个切片计划项生成需求草稿（模板路径，离线确定）。
    language："zh" 中文"应"句式 / "en" 英文 shall 句式，其他值按中文处理。"""
    ...


def fallback_draft(
    func_name: str, kind: str, start_line: int, end_line: int, language: str = "zh"
) -> ReqDraft:
    """兜底草稿：语义提取失败（空函数体/无法解析的行为）时使用。
    kind 传切片类型字符串，未知值按核心口径回退为 computation。"""
    ...


def generate_drafts(
    func_name: str, items: List[SlicePlanItem], language: str = "zh"
) -> List[ReqDraft]:
    """批量：对同一函数的多个切片计划项生成草稿（按列表顺序）。"""
    ...
