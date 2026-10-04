//! `cslice` 核心引擎：C 函数解析与逻辑块切分（基于 tree-sitter / tree-sitter-c）。
//!
//! 无状态纯函数库，零业务耦合：
//! - [`parse_functions`]：解析源码中的全部函数定义（名称、签名、行范围）
//! - [`plan_function_slices`]：把单个函数体按逻辑块切分为切片计划
//!   （computation / branch / loop / case / preproc），每片带语义摘要
//! - [`extract_lines`]：按 1-based 行范围提取代码文本快照
//!
//! 本 crate 不含 Python 绑定；Python 包由 `cslice-py`（PyO3）提供。

use tree_sitter::{Node, Parser};

/// 解析出的函数定义
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDef {
    /// 函数名（标识符）
    pub name: String,
    /// 签名文本：从函数定义起始到函数体 '{' 之前的源码片段（含返回类型、函数名、参数表）
    pub signature: String,
    /// 1-based 起始行（含）
    pub start_line: u32,
    /// 1-based 结束行（含）
    pub end_line: u32,
}

/// 解析 C 源码内容，提取全部函数定义（按出现顺序）。
/// tree-sitter 容错解析：语法错误时仍尽量返回可识别的函数。
pub fn parse_functions(content: &str) -> Vec<FunctionDef> {
    let mut parser = match new_parser() {
        Some(p) => p,
        None => return Vec::new(),
    };
    let tree = match parser.parse(content, None) {
        Some(t) => t,
        None => return Vec::new(),
    };
    let mut result = Vec::new();
    collect_function_defs(&tree.root_node(), content, &mut result);
    result
}

/// 创建并配置 C 语法解析器。
pub(crate) fn new_parser() -> Option<Parser> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_c::LANGUAGE.into())
        .ok()?;
    Some(parser)
}

/// 递归遍历全部节点（含函数体内部、preproc 块等任意嵌套位置），
/// 按出现顺序收集 kind == "function_definition" 的节点。
fn collect_function_defs(node: &Node, content: &str, out: &mut Vec<FunctionDef>) {
    if node.kind() == "function_definition" {
        if let Some(fd) = build_function_def(node, content) {
            out.push(fd);
        }
    }
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            collect_function_defs(&child, content, out);
        }
    }
}

/// 从 function_definition 节点构造 FunctionDef；无法确定名字或函数体时返回 None。
fn build_function_def(node: &Node, content: &str) -> Option<FunctionDef> {
    let body = node.child_by_field_name("body")?;
    let declarator = node.child_by_field_name("declarator")?;
    let name = identifier_name(declarator, content)?;

    // 签名：函数定义起始到函数体 '{' 之前（不含 '{'）。
    let raw = content.get(node.start_byte()..body.start_byte())?;
    let signature = normalize_signature(raw);

    Some(FunctionDef {
        name,
        signature,
        start_line: node.start_position().row as u32 + 1,
        end_line: node.end_position().row as u32 + 1,
    })
}

/// 从 declarator 出发，沿 "declarator" 字段链下降（function_declarator /
/// pointer_declarator / parenthesized_declarator 等嵌套），直到 identifier。
pub(crate) fn identifier_name(mut node: Node, content: &str) -> Option<String> {
    loop {
        if node.kind() == "identifier" {
            return node.utf8_text(content.as_bytes()).ok().map(str::to_string);
        }
        node = node.child_by_field_name("declarator")?;
    }
}

/// 去除首尾空白与多余空行：逐行 trim、剔除空行后以换行拼接。
fn normalize_signature(raw: &str) -> String {
    raw.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------
// 自动切片规划（逻辑块级）
// ---------------------------------------------------------------------------

/// 自动切片计划项：函数体内一个可独立作为低层需求实现依据的代码块
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlicePlanKind {
    /// 连续简单语句组（声明/赋值/调用/return，含函数签名等上下文）
    Computation,
    /// if / else-if / else 分支
    Branch,
    /// for / while / do-while 循环
    Loop,
    /// switch 的 case / default 分支
    Case,
    /// 函数体内的条件编译块（#ifdef 等）
    Preproc,
}

impl SlicePlanKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SlicePlanKind::Computation => "computation",
            SlicePlanKind::Branch => "branch",
            SlicePlanKind::Loop => "loop",
            SlicePlanKind::Case => "case",
            SlicePlanKind::Preproc => "preproc",
        }
    }

    /// `as_str` 的逆运算：按 slices.kind 等持久化编码还原枚举。
    /// 未知值（空串/旧数据）回退 Computation——兜底场景的安全默认。
    pub fn from_code(code: &str) -> Self {
        match code {
            "branch" => SlicePlanKind::Branch,
            "loop" => SlicePlanKind::Loop,
            "case" => SlicePlanKind::Case,
            "preproc" => SlicePlanKind::Preproc,
            _ => SlicePlanKind::Computation,
        }
    }
}

/// 逻辑块内的单一行为事实（按源码出现顺序收集）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Behavior {
    /// 声明带初始化：var = value（如 y = 0）
    Init { var: String, value: String },
    /// 普通赋值：lhs = rhs（operator 为 "="）
    Assign { lhs: String, rhs: String },
    /// 复合赋值（y += i、y &= x 等），保留原表达式文本
    CompoundAssign { text: String },
    /// return 语句（裸 return 时 expr 为空串）
    Return { expr: String },
    /// 函数调用（函数名）
    Call { name: String },
    Break,
    Continue,
}

/// 逻辑块的语义摘要：从 AST 提取的"代码事实"，供需求模板生成器使用
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BlockSummary {
    /// if/else-if/switch 条件表达式文本（去外层括号）；提取失败为 None
    pub condition: Option<String>,
    /// 是否为无条件分支（else / default）
    pub is_else: bool,
    /// case 取值文本（default 分支为 None；仅 Case 类块有值）
    pub case_value: Option<String>,
    /// 循环头完整文本（如 "for (int i = 0; i < 10; i++)"），仅 Loop 类块
    pub loop_header: Option<String>,
    /// for 三段式：init / condition / update 文本（均可能缺省，如 for(;;)）
    pub loop_init: Option<String>,
    pub loop_cond: Option<String>,
    pub loop_update: Option<String>,
    /// 条件编译指令首行（如 "#ifdef DEBUG"），仅 Preproc 类块
    pub preproc_directive: Option<String>,
    /// 父循环条件文本：循环体嵌套拆分产生的片（内层循环片/尾部计算片）
    /// 携带外层循环条件，供需求模板生成"外层循环每次迭代中"的上下文
    pub parent_loop_cond: Option<String>,
    /// 行为清单（赋值/初始化/return/调用/跳转，按源码顺序）
    pub behaviors: Vec<Behavior>,
    /// 行为语句总数
    pub behavior_count: u32,
}

/// 自动切片计划项（1-based 行范围，含端点）
#[derive(Debug, Clone, PartialEq)]
pub struct SlicePlanItem {
    pub start_line: u32,
    pub end_line: u32,
    pub kind: SlicePlanKind,
    pub summary: BlockSummary,
}

/// 按行范围定位函数节点并生成自动切片计划。
/// 找不到匹配的函数定义时返回 None（函数与基线 commit 内容不一致等）。
pub fn plan_function_slices(
    content: &str,
    func_start_line: u32,
    func_end_line: u32,
) -> Option<Vec<SlicePlanItem>> {
    let mut parser = new_parser()?;
    let tree = parser.parse(content, None)?;
    let node = find_function_by_range(&tree.root_node(), func_start_line, func_end_line)?;
    Some(collect_slice_plan(&node, content))
}

