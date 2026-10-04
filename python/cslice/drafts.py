"""cslice.drafts：需求草稿生成（可选领域能力）。

从切分结果生成模板化需求草稿（离线、确定性）：默认英文 shall 句式，
传 language="zh" 得中文"应"句式；验证方法按切片类型推断
（条件编译 → 审查/Review，其余 → 测试/Test）。

这是面向需求工程场景的附加能力——只需要代码切分的用户可完全忽略本模块。
本模块不包含任何 LLM/网络能力；AI 增强属调用方。
"""

from cslice._native import (
    ReqDraft,
    draft_fallback_draft as fallback_draft,
    draft_generate_draft as generate_draft,
    draft_generate_drafts as generate_drafts,
)

__all__ = [
    "ReqDraft",
    "fallback_draft",
    "generate_draft",
    "generate_drafts",
]
