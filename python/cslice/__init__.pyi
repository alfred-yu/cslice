from typing import List, Literal, Optional, Tuple

SliceKind = Literal["computation", "branch", "loop", "case", "preproc"]
BehaviorKind = Literal[
    "init", "assign", "compound_assign", "return", "call", "break", "continue"
]

KINDS: Tuple[SliceKind, ...]
__version__: str


class Behavior:
    """逻辑块内的单一行为事实。kind 决定哪些字段有值：init → var/value；
    assign → lhs/rhs；compound_assign → text；return → expr（裸 return 为空串）；
    call → name；break/continue 无附加字段。"""

    kind: BehaviorKind
    var: Optional[str]
    value: Optional[str]
    lhs: Optional[str]
    rhs: Optional[str]
    text: Optional[str]
    expr: Optional[str]
    name: Optional[str]

    def __repr__(self) -> str: ...


class BlockSummary:
    """切片语义摘要：从 AST 提取的"代码事实"。"""

    condition: Optional[str]  # if/else-if/switch 条件表达式（去外层括号）
    is_else: bool  # 无条件分支（else / default）
    case_value: Optional[str]  # case 取值文本（default 为 None）
    loop_header: Optional[str]  # 循环头完整文本，仅 loop 类切片
    loop_init: Optional[str]  # for 三段式
    loop_cond: Optional[str]
    loop_update: Optional[str]
    preproc_directive: Optional[str]  # 条件编译指令首行，仅 preproc 类切片
    parent_loop_cond: Optional[str]  # 嵌套循环拆分产生的内层片携带父循环条件
    behaviors: List[Behavior]  # 行为清单（按源码顺序）
    behavior_count: int


class FunctionDef:
    """一个 C 函数定义。"""

    name: str
    signature: str  # 函数定义起始到函数体 '{' 之前
    start_line: int  # 1-based（含）
    end_line: int  # 1-based（含）

    def __repr__(self) -> str: ...


class SlicePlanItem:
    """一个切片：函数体内可独立描述的原子行为代码块。全部切片构成
    深度优先森林：父片必先于子片出现（从顶向下、从外到内）。"""

    id: int  # 函数内序号（0-based，等于列表下标）
    parent_id: Optional[int]  # 包裹本片的循环头片 id；顶层片为 None
    depth: int  # 嵌套深度：函数体顶层 0，进入循环体 +1
    guard_conds: List[str]  # 执行前置守卫条件（卫语句放行条件，按序累积）
    start_line: int  # 1-based（含）
    end_line: int  # 1-based（含）
    kind: SliceKind
    code_text: str  # 切片代码文本快照
    summary: BlockSummary

    def __repr__(self) -> str: ...


class FunctionPlan:
    """单个函数的完整切片计划（slice_all 的返回项）。"""

    function: FunctionDef
    items: List[SlicePlanItem]  # 按行有序；个别函数计划失败时为空


def parse_functions(source: str) -> List[FunctionDef]:
    """解析 C 源码中的全部函数定义（按出现顺序）。语法错误时容错解析。"""
    ...


def plan_function_slices(
    source: str, start_line: int, end_line: int
) -> Optional[List[SlicePlanItem]]:
    """按行范围定位函数并生成逻辑块切片计划（1-based 含端点，须精确匹配
    函数起止行）。找不到匹配的函数定义时返回 None。每片附带 code_text。"""
    ...


def extract_lines(source: str, start_line: int, end_line: int) -> str:
    """提取 1-based 行范围 [start_line, end_line] 的代码文本（去除行尾 \\r，
    \\n 连接，不含结尾换行）。越界抛 ValueError。"""
    ...


def slice_all(source: str) -> List[FunctionPlan]:
    """一次性切分源码中的全部函数，每片附带 code_text。"""
    ...


# ---------------------------------------------------------------------------
# 控制流图（CFG）
# ---------------------------------------------------------------------------

class CfgNode:
    """CFG 节点：基本块与控制结构，带"做什么"语义标签（非源码原文）。"""

    id: int
    kind: Literal[
        "entry", "exit", "block", "branch", "loop", "switch", "case", "return", "join"
    ]
    label: str  # 条件表达式 / "返回 x" / "调用 f()" 等
    start_line: int  # 1-based（含）
    end_line: int  # 1-based（含）


class CfgEdge:
    """CFG 有向边（src → dst）：label 携带条件/事件语义，style 为视觉分类。
    属性名用 src/dst 而非 from/to——from 是 Python 关键字。"""

    src: int
    dst: int
    label: Optional[str]  # 是/否/直落/循环/break/continue/退出/case 值
    style: str  # "seq" 顺序流 / "false" 条件不成立 / "back" 回边或跳出


class CfgGraph:
    """单个函数的控制流图。"""

    nodes: List[CfgNode]
    edges: List[CfgEdge]


def build_cfg(source: str, start_line: int, end_line: int) -> Optional[CfgGraph]:
    """生成函数控制流图（1-based 起止行定位函数）；找不到返回 None。"""
    ...