/// 按 1-based 起止行精确匹配 function_definition 节点
pub(crate) fn find_function_by_range<'a>(
    node: &Node<'a>,
    start_line: u32,
    end_line: u32,
) -> Option<Node<'a>> {
    if node.kind() == "function_definition" {
        let s = node.start_position().row as u32 + 1;
        let e = node.end_position().row as u32 + 1;
        if s == start_line && e == end_line {
            return Some(*node);
        }
    }
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if let Some(found) = find_function_by_range(&child, start_line, end_line) {
                return Some(found);
            }
        }
    }
    None
}

/// 遍历函数体直接子节点切分逻辑块，行范围采用**精确行为边界**：
/// 每片 = 逻辑块的自然行范围，函数头（签名）行与 '{' '}' 所在行
/// 不参与切片，纯声明/注释/空行不归属任何片（片间允许未覆盖行），
/// 切片范围与其需求描述的行为精确对应；仅当相邻块自然范围重叠时
/// （如 else 关键字行同时是 if 分支体的结束行）将后片起点压到前片
/// 结束 + 1 消除重叠。
/// 顶层 return 语句独立成片（行为边界）：其前的语句是数据准备、
/// return 本身是返回结果（可观测输出），分别对应各自的低层需求。
/// 局部变量声明：带初始化的独立成片（初始值影响逻辑，一条初始化需求）；
/// 纯声明不参与切片（无行为可表述）。
/// 循环体嵌套拆分：循环头行独立成片（迭代控制是独立的层级行为），
/// 循环体内嵌套的控制结构（if 链/循环/switch/条件编译）递归独立成片
/// （DO-178C 原子性——嵌套控制流是不同层级行为，不得用一条需求覆盖），
/// 嵌套片与体内直接语句计算片的 summary.parent_loop_cond 携带直接父循环条件。
/// 切分的同时从 AST 提取各块的语义摘要（BlockSummary）。
fn collect_slice_plan(func: &Node, content: &str) -> Vec<SlicePlanItem> {
    let func_start = func.start_position().row as u32 + 1;
    let func_end = func.end_position().row as u32 + 1;
    let Some(body) = func.child_by_field_name("body") else {
        return vec![SlicePlanItem {
            start_line: func_start,
            end_line: func_end,
            kind: SlicePlanKind::Computation,
            summary: BlockSummary::default(),
        }];
    };

    // 切片基准起止：切片仅覆盖函数体内容行——函数头（签名）行与 '{' '}' 所在行
    // 均不参与切片（大括号本身无行为语义）。
    // - 起始：跳过 '{' 所在行（无论该行是否含签名）；
    //   若体内首条语句与 '{' 同行（单行函数），无法避开，从该行开始。
    // - 结束：末片最多延伸到 '}' 所在行的前一行；
    //   若最后一条语句与 '}' 同行（如 "return v; }"），无法避开，到该行。
    let brace_line = body.start_position().row as u32 + 1;
    let close_line = body.end_position().row as u32 + 1;
    let has_stmt_at = |line: u32, use_start: bool| {
        (0..body.child_count()).any(|i| {
            body.child(i)
                .map(|c| {
                    let k = c.kind();
                    if k == "{" || k == "}" || k == "comment" {
                        return false;
                    }
                    if use_start {
                        (c.start_position().row as u32 + 1) == line
                    } else {
                        (c.end_position().row as u32 + 1) == line
                    }
                })
                .unwrap_or(false)
        })
    };
    let base = if has_stmt_at(brace_line, true) {
        brace_line
    } else {
        brace_line + 1
    };
    let body_end = if has_stmt_at(close_line, false) {
        close_line
    } else {
        close_line - 1
    };
    // 单行空函数等边界：起点不越过函数结束行
    let base = base.min(func_end);

    // 1) 收集逻辑块的自然行范围与语义摘要
    let mut blocks: Vec<PlannedBlock> = Vec::new();
    // 计算组：连续简单语句/声明，聚合子节点后统一提取行为
    let mut group: Option<(u32, u32, Vec<Node>)> = None;

    for i in 0..body.child_count() {
        let Some(child) = body.child(i) else { continue };
        match child.kind() {
            // 大括号与注释跳过：注释行由无缝拼接吸收进下一片（注释描述其后的代码）
            "{" | "}" | "comment" => {}
            "if_statement" => {
                flush_group(&mut group, &mut blocks, content);
                plan_if_chain(&child, content, &mut blocks, None);
            }
            "for_statement" | "while_statement" | "do_statement" => {
                flush_group(&mut group, &mut blocks, content);
                plan_loop(&child, content, &mut blocks, None);
            }
            "switch_statement" => {
                flush_group(&mut group, &mut blocks, content);
                plan_switch(&child, content, &mut blocks, None);
            }
            k if k.starts_with("preproc_") => {
                flush_group(&mut group, &mut blocks, content);
                let mut summary = BlockSummary::default();
                summary.preproc_directive = first_line_text(&child, content);
                collect_behaviors(&child, content, &mut summary);
                blocks.push(PlannedBlock {
                    natural_start: child.start_position().row as u32 + 1,
                    natural_end: child.end_position().row as u32 + 1,
                    kind: SlicePlanKind::Preproc,
                    summary,
                });
            }
            "return_statement" => {
                // 顶层 return 独立成片：结束当前计算组（数据准备归前片），
                // return（返回结果）单独一片。其后语句（不可达代码）归新组。
                flush_group(&mut group, &mut blocks, content);
                let mut summary = BlockSummary::default();
                collect_behaviors(&child, content, &mut summary);
                blocks.push(PlannedBlock {
                    natural_start: child.start_position().row as u32 + 1,
                    natural_end: child.end_position().row as u32 + 1,
                    kind: SlicePlanKind::Computation,
                    summary,
                });
            }
            "declaration" => {
                // 局部变量声明：
                // - 带初始化（初始值影响函数逻辑）：独立成片，对应一条
                //   "将变量初始化为值"的低层需求；
                // - 纯声明（无初始化）：不参与切片（无行为可表述），
                //   其所在行不归属任何片（片间未覆盖行）。
                let has_init = (0..child.named_child_count()).any(|j| {
                    child
                        .named_child(j)
                        .map(|c| c.kind() == "init_declarator")
                        .unwrap_or(false)
                });
                if has_init {
                    flush_group(&mut group, &mut blocks, content);
                    let mut summary = BlockSummary::default();
                    collect_behaviors(&child, content, &mut summary);
                    blocks.push(PlannedBlock {
                        natural_start: child.start_position().row as u32 + 1,
                        natural_end: child.end_position().row as u32 + 1,
                        kind: SlicePlanKind::Computation,
                        summary,
                    });
                }
            }
            _ => {
                // 简单语句/声明/注释：并入当前组（仅记录起止行，中间空行自然吸收）
                let s = child.start_position().row as u32 + 1;
                let e = child.end_position().row as u32 + 1;
                group = Some(match group {
                    Some((gs, _, mut nodes)) => {
                        nodes.push(child);
                        (gs, e, nodes)
                    }
                    None => (s, e, vec![child]),
                });
            }
        }
    }
    flush_group(&mut group, &mut blocks, content);

    // 2) 精确行为边界：每片行范围 = 逻辑块的自然行范围；仅当相邻块自然
    //    范围重叠时（如 else 关键字行同时是 if 分支体的结束行）将后片
    //    起点压到前片结束 + 1 消除重叠。纯声明/注释/空行不归属任何片
    //    （片间允许未覆盖行），切片范围与其需求描述的行为精确对应。
    let mut result: Vec<SlicePlanItem> = Vec::new();
    for block in blocks {
        let start = match result.last() {
            Some(prev) => block.natural_start.max(prev.end_line + 1),
            None => block.natural_start,
        };
        let end = block.natural_end.max(start);
        result.push(SlicePlanItem {
            start_line: start,
            end_line: end,
            kind: block.kind,
            summary: block.summary,
        });
    }

    // 空函数体/纯声明兜底：无行为块可切，一片供人工补充需求。
    // 有内容行（base <= body_end，如仅纯声明）时覆盖内容行；
    // 否则（'{' '}' 相邻或同行）退化为覆盖到函数尾的最小合法范围。
    if result.is_empty() {
        let end = if base <= body_end { body_end } else { func_end };
        return vec![SlicePlanItem {
            start_line: base.min(end),
            end_line: end,
            kind: SlicePlanKind::Computation,
            summary: BlockSummary::default(),
        }];
    }
    result
}

/// 切分中的中间结构：自然行范围 + 块类型 + 语义摘要
struct PlannedBlock {
    natural_start: u32,
    natural_end: u32,
    kind: SlicePlanKind,
    summary: BlockSummary,
}

/// 将当前累积的简单语句组作为一个计算片输出（聚合子节点行为）
fn flush_group(
    group: &mut Option<(u32, u32, Vec<Node>)>,
    blocks: &mut Vec<PlannedBlock>,
    content: &str,
) {
    if let Some((s, e, nodes)) = group.take() {
        let mut summary = BlockSummary::default();
        for n in &nodes {
            collect_behaviors(n, content, &mut summary);
        }
        blocks.push(PlannedBlock {
            natural_start: s,
            natural_end: e,
            kind: SlicePlanKind::Computation,
            summary,
        });
    }
}

/// 拆分 if / else-if / else 链：每个分支一片（含条件与完整分支体）。
/// 嵌套在分支体内部的 if 不拆（随所属分支成片）；分支位于循环体内时
/// parent_loop_cond 携带外层循环条件（需求上下文）。
fn plan_if_chain(if_node: &Node, content: &str, blocks: &mut Vec<PlannedBlock>, parent_loop_cond: Option<&str>) {
    // 首个 if 片：语句头（含条件）到 consequence 结束
    let start = if_node.start_position().row as u32 + 1;
    let end = if_node
        .child_by_field_name("consequence")
        .map(|c| c.end_position().row as u32 + 1)
        .unwrap_or_else(|| if_node.end_position().row as u32 + 1);
    let mut summary = BlockSummary::default();
    summary.parent_loop_cond = parent_loop_cond.map(str::to_string);
    summary.condition = if_node
        .child_by_field_name("condition")
        .map(|c| condition_text(&c, content));
    if let Some(cons) = if_node.child_by_field_name("consequence") {
        collect_behaviors(&cons, content, &mut summary);
    }
    blocks.push(PlannedBlock {
        natural_start: start,
        natural_end: end,
        kind: SlicePlanKind::Branch,
        summary,
    });

    // 沿 else_clause 链逐分支拆分；alternative 内部语句无字段名
    // （named_child(0) 跳过 "else" token），为 if_statement 时即 else-if，否则为普通 else 块
    let mut current = *if_node;
    while let Some(alt) = current.child_by_field_name("alternative") {
        match alt.named_child(0) {
            Some(inner) if inner.kind() == "if_statement" => {
                // else-if 片：else 到内层 if 的 consequence 结束（内层条件含其中）
                let inner_end = inner
                    .child_by_field_name("consequence")
                    .map(|c| c.end_position().row as u32 + 1)
                    .unwrap_or_else(|| inner.end_position().row as u32 + 1);
                let mut summary = BlockSummary::default();
                summary.parent_loop_cond = parent_loop_cond.map(str::to_string);
                summary.condition = inner
                    .child_by_field_name("condition")
                    .map(|c| condition_text(&c, content));
                if let Some(cons) = inner.child_by_field_name("consequence") {
                    collect_behaviors(&cons, content, &mut summary);
                }
                blocks.push(PlannedBlock {
                    natural_start: alt.start_position().row as u32 + 1,
                    natural_end: inner_end,
                    kind: SlicePlanKind::Branch,
                    summary,
                });
                current = inner;
            }
            _ => {
                // 普通 else 块：无条件分支
                let mut summary = BlockSummary::default();
                summary.parent_loop_cond = parent_loop_cond.map(str::to_string);
                summary.is_else = true;
                collect_behaviors(&alt, content, &mut summary);
                blocks.push(PlannedBlock {
                    natural_start: alt.start_position().row as u32 + 1,
                    natural_end: alt.end_position().row as u32 + 1,
                    kind: SlicePlanKind::Branch,
                    summary,
                });
                return;
            }
        }
    }
}

/// 循环块：循环头行独立成片（迭代控制是独立的层级行为）——for 三段式/
/// while 条件单独一片，不与任何语句混合。循环体的**全部直接语句**（声明/
/// 表达式/return/跳转，无论位于嵌套块之前或之后）独立成计算片并携带本循环
/// 条件上下文；体内嵌套的控制结构（if 链/循环/switch/条件编译）递归独立
/// 成片。DO-178C 原子性：嵌套控制流是不同层级行为（迭代控制 vs 迭代内
/// 子处理 vs 条件分支），不得用一条需求覆盖。循环收尾 `}` 行（do-while 的
/// `while (cond);` 条件行）不归属任何片。parent_loop_cond 为外层循环条件
/// （循环嵌套时），记录到嵌套片与计算片的 summary 供需求模板生成
/// "外层循环每次迭代中"的上下文。
fn plan_loop(node: &Node, content: &str, blocks: &mut Vec<PlannedBlock>, parent_loop_cond: Option<&str>) {
    let mut summary = BlockSummary::default();
    summary.parent_loop_cond = parent_loop_cond.map(str::to_string);
    let body = node.child_by_field_name("body");
    match node.kind() {
        "for_statement" => {
            // initializer（declaration）含结尾分号，去除以保持控制信息整洁
            summary.loop_init = node
                .child_by_field_name("initializer")
                .map(|n| node_text(&n, content).trim_end_matches(';').to_string());
            summary.loop_cond = node
                .child_by_field_name("condition")
                .map(|n| condition_text(&n, content));
            summary.loop_update = node
                .child_by_field_name("update")
                .map(|n| node_text(&n, content));
            // 循环头：语句起始到 body 起始之间的源码文本
            if let Some(b) = body {
                if let Some(raw) = content.get(node.start_byte()..b.start_byte()) {
                    summary.loop_header = Some(collapse_ws(raw));
                }
            }
        }
        "while_statement" | "do_statement" => {
            summary.loop_cond = node
                .child_by_field_name("condition")
                .map(|n| condition_text(&n, content));
        }
        _ => {}
    }
    let header_line = node.start_position().row as u32 + 1;
    // 循环头末行 = 头部源码（语句起始到 body 起始之间，去除尾部空白）的行跨度；
    // 单行头即起始行，多行头覆盖到条件所在行（`{` 同行时无法避开）
    let header_end = match body {
        Some(b) => {
            let raw = &content[node.start_byte()..b.start_byte()];
            header_line + raw.trim_end().matches('\n').count() as u32
        }
        None => node.end_position().row as u32 + 1,
    };

    // 1) 分类循环体直接子节点（保持源码顺序）：
    //    - 直接语句组（首个嵌套块前/嵌套块之间/之后）→ 独立计算片
    //    - 嵌套控制块 → 递归拆分
    enum Segment<'a> {
        Nested(Node<'a>),
        Tail { start: u32, end: u32, nodes: Vec<Node<'a>> },
    }
    let mut segments: Vec<Segment> = Vec::new();
    let mut tail: Option<(u32, u32, Vec<Node>)> = None;
    if let Some(b) = body {
        for i in 0..b.child_count() {
            let Some(child) = b.child(i) else { continue };
            match child.kind() {
                "{" | "}" | "comment" => {}
                "if_statement" | "for_statement" | "while_statement" | "do_statement"
                | "switch_statement" => {
                    if let Some((s, e, nodes)) = tail.take() {
                        segments.push(Segment::Tail { start: s, end: e, nodes });
                    }
                    segments.push(Segment::Nested(child));
                }
                k if k.starts_with("preproc_") => {
                    if let Some((s, e, nodes)) = tail.take() {
                        segments.push(Segment::Tail { start: s, end: e, nodes });
                    }
                    segments.push(Segment::Nested(child));
                }
                _ => {
                    let e = child.end_position().row as u32 + 1;
                    match &mut tail {
                        Some((_, te, nodes)) => {
                            nodes.push(child);
                            *te = (*te).max(e);
                        }
                        None => {
                            let s = child.start_position().row as u32 + 1;
                            tail = Some((s, e, vec![child]));
                        }
                    }
                }
            }
        }
    }
    if let Some((s, e, nodes)) = tail.take() {
        segments.push(Segment::Tail { start: s, end: e, nodes });
    }

    // 2) 循环片：仅循环头行（迭代控制独立成片，行为清单为空——循环体内的
    //    直接语句与嵌套行为由各自的片覆盖，避免一条需求重复覆盖多个层级）
    let self_cond = summary.loop_cond.clone();
    blocks.push(PlannedBlock {
        natural_start: header_line,
        natural_end: header_end,
        kind: SlicePlanKind::Loop,
        summary,
    });

    // 3) 嵌套块递归拆分 + 尾部直接语句组独立成片（带父循环上下文）；
    //    子块的父循环上下文 = 本循环的条件（直接父级）
    for seg in segments {
        match seg {
            Segment::Nested(n) => match n.kind() {
                "if_statement" => plan_if_chain(&n, content, blocks, self_cond.as_deref()),
                "for_statement" | "while_statement" | "do_statement" => {
                    plan_loop(&n, content, blocks, self_cond.as_deref())
                }
                "switch_statement" => plan_switch(&n, content, blocks, self_cond.as_deref()),
                _ => {
                    // 条件编译块整体一片（同函数体顶层规则）
                    let mut s = BlockSummary::default();
                    s.parent_loop_cond = self_cond.clone();
                    s.preproc_directive = first_line_text(&n, content);
                    collect_behaviors(&n, content, &mut s);
                    blocks.push(PlannedBlock {
                        natural_start: n.start_position().row as u32 + 1,
                        natural_end: n.end_position().row as u32 + 1,
                        kind: SlicePlanKind::Preproc,
                        summary: s,
                    });
                }
            },
            Segment::Tail { start, end, nodes } => {
                let mut s = BlockSummary::default();
                s.parent_loop_cond = self_cond.clone();
                for n in &nodes {
                    collect_behaviors(n, content, &mut s);
                }
                blocks.push(PlannedBlock {
                    natural_start: start,
                    natural_end: end,
                    kind: SlicePlanKind::Computation,
                    summary: s,
                });
            }
        }
    }
}

/// 拆分 switch：每个 case / default 一片。switch 头行（含条件表达式，
/// 是所有 case 需求的分派前提）归入首个 case 片；switch 的 '}' 收尾行
/// 不归属任何片。switch 位于循环体内时 parent_loop_cond 携带外层循环条件。
fn plan_switch(switch_node: &Node, content: &str, blocks: &mut Vec<PlannedBlock>, parent_loop_cond: Option<&str>) {
    let Some(body) = switch_node.child_by_field_name("body") else {
        return;
    };
    let switch_cond = switch_node
        .child_by_field_name("condition")
        .map(|c| condition_text(&c, content));
    let switch_start = switch_node.start_position().row as u32 + 1;
    let mut first = true;
    for i in 0..body.child_count() {
        let Some(child) = body.child(i) else { continue };
        if child.kind() == "case_statement" {
            let mut summary = BlockSummary::default();
            summary.parent_loop_cond = parent_loop_cond.map(str::to_string);
            summary.condition = switch_cond.clone();
            match child.child_by_field_name("value") {
                Some(v) => summary.case_value = Some(node_text(&v, content)),
                None => summary.is_else = true, // default 分支
            }
            collect_behaviors(&child, content, &mut summary);
            blocks.push(PlannedBlock {
                // 首个 case 片从 switch 头起始（switch 表达式是分派条件）
                natural_start: if first {
                    switch_start
                } else {
                    child.start_position().row as u32 + 1
                },
                natural_end: child.end_position().row as u32 + 1,
                kind: SlicePlanKind::Case,
                summary,
            });
            first = false;
        }
    }
}

/// 递归收集子树内的行为事实（赋值/初始化声明/return/调用/跳转）
fn collect_behaviors(node: &Node, content: &str, out: &mut BlockSummary) {
    match node.kind() {
        "assignment_expression" => {
            let op = node.child_by_field_name("operator");
            let is_plain = op
                .map(|o| node_text(&o, content) == "=")
                .unwrap_or(false);
            if is_plain {
                if let (Some(l), Some(r)) = (
                    node.child_by_field_name("left"),
                    node.child_by_field_name("right"),
                ) {
                    out.behaviors.push(Behavior::Assign {
                        lhs: node_text(&l, content),
                        rhs: node_text(&r, content),
                    });
                } else {
                    out.behaviors.push(Behavior::CompoundAssign {
                        text: node_text(node, content),
                    });
                }
            } else {
                out.behaviors.push(Behavior::CompoundAssign {
                    text: node_text(node, content),
                });
            }
        }
        "init_declarator" => {
            if let (Some(d), Some(v)) = (
                node.child_by_field_name("declarator"),
                node.child_by_field_name("value"),
            ) {
                out.behaviors.push(Behavior::Init {
                    var: node_text(&d, content),
                    value: node_text(&v, content),
                });
            }
        }
        "return_statement" => {
            out.behaviors.push(Behavior::Return {
                expr: node
                    .named_child(0)
                    .map(|c| node_text(&c, content))
                    .unwrap_or_default(),
            });
        }
        "call_expression" => {
            if let Some(f) = node.child_by_field_name("function") {
                out.behaviors.push(Behavior::Call {
                    name: node_text(&f, content),
                });
            }
        }
        "break_statement" => out.behaviors.push(Behavior::Break),
        "continue_statement" => out.behaviors.push(Behavior::Continue),
        _ => {}
    }
    for i in 0..node.named_child_count() {
        if let Some(child) = node.named_child(i) {
            collect_behaviors(&child, content, out);
        }
    }
}

/// 提取条件表达式文本：parenthesized_expression（if/while/switch 的 condition 字段）
/// 取内部表达式去括号，裸表达式（for 的 condition 字段）原样；空白归一
fn condition_text(cond: &Node, content: &str) -> String {
    let inner = if cond.kind() == "parenthesized_expression" {
        cond.named_child(0).unwrap_or(*cond)
    } else {
        *cond
    };
    node_text(&inner, content)
}

/// 节点源码文本（多行压缩为单行、连续空白压为一个空格）
fn node_text(node: &Node, content: &str) -> String {
    node.utf8_text(content.as_bytes())
        .map(collapse_ws)
        .unwrap_or_default()
}

/// 节点首行文本（如 "#ifdef DEBUG"）
fn first_line_text(node: &Node, content: &str) -> Option<String> {
    let raw = node.utf8_text(content.as_bytes()).ok()?;
    raw.lines().next().map(|l| collapse_ws(l))
}

/// 空白归一化：换行变空格、连续空白压为一个空格、去首尾空白
pub(crate) fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---------------------------------------------------------------------------
// 行范围代码快照
// ---------------------------------------------------------------------------

/// 行范围越界错误：携带请求范围与文件总行数
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineRangeError {
    pub start_line: i64,
    pub end_line: i64,
    pub total: i64,
}

impl std::fmt::Display for LineRangeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "行范围 L{}-L{} 超出文件行数（共 {} 行）",
            self.start_line, self.end_line, self.total
        )
    }
}

impl std::error::Error for LineRangeError {}

/// 从文件内容中提取 1-based 行范围 [start_line, end_line] 的文本快照。
/// 每行去除行尾 \r，以 \n 连接（不含结尾换行）。行范围越界返回 [`LineRangeError`]。
pub fn extract_lines(
    content: &str,
    start_line: i64,
    end_line: i64,
) -> Result<String, LineRangeError> {
    let lines: Vec<&str> = content.split('\n').collect();
    let total = lines.len() as i64;
    if start_line < 1 || end_line < start_line || end_line > total {
        return Err(LineRangeError { start_line, end_line, total });
    }
    let selected: Vec<String> = lines[(start_line - 1) as usize..end_line as usize]
        .iter()
        .map(|l| l.trim_end_matches('\r').to_string())
        .collect();
    Ok(selected.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slice_plan_kind_code_roundtrip() {
        for (code, kind) in [
            ("computation", SlicePlanKind::Computation),
            ("branch", SlicePlanKind::Branch),
            ("loop", SlicePlanKind::Loop),
            ("case", SlicePlanKind::Case),
            ("preproc", SlicePlanKind::Preproc),
        ] {
            assert_eq!(SlicePlanKind::from_code(code), kind);
            assert_eq!(kind.as_str(), code);
        }
        // 未知值（空串/旧数据）回退 Computation
        assert_eq!(SlicePlanKind::from_code(""), SlicePlanKind::Computation);
        assert_eq!(SlicePlanKind::from_code("unknown"), SlicePlanKind::Computation);
    }

    fn names(srcs: &[FunctionDef]) -> Vec<&str> {
        srcs.iter().map(|f| f.name.as_str()).collect()
    }

    #[test]
    fn basic_static_and_pointer_return_functions() {
        let src = r#"
int add(int a, int b) {
    return a + b;
}

static void hello(void) {
    printf("hi");
}

static int *foo(int a, char *b) {
    return 0;
}
"#;
        let funcs = parse_functions(src);
        assert_eq!(names(&funcs), vec!["add", "hello", "foo"]);
        for f in &funcs {
            assert!(f.signature.contains(f.name.as_str()), "签名应含函数名: {}", f.signature);
            assert!(f.signature.contains('('), "签名应含 '(': {}", f.signature);
            assert!(!f.signature.contains('{'), "签名不应含函数体 '{{': {}", f.signature);
        }
        assert!(funcs[2].signature.contains("static int *foo(int a, char *b)"));
    }

    #[test]
    fn multiline_signature() {
        let src = r#"
static int *foo(int a,
                char *b)
{
    return NULL;
}
"#;
        let funcs = parse_functions(src);
        assert_eq!(names(&funcs), vec!["foo"]);
        let sig = &funcs[0].signature;
        assert!(sig.contains("foo") && sig.contains('('));
        assert!(!sig.contains('{'));
        assert!(!sig.contains("return"), "签名不应含函数体内容: {}", sig);
        // 无首尾空白、无空行
        assert_eq!(sig.trim(), sig.as_str());
        assert!(!sig.lines().any(|l| l.trim().is_empty()), "签名不应含空行: {}", sig);
        // 多行签名被压缩为两行（去缩进、去行尾空白）
        assert_eq!(sig, "static int *foo(int a,\nchar *b)");
    }

    #[test]
    fn signature_with_blank_line_collapsed() {
        let src = "int foo(int a,\n\n        int b)\n{\n    return 0;\n}\n";
        let funcs = parse_functions(src);
        assert_eq!(names(&funcs), vec!["foo"]);
        let sig = &funcs[0].signature;
        assert!(!sig.lines().any(|l| l.trim().is_empty()), "多余空行应被去除: {}", sig);
        assert_eq!(sig, "int foo(int a,\nint b)");
    }

    #[test]
    fn body_with_comments_and_nested_braces() {
        let src = r#"
int process(int x) {
    /* 块注释 */
    // 行注释

    int acc = 0;
    if (x > 0) {
        for (int i = 0; i < x; i++) {
            acc += i;
        }
    }
    return acc;
}
"#;
        let funcs = parse_functions(src);
        assert_eq!(names(&funcs), vec!["process"]);
        let f = &funcs[0];
        assert_eq!(f.start_line, 2);
        assert_eq!(f.end_line, 13);
    }

    #[test]
    fn skips_non_function_constructs() {
        let src = r#"// 文件头注释
#include <stdio.h>
#include "local.h"

#define MAX(a, b) ((a) > (b) ? (a) : (b))
#define VERSION 100

typedef struct Point {
    int x;
    int y;
} PointT;

enum Color { RED, GREEN };

static int global_counter = 0;
extern int other_global;

int only_prototype(int a, char *b);

typedef int (*callback_t)(int, void *);

int real_function(int a) {
    return a + 1;
}
"#;
        let funcs = parse_functions(src);
        assert_eq!(names(&funcs), vec!["real_function"]);
    }

    #[test]
    fn ifdef_inside_function_body() {
        let src = r#"
void configure(void) {
#ifdef DEBUG
    log_debug();
#else
    log_release();
#endif
    return;
}
"#;
        let funcs = parse_functions(src);
        assert_eq!(names(&funcs), vec!["configure"]);
        let f = &funcs[0];
        assert_eq!(f.start_line, 2);
        assert_eq!(f.end_line, 9);
        assert!(f.signature.starts_with("void configure(void)"));
    }

    #[test]
    fn function_pointer_parameter() {
        let src = r#"
void cb(void (*f)(int)) {
    f(1);
}

int run(int (*handler)(int, int), int x) {
    return handler(x, x);
}
"#;
        let funcs = parse_functions(src);
        assert_eq!(names(&funcs), vec!["cb", "run"]);
        assert!(funcs[0].signature.contains("void (*f)(int)"));
        assert!(funcs[1].signature.contains("int (*handler)(int, int)"));
    }

    #[test]
    fn function_inside_preproc_if_block() {
        let src = r#"
#if defined(IMPLEMENT)
int enabled_fn(void) {
    return 1;
}
#endif

int always_there(void) {
    return 2;
}
"#;
        let funcs = parse_functions(src);
        assert_eq!(names(&funcs), vec!["enabled_fn", "always_there"]);
    }

    #[test]
    fn empty_and_comment_only() {
        assert!(parse_functions("").is_empty());
        assert!(parse_functions("   \n\t  \n").is_empty());
        let comment_only = r#"
// 只是注释
/* 块注释
   多行
*/
"#;
        assert!(parse_functions(comment_only).is_empty());
    }

    #[test]
    fn exact_line_numbers_and_signature() {
        let src = r#"// header
#include <stdio.h>

typedef struct Point {
    int x;
} PointT;

static int *foo(int a,
                char *b)
{
    if (a > 0) {
        return a;
    }
    return 0;
}

void bar(void) {
#ifdef DEBUG
    printf("x");
#endif
}
"#;
        let funcs = parse_functions(src);
        assert_eq!(names(&funcs), vec!["foo", "bar"]);

        let foo = &funcs[0];
        assert_eq!(foo.start_line, 8, "foo 起始行（static 所在行）");
        assert_eq!(foo.end_line, 15, "foo 结束行（右花括号所在行）");
        assert_eq!(foo.signature, "static int *foo(int a,\nchar *b)");

        let bar = &funcs[1];
        assert_eq!(bar.start_line, 17);
        assert_eq!(bar.end_line, 21);
        assert_eq!(bar.signature, "void bar(void)");
        // 签名不含函数体 '{' 及其后的任何内容
        assert!(!bar.signature.contains('{'));
        assert!(!bar.signature.contains("printf"));
    }

    #[test]
    fn syntax_error_tolerance() {
        let src = "
int broken(int a {
    return a;
}

int good(int x) {
    return x;
}
";
        let funcs = parse_functions(src);
        // 语法错误的文件不 panic，且仍能识别出格式完好的函数
        assert!(funcs.iter().any(|f| f.name == "good"), "应识别出 good: {:?}", names(&funcs));
        for f in &funcs {
            assert!(f.end_line >= f.start_line);
            assert!(f.signature.contains('('));
        }
    }

    // -----------------------------------------------------------------
    // 自动切片规划测试
    // -----------------------------------------------------------------

    /// 辅助：对 src 中的目标函数生成切片计划，返回 (起, 止, 类型) 列表
    fn plan(src: &str, name: &str) -> Vec<(u32, u32, &'static str)> {
        plan_items(src, name)
            .into_iter()
            .map(|i| (i.start_line, i.end_line, i.kind.as_str()))
            .collect()
    }

    /// 辅助：对 src 中的目标函数生成切片计划，返回完整计划项（含语义摘要）
    fn plan_items(src: &str, name: &str) -> Vec<SlicePlanItem> {
        let funcs = parse_functions(src);
        let f = funcs.iter().find(|f| f.name == name).expect("函数应存在");
        plan_function_slices(src, f.start_line, f.end_line).expect("应生成计划")
    }

    /// 辅助：断言切片计划符合精确行为边界不变量：
    /// 切片按行有序、互不重叠（允许片间未覆盖行——纯声明/注释/空行）、
    /// 首片起始于 expected_first、末片止于 expected_last（均为行为块边界行）。
    fn assert_full_coverage(src: &str, name: &str, expected_first: u32, expected_last: u32) {
        let funcs = parse_functions(src);
        let f = funcs.iter().find(|f| f.name == name).expect("函数应存在");
        let items = plan_function_slices(src, f.start_line, f.end_line).expect("应生成计划");
        assert!(
            items.iter().all(|i| i.start_line <= i.end_line),
            "起始行不应大于结束行: {items:?}"
        );
        assert_eq!(
            items[0].start_line, expected_first,
            "首片应起始于首个行为块的自然起始行"
        );
        assert_eq!(
            items.last().unwrap().end_line, expected_last,
            "末片应止于末个行为块的自然结束行"
        );
        for w in items.windows(2) {
            assert!(
                w[1].start_line > w[0].end_line,
                "切片不应重叠（片间允许未覆盖行）: {items:?}"
            );
        }
    }

    #[test]
    fn plan_mixed_control_flow() {
        let src = r#"int foo(int x) {
    int y = 0;
    int z = 1;

    if (x < 0) {
        return -1;
    }

    if (x > 100) {
        z = 2;
    } else {
        z = 3;
    }

    for (int i = 0; i < 10; i++) {
        y += i;
    }

    y = y * z;
    return y;
}
"#;
        // init y → init z → if → if/else（两分支各一片）→ for 头 → 循环内计算片 → 计算组 → return
        // 精确行为边界：空行 L4/L8/L14/L18 不归属任何片（片间未覆盖行）
        let items = plan(src, "foo");
        assert_eq!(items.len(), 9, "9 个逻辑块: {items:?}");
        assert_eq!(items[0], (2, 2, "computation"), "y 初始化声明独立成片: {items:?}");
        assert_eq!(items[1], (3, 3, "computation"), "z 初始化声明独立成片: {items:?}");
        assert_eq!(items[2], (5, 7, "branch"), "if 分支（前置空行不归属）: {items:?}");
        assert_eq!(items[3], (9, 11, "branch"), "if 片: {items:?}");
        assert_eq!(items[4], (12, 13, "branch"), "else 片（else 关键字行随 if 片）: {items:?}");
        assert_eq!(items[5], (15, 15, "loop"), "for 头独立成片（{{ 与头同行无法避开）: {items:?}");
        assert_eq!(items[6], (16, 16, "computation"), "循环体直接语句独立成计算片: {items:?}");
        assert_eq!(items[7], (19, 19, "computation"), "数据准备组（y = y * z）: {items:?}");
        assert_eq!(items[8], (20, 20, "computation"), "return 独立成片: {items:?}");
        assert_full_coverage(src, "foo", 2, 20);
    }

    #[test]
    fn plan_return_splits_computation_group() {
        // 用户的典型用例：声明 + 调用 + return 应切成两片（数据准备 / 返回结果）
        let src = "static uint32_t Core_ReadU32(const uint8_t* p)\n{\n    uint32_t v;\n    (void)memcpy(&v, p, sizeof(v));\n    return v;\n}\n";
        let items = plan(src, "Core_ReadU32");
        assert_eq!(items.len(), 2, "应切成数据准备与返回结果两片: {items:?}");
        // 精确行为边界：纯声明行 L3 不归属任何片
        assert_eq!(items[0], (4, 4, "computation"), "首片：仅 memcpy（纯声明行不归属）: {items:?}");
        assert_eq!(items[1], (5, 5, "computation"), "末片：return v: {items:?}");
        assert_full_coverage(src, "Core_ReadU32", 4, 5);
    }

    #[test]
    fn plan_return_in_middle_groups_following_statements() {
        // return 之后的语句（不可达代码）归新组，不混入 return 片
        let src = "int f(int x) {\n    int y = x + 1;\n    return y;\n    y = 0;\n}\n";
        let items = plan(src, "f");
        assert_eq!(items.len(), 3, "{items:?}");
        assert_eq!(items[0], (2, 2, "computation"));
        assert_eq!(items[1], (3, 3, "computation"), "return 独立成片: {items:?}");
        assert_eq!(items[2], (4, 4, "computation"), "return 后语句归新组: {items:?}");
        assert_full_coverage(src, "f", 2, 4);
    }

    #[test]
    fn plan_if_chain_splits_each_branch() {
        let src = r#"int grade(int s) {
    if (s >= 90) {
        return 4;
    } else if (s >= 80) {
        return 3;
    } else if (s >= 70) {
        return 2;
    } else {
        return 0;
    }
}
"#;
        let items = plan(src, "grade");
        // if → else-if → else-if → else 共 4 片分支（首片从函数体首行开始，末片延伸到函数尾）
        assert_eq!(items.len(), 4, "else-if 链应逐分支拆分: {items:?}");
        assert_eq!(items[0], (2, 4, "branch"), "if 片（else-if 行归 if 片）: {items:?}");
        assert_eq!(items[1], (5, 6, "branch"), "第一个 else-if: {items:?}");
        assert_eq!(items[2], (7, 8, "branch"), "第二个 else-if: {items:?}");
        assert_eq!(items[3], (9, 10, "branch"), "else 片: {items:?}");
        assert_full_coverage(src, "grade", 2, 10);
    }

    #[test]
    fn plan_switch_cases() {
        let src = r#"int op(int cmd, int v) {
    switch (cmd) {
    case 1:
        v += 1;
        break;
    case 2:
        v -= 1;
        break;
    default:
        v = 0;
        break;
    }
    return v;
}
"#;
        let items = plan(src, "op");
        // case1 / case2 / default / 计算组（return）
        assert_eq!(items.len(), 4, "switch 每个 case 一片: {items:?}");
        assert_eq!(items[0], (2, 5, "case"), "首片从函数体首行开始（含 switch 头，不含签名）: {items:?}");
        assert_eq!(items[1], (6, 8, "case"), "第二个 case: {items:?}");
        assert_eq!(items[2], (9, 11, "case"), "default: {items:?}");
        assert_eq!(items[3], (13, 13, "computation"), "return（switch 收尾行 L12 不归属）: {items:?}");
        assert_full_coverage(src, "op", 2, 13);
    }

    #[test]
    fn plan_nested_stays_in_parent_block() {
        let src = r#"int deep(int x) {
    if (x > 0) {
        if (x > 10) {
            return 2;
        }
        return 1;
    }
    return 0;
}
"#;
        let items = plan(src, "deep");
        // 分支体内的嵌套 if 不拆：随外层分支成片（循环嵌套才递归拆分），
        // 外层 if 一片（2-7，首片从函数体首行开始），末尾 return 归下一片
        assert_eq!(items.len(), 2, "嵌套 if 随外层分支成片: {items:?}");
        assert_eq!(items[0], (2, 7, "branch"));
        assert_eq!(items[1], (8, 8, "computation"));
        assert_full_coverage(src, "deep", 2, 8);
    }

    #[test]
    fn plan_nested_loop_splits_inner_structures() {
        // 用户用例：CRC32 位处理——外层循环内嵌套内层循环与 if/else。
        // DO-178C 原子性：嵌套控制流是不同层级行为，不得一条需求覆盖，
        // 外层循环 / 内层循环 / if 分支 / else 分支各自独立成片。
        let src = "static uint32_t Crc32(const uint8_t* data, uint32_t len)\n{\n    uint32_t crc = 0xFFFFFFFFu;\n    uint32_t i;\n    uint32_t k;\n    for (i = 0u; i < len; i++)\n    {\n        crc ^= (uint32_t)data[i];\n        for (k = 0; k < 8; k++)\n        {\n            if ((crc & 1u) != 0u)\n            {\n                crc = (crc >> 1) ^ 0xEDB88320u;\n            }\n            else\n            {\n                crc >>= 1;\n            }\n        }\n    }\n    return ~crc;\n}\n";
        let items = plan_items(src, "Crc32");
        assert_eq!(items.len(), 7, "嵌套结构应逐层拆分: {items:?}");
        let expect = [
            (3u32, 3u32, "computation", "crc 初始化声明"),
            (6, 6, "loop", "外层循环头独立成片"),
            (8, 8, "computation", "循环体直接语句 crc ^= data[i] 独立成计算片"),
            (9, 9, "loop", "内层循环头独立成片"),
            (11, 14, "branch", "if 分支"),
            (15, 18, "branch", "else 分支"),
            (21, 21, "computation", "return ~crc"),
        ];
        for (i, (s, e, k, msg)) in expect.iter().enumerate() {
            assert_eq!(
                (items[i].start_line, items[i].end_line, items[i].kind.as_str()),
                (*s, *e, *k),
                "{msg}: {items:?}"
            );
        }
        assert_full_coverage(src, "Crc32", 3, 21);
        // 嵌套片的父循环上下文：内层循环与循环内计算片携带外层循环条件
        assert_eq!(
            items[2].summary.parent_loop_cond.as_deref(),
            Some("i < len"),
            "循环内计算片应携带外层循环条件: {:?}",
            items[2].summary
        );
        assert_eq!(
            items[3].summary.parent_loop_cond.as_deref(),
            Some("i < len"),
            "内层循环应携带外层循环条件: {:?}",
            items[3].summary
        );
        // 循环片仅含头行：行为清单为空（直接语句/嵌套行为由各自的片覆盖）
        assert!(
            items[1].summary.behaviors.is_empty(),
            "外层循环片不应包含行为: {:?}",
            items[1].summary.behaviors
        );
    }

    #[test]
    fn plan_loop_tail_statements_after_nested_block() {
        // 嵌套块之后的直接语句段独立成计算片（带父循环条件上下文）
        let src = "int f(int n) {\n    int s = 0;\n    int j;\n    for (j = 0; j < n; j++)\n    {\n        if (j % 2 == 0)\n        {\n            s += j;\n        }\n        s = s + 1;\n    }\n    return s;\n}\n";
        let items = plan_items(src, "f");
        assert_eq!(items.len(), 5, "{items:?}");
        let expect = [
            (2u32, 2u32, "computation", "s 初始化"),
            (4, 4, "loop", "for 头（无头部直接语句）"),
            (6, 9, "branch", "if 分支"),
            (10, 10, "computation", "嵌套块后的直接语句独立成片"),
            (12, 12, "computation", "return"),
        ];
        for (i, (s, e, k, msg)) in expect.iter().enumerate() {
            assert_eq!(
                (items[i].start_line, items[i].end_line, items[i].kind.as_str()),
                (*s, *e, *k),
                "{msg}: {items:?}"
            );
        }
        assert_eq!(
            items[3].summary.parent_loop_cond.as_deref(),
            Some("j < n"),
            "尾部计算片应携带父循环条件: {:?}",
            items[3].summary
        );
        assert_full_coverage(src, "f", 2, 12);
    }

    #[test]
    fn plan_multiline_for_header_single_slice() {
        // 多行 for 头：循环片覆盖头的全部行（至条件/更新所在行），
        // 体语句独立成计算片；`{` 单独成行不归属任何片
        let src = "void f(int n) {\n    for (int i = 0;\n         i < n;\n         i++)\n    {\n        work(i);\n    }\n}\n";
        let items = plan_items(src, "f");
        assert_eq!(items.len(), 2, "{items:?}");
        assert_eq!(
            (items[0].start_line, items[0].end_line, items[0].kind.as_str()),
            (2, 4, "loop"),
            "循环片覆盖多行头的全部行: {items:?}"
        );
        assert_eq!(
            (items[1].start_line, items[1].end_line, items[1].kind.as_str()),
            (6, 6, "computation"),
            "体语句独立成计算片: {items:?}"
        );
    }

    #[test]
    fn plan_triple_nested_loops() {
        // 多层嵌套循环：每层独立成片（最内层含直接语句）
        let src = "void t(int n) {\n    for (int a = 0; a < n; a++)\n    {\n        for (int b = 0; b < n; b++)\n        {\n            for (int c = 0; c < n; c++)\n            {\n                work(a, b, c);\n            }\n        }\n    }\n}\n";
        let items = plan_items(src, "t");
        assert_eq!(items.len(), 4, "三层循环头各一片 + 最内层语句计算片: {items:?}");
        let expect = [
            (2u32, 2u32, "loop", "外层"),
            (4, 4, "loop", "中层"),
            (6, 6, "loop", "内层"),
            (8, 8, "computation", "最内层直接语句独立成计算片"),
        ];
        for (i, (s, e, k, msg)) in expect.iter().enumerate() {
            assert_eq!(
                (items[i].start_line, items[i].end_line, items[i].kind.as_str()),
                (*s, *e, *k),
                "{msg}: {items:?}"
            );
        }
        // 中层片携带外层循环条件，内层片携带中层循环条件，最内层计算片携带内层循环条件
        assert_eq!(items[1].summary.parent_loop_cond.as_deref(), Some("a < n"));
        assert_eq!(items[2].summary.parent_loop_cond.as_deref(), Some("b < n"));
        assert_eq!(items[3].summary.parent_loop_cond.as_deref(), Some("c < n"));
        assert_full_coverage(src, "t", 2, 8);
    }

    #[test]
    fn plan_empty_and_return_only() {
        let empty = "void nop(void) {\n}\n";
        let items = plan(empty, "nop");
        assert_eq!(items.len(), 1, "空函数体一片: {items:?}");
        // K&R 空函数：签名/'{' 行不参与切片，体内无内容行，退化覆盖 '}' 行
        assert_eq!(items[0], (2, 2, "computation"));

        let ret_only = "int one(void)\n{\n    return 1;\n}\n";
        let items = plan(ret_only, "one");
        assert_eq!(items.len(), 1, "仅 return 的函数一片: {items:?}");
        // 大括号换行风格：'{' '}' 行均不参与切片，仅覆盖 return 行
        assert_eq!(items[0], (3, 3, "computation"));
        assert_full_coverage(ret_only, "one", 3, 3);
    }

    #[test]
    fn plan_single_line_function() {
        // 单行函数：语句与签名同行，无法避开签名，整行一片
        let src = "int add(int a, int b) { return a + b; }\n";
        let items = plan(src, "add");
        assert_eq!(items.len(), 1, "{items:?}");
        assert_eq!(items[0], (1, 1, "computation"));
    }

    #[test]
    fn plan_local_declarations() {
        // 纯声明（int a;）不参与切片；带初始化声明（int b = 1;）独立成片
        let src = "int f(int x) {\n    int a;\n    int b = 1;\n    a = b + x;\n    return a;\n}\n";
        let items = plan(src, "f");
        assert_eq!(items.len(), 3, "{items:?}");
        // 精确行为边界：纯声明行 L2 不归属任何片，初始化声明 L3 独立成片
        assert_eq!(items[0], (3, 3, "computation"), "初始化声明独立成片: {items:?}");
        assert_eq!(items[1], (4, 4, "computation"), "计算组 a = b + x: {items:?}");
        assert_eq!(items[2], (5, 5, "computation"), "return 末片: {items:?}");
        assert_full_coverage(src, "f", 3, 5);

        // 函数体仅有纯声明：无行为块，兜底一片覆盖内容行（不含 '}' 行）
        let only_decl = "void g(void) {\n    int a;\n}\n";
        let items = plan(only_decl, "g");
        assert_eq!(items.len(), 1, "{items:?}");
        assert_eq!(items[0], (2, 2, "computation"));
    }

    #[test]
    fn plan_preproc_block_in_body() {
        let src = r#"void configure(void) {
    int a = 0;
#ifdef DEBUG
    log_debug();
#else
    log_release();
#endif
    a = 1;
}
"#;
        let items = plan(src, "configure");
        // 计算组（声明）→ preproc 块 → 计算组（a=1，末片延伸到函数尾）
        assert_eq!(items.len(), 3, "条件编译块独立成片: {items:?}");
        assert_eq!(items[0], (2, 2, "computation"));
        assert_eq!(items[1], (3, 7, "preproc"));
        assert_eq!(items[2], (8, 8, "computation"));
        assert_full_coverage(src, "configure", 2, 8);
    }

    #[test]
    fn plan_not_found_when_range_mismatch() {
        let src = "int foo(void) {\n    return 1;\n}\n";
        assert!(plan_function_slices(src, 1, 99).is_none(), "行范围不匹配应返回 None");
        assert!(plan_function_slices("", 1, 1).is_none(), "空内容返回 None");
    }

    #[test]
    fn plan_comments_not_covered() {
        // 精确行为边界：块间注释行不归属任何片（非行为行）
        let src = "int f(int x) {\n    /* 校验输入 */\n    if (x < 0) {\n        return -1;\n    }\n    return x;\n}\n";
        let items = plan(src, "f");
        assert_eq!(items.len(), 2, "{items:?}");
        assert_eq!(items[0], (3, 5, "branch"), "注释行 L2 不归属 if 片: {items:?}");
        assert_eq!(items[1], (6, 6, "computation"));
        assert_full_coverage(src, "f", 3, 6);
    }

    // -----------------------------------------------------------------
    // extract_lines（1-based 行范围代码快照）
    // -----------------------------------------------------------------

    #[test]
    fn extract_lines_basic() {
        let src = "line 1\nline 2\r\nline 3\n";
        // 首行、中行（去 \r）、末行（split 产生的空尾段使总行数含 4）
        assert_eq!(extract_lines(src, 1, 1).unwrap(), "line 1");
        assert_eq!(extract_lines(src, 2, 3).unwrap(), "line 2\nline 3");
        assert_eq!(extract_lines(src, 1, 3).unwrap(), "line 1\nline 2\nline 3");
        // 无结尾换行
        assert_eq!(extract_lines("a\nb", 2, 2).unwrap(), "b");
    }

    #[test]
    fn extract_lines_rejects_out_of_range() {
        let src = "a\nb\nc\n";
        // 起始行 < 1
        assert!(extract_lines(src, 0, 2).is_err());
        // end < start
        assert!(extract_lines(src, 2, 1).is_err());
        // 超出总行数（3 行内容 + 结尾换行的空段 = 4）
        assert!(extract_lines(src, 1, 5).is_err());
        let err = extract_lines(src, 1, 5).unwrap_err();
        assert_eq!(err.total, 4);
        assert!(err.to_string().contains("L1-L5"));
    }
}
