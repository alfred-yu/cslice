//! C 语言函数控制流图（CFG）提取（基于 tree-sitter / tree-sitter-c）。
//!
//! 面向结构化审阅：以可控可读的顺序把函数体拆成基本块，
//! 控制结构（if / else-if / else、for / while / do、switch / case / default）
//! 作为分支点，边携带条件/事件标签（是 / 否 / case 值 / default / 直落 /
//! break / continue / 退出）。节点与边标签只体现"做什么"和条件，不照抄源码。
//!
//! 节点均带 1-based 起止行，供前端点击定位到源码；节点类型 kind 供前端
//! 差异化着色，与现有切片规划（SlicePlanKind）的语义口径保持一致。

use std::collections::{HashMap, HashSet};

use tree_sitter::Node;

use super::{collapse_ws, find_function_by_range, identifier_name, new_parser};

/// CFG 节点类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CfgNodeKind {
    /// 函数入口
    Entry,
    /// 函数出口（返回语义汇聚点）
    Exit,
    /// 直落计算基本块（连续语句）
    Block,
    /// if / else-if 条件分支
    Branch,
    /// for / while / do-while 循环头
    Loop,
    /// switch 分派
    Switch,
    /// switch 的 case / default
    Case,
    /// return 节点
    Return,
    /// 汇合点（if / 循环 / switch 收口）
    Join,
}

impl CfgNodeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Entry => "entry",
            Self::Exit => "exit",
            Self::Block => "block",
            Self::Branch => "branch",
            Self::Loop => "loop",
            Self::Switch => "switch",
            Self::Case => "case",
            Self::Return => "return",
            Self::Join => "join",
        }
    }
}

/// CFG 节点（1-based 行范围，供前端点击定位到源码）
#[derive(Debug, Clone, PartialEq)]
pub struct CfgNode {
    pub id: u32,
    pub kind: CfgNodeKind,
    pub label: String,
    pub start_line: u32,
    pub end_line: u32,
}

/// CFG 有向边；label 携带条件/事件语义，style 是其视觉语义分类
/// （seq=顺序流 / false=条件不成立 / back=回边或跳出，见 `edge_style`）
#[derive(Debug, Clone, PartialEq)]
pub struct CfgEdge {
    pub from: u32,
    pub to: u32,
    pub label: Option<String>,
    pub style: &'static str,
}

/// 单个函数的控制流图
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CfgGraph {
    pub nodes: Vec<CfgNode>,
    pub edges: Vec<CfgEdge>,
}

/// 生成函数 CFG。按 1-based 起止行定位函数定义；找不到返回 None。
pub fn build_cfg(content: &str, func_start_line: u32, func_end_line: u32) -> Option<CfgGraph> {
    let mut parser = new_parser()?;
    let tree = parser.parse(content, None)?;
    let func = find_function_by_range(&tree.root_node(), func_start_line, func_end_line)?;
    Some(build_function_cfg(&func, content))
}

/// 循环/开关结构的上下文：break / continue 的目标节点
#[derive(Clone, Copy)]
struct JumpCtx {
    break_target: u32,
    continue_target: Option<u32>,
}

/// CFG 构建器：累计节点/边并分配自增 id
struct Cfg {
    nodes: Vec<CfgNode>,
    edges: Vec<CfgEdge>,
    next_id: u32,
    exit_id: u32,
}

impl Cfg {
    fn new() -> Self {
        Cfg {
            nodes: Vec::new(),
            edges: Vec::new(),
            next_id: 0,
            exit_id: u32::MAX,
        }
    }

    fn add(&mut self, kind: CfgNodeKind, label: String, start_line: u32, end_line: u32) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.nodes.push(CfgNode {
            id,
            kind,
            label,
            start_line,
            end_line,
        });
        id
    }

    /// 唯一的建边入口：style 在此按 label 分类一次，下游全部直读
    fn edge(&mut self, from: u32, to: u32, label: Option<String>) {
        let style = edge_style(label.as_deref());
        self.edges.push(CfgEdge {
            from,
            to,
            label,
            style,
        });
    }

    /// 多个前驱 → 多个目标（可选标签），用于将串行控制流接续起来
    fn link(&mut self, from: &[u32], to: &[u32], label: Option<String>) {
        for f in from {
            for t in to {
                self.edge(*f, *t, label.clone());
            }
        }
    }
}

/// 生成函数 CFG（已定位到函数节点；已持有语法树者用本函数，避免二次解析）
pub fn build_function_cfg(func: &Node, content: &str) -> CfgGraph {
    let mut g = Cfg::new();
    let s = func.start_position().row as u32 + 1;
    let e = func.end_position().row as u32 + 1;
    let fname = func
        .child_by_field_name("declarator")
        .and_then(|d| identifier_name(d, content))
        .unwrap_or_else(|| "function".to_string());
    let entry = g.add(CfgNodeKind::Entry, fname, s, s);
    let exit = g.add(CfgNodeKind::Exit, "返回".to_string(), e, e);
    g.exit_id = exit;

    let mut stack: Vec<JumpCtx> = Vec::new();
    let (entries, exits) = match func.child_by_field_name("body") {
        Some(b) => g.seq_stmt_node(&b, content, &mut stack),
        None => (Vec::new(), Vec::new()),
    };
    g.link(&[entry], &entries, None);
    if exits.is_empty() {
        g.edge(entry, exit, None);
    } else {
        g.link(&exits, &[exit], None);
    }

    CfgGraph {
        nodes: g.nodes,
        edges: g.edges,
    }
}

impl Cfg {
    /// 按顺序处理一组语句，返回 (入口节点, 出口节点)。
    /// 直落语句聚合成基本块；控制结构/跳转语句作为分支点。
    fn seq(
        &mut self,
        stmts: &[Node],
        content: &str,
        stack: &mut Vec<JumpCtx>,
    ) -> (Vec<u32>, Vec<u32>) {
        let mut entries: Vec<u32> = Vec::new();
        let mut tails: Vec<u32> = Vec::new();
        let mut pending: Option<(u32, Vec<Node>, u32)> = None;

        for stmt in stmts {
            let kind = stmt.kind();
            match kind {
                "if_statement" => {
                    flush_pending(self, &mut pending, &mut tails, &mut entries, content);
                    let (en, ex) = self.build_if(stmt, content, stack);
                    self.link(&tails, &en, None);
                    tails = ex;
                    if entries.is_empty() {
                        entries.extend(&en);
                    }
                }
                "for_statement" | "while_statement" | "do_statement" => {
                    flush_pending(self, &mut pending, &mut tails, &mut entries, content);
                    let (en, ex) = self.build_loop(stmt, content, stack);
                    self.link(&tails, &en, None);
                    tails = ex;
                    if entries.is_empty() {
                        entries.extend(&en);
                    }
                }
                "switch_statement" => {
                    flush_pending(self, &mut pending, &mut tails, &mut entries, content);
                    let (en, ex) = self.build_switch(stmt, content, stack);
                    self.link(&tails, &en, None);
                    tails = ex;
                    if entries.is_empty() {
                        entries.extend(&en);
                    }
                }
                "return_statement" => {
                    flush_pending(self, &mut pending, &mut tails, &mut entries, content);
                    let r = self.build_return(stmt, content);
                    self.link(&tails, &[r], None);
                    tails = Vec::new();
                    if entries.is_empty() {
                        entries.push(r);
                    }
                }
                "break_statement" => {
                    flush_pending(self, &mut pending, &mut tails, &mut entries, content);
                    if let Some(ctx) = stack.last() {
                        self.link(&tails, &[ctx.break_target], Some("break".to_string()));
                        tails = Vec::new();
                    }
                }
                "continue_statement" => {
                    flush_pending(self, &mut pending, &mut tails, &mut entries, content);
                    if let Some(ctx) = stack.last() {
                        if let Some(t) = ctx.continue_target {
                            self.link(&tails, &[t], Some("continue".to_string()));
                            tails = Vec::new();
                        }
                    }
                }
                _ => {
                    let st = stmt.start_position().row as u32 + 1;
                    let et = stmt.end_position().row as u32 + 1;
                    match &mut pending {
                        Some((_, nodes, end)) => {
                            nodes.push(*stmt);
                            *end = (*end).max(et);
                        }
                        None => pending = Some((st, vec![*stmt], et)),
                    }
                }
            }
        }
        flush_pending(self, &mut pending, &mut tails, &mut entries, content);
        (entries, tails)
    }

    /// 对函数体/分支体/循环体节点（compound 或单语句）生成子流
    fn seq_stmt_node(
        &mut self,
        node: &Node,
        content: &str,
        stack: &mut Vec<JumpCtx>,
    ) -> (Vec<u32>, Vec<u32>) {
        let stmts = if node.kind() == "compound_statement" {
            stmt_children(node)
        } else {
            vec![*node]
        };
        self.seq(&stmts, content, stack)
    }
}

/// 将聚合的直落语句组落成一个基本块节点，并把当前活尾接到该块
fn flush_pending(
    g: &mut Cfg,
    pending: &mut Option<(u32, Vec<Node>, u32)>,
    tails: &mut Vec<u32>,
    entries: &mut Vec<u32>,
    content: &str,
) {
    if let Some((st, nodes, et)) = pending.take() {
        let label = block_label(&nodes, content);
        let b = g.add(CfgNodeKind::Block, label, st, et);
        if tails.is_empty() {
            entries.push(b);
        } else {
            g.link(tails, &[b], None);
        }
        *tails = vec![b];
    }
}

/// 基本块标签：以"做什么"简述替代源码原文，多条时带数量提示
fn block_label(nodes: &[Node], content: &str) -> String {
    if nodes.is_empty() {
        return "…".to_string();
    }
    let first = stmt_summary(&nodes[0], content);
    if nodes.len() > 1 {
        format!("{first} …(+{})", nodes.len() - 1)
    } else {
        first
    }
}

/// 单条直落语句的语义摘要：声明只留「名字 = 初值」，调用加「调用」前缀，
/// 其余语句去掉结尾分号后保留表达式（流程图节点只体现"做什么"，不照抄源码）。
fn stmt_summary(node: &Node, content: &str) -> String {
    match node.kind() {
        "declaration" => decl_summary(node, content),
        "expression_statement" => match node.named_child(0) {
            Some(c) if c.kind() == "call_expression" => {
                format!("调用 {}", collapse_node(&c, content))
            }
            _ => stmt_text(node, content),
        },
        _ => stmt_text(node, content),
    }
}

/// 声明摘要：把 `int y = 0;` 归约为 `y = 0`，无初值时只留名字，多个声明用「、」连接
fn decl_summary(node: &Node, content: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    for i in 0..node.named_child_count() {
        let Some(ch) = node.named_child(i) else {
            continue;
        };
        match ch.kind() {
            "init_declarator" => {
                let name = ch
                    .child_by_field_name("declarator")
                    .and_then(|d| identifier_name(d, content))
                    .unwrap_or_default();
                let value = ch
                    .child_by_field_name("value")
                    .map(|v| collapse_node(&v, content))
                    .unwrap_or_default();
                parts.push(match (name.is_empty(), value.is_empty()) {
                    (false, false) => format!("{name} = {value}"),
                    (false, true) => name,
                    (true, false) => value,
                    (true, true) => continue,
                });
            }
            "identifier" => parts.push(collapse_node(&ch, content)),
            _ => {}
        }
    }
    if parts.is_empty() {
        stmt_text(node, content)
    } else {
        parts.join("、")
    }
}

/// 语句文本：压缩空白并去掉结尾分号
fn stmt_text(node: &Node, content: &str) -> String {
    collapse_node(node, content)
        .trim_end_matches(';')
        .trim()
        .to_string()
}

/// 条件标签：只保留条件表达式本身（形状已表达"判断 / 循环 / 分派"，无需再写 if / while / switch）
fn condition_label(node: &Node, content: &str, fallback: &str) -> String {
    node.child_by_field_name("condition")
        .map(|c| condition_text(&c, content))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

/// 取一组自左起计算 if / for / while / switch 的条件文本
fn condition_text(cond: &Node, content: &str) -> String {
    let inner = if cond.kind() == "parenthesized_expression" {
        cond.named_child(0).unwrap_or(*cond)
    } else {
        *cond
    };
    collapse_node(&inner, content)
}

impl Cfg {
    fn build_if(
        &mut self,
        if_node: &Node,
        content: &str,
        stack: &mut Vec<JumpCtx>,
    ) -> (Vec<u32>, Vec<u32>) {
        let s = if_node.start_position().row as u32 + 1;
        let e = if_node.end_position().row as u32 + 1;
        let b = self.add(
            CfgNodeKind::Branch,
            condition_label(if_node, content, "条件"),
            s,
            e,
        );

        // true 分支
        let (tin, tout) = match if_node.child_by_field_name("consequence") {
            Some(c) => self.seq_stmt_node(&c, content, stack),
            None => (Vec::new(), Vec::new()),
        };
        self.link(&[b], &tin, Some("是".to_string()));

        // false 分支（else-if 链递归 / else 块）
        let mut fout: Vec<u32> = Vec::new();
        let mut has_alt = false;
        if let Some(alt) = if_node.child_by_field_name("alternative") {
            has_alt = true;
            match alt.named_child(0) {
                Some(inner) if inner.kind() == "if_statement" => {
                    let (en, ex) = self.build_if(&inner, content, stack);
                    self.link(&[b], &en, Some("否".to_string()));
                    fout = ex;
                }
                _ => {
                    // alternative 是 else_clause 包装节点（含 else 关键字），
                    // 解包真实语句再递归，否则 else 体聚成不透明 Block，
                    // return / break / continue 的提前退出语义会丢失
                    let body = alt.named_child(0).unwrap_or(alt);
                    let (en, ex) = self.seq_stmt_node(&body, content, stack);
                    self.link(&[b], &en, Some("否".to_string()));
                    fout = ex;
                }
            }
        }

        // 汇合点
        let j = self.add(CfgNodeKind::Join, "合并".to_string(), s, e);
        for t in &tout {
            self.edge(*t, j, Some("直落".to_string()));
        }
        for t in &fout {
            self.edge(*t, j, Some("直落".to_string()));
        }
        if !has_alt {
            // 无 else：条件不成立直接落到汇合点
            self.edge(b, j, Some("否".to_string()));
        }
        (vec![b], vec![j])
    }

    fn build_loop(
        &mut self,
        node: &Node,
        content: &str,
        stack: &mut Vec<JumpCtx>,
    ) -> (Vec<u32>, Vec<u32>) {
        let s = node.start_position().row as u32 + 1;
        let e = node.end_position().row as u32 + 1;
        let body = node.child_by_field_name("body");
        let l = self.add(
            CfgNodeKind::Loop,
            condition_label(node, content, "循环"),
            s,
            loop_header_end(node, body, content),
        );
        let join = self.add(CfgNodeKind::Join, "循环出口".to_string(), s, e);

        stack.push(JumpCtx {
            break_target: join,
            continue_target: Some(l),
        });
        let (bin, bout) = match body {
            Some(b) => self.seq_stmt_node(&b, content, stack),
            None => (Vec::new(), Vec::new()),
        };
        stack.pop();

        match node.kind() {
            "for_statement" | "while_statement" => {
                // 条件成立进入体；不成立退出
                self.link(&[l], &bin, Some("是".to_string()));
            }
            "do_statement" => {
                // 先进入体一次，再由回边回到 l 重新检查条件
                self.link(&[l], &bin, None);
            }
            _ => {}
        }
        self.edge(l, join, Some("退出".to_string()));
        // 体回边：回到循环头重新检查条件
        for t in &bout {
            self.edge(*t, l, Some("循环".to_string()));
        }
        (vec![l], vec![join])
    }

    fn build_switch(
        &mut self,
        node: &Node,
        content: &str,
        stack: &mut Vec<JumpCtx>,
    ) -> (Vec<u32>, Vec<u32>) {
        let s = node.start_position().row as u32 + 1;
        let e = node.end_position().row as u32 + 1;
        let sw = self.add(
            CfgNodeKind::Switch,
            condition_label(node, content, "多路分派"),
            s,
            e,
        );
        let join = self.add(CfgNodeKind::Join, "switch 出口".to_string(), s, e);

        let outer_cont = stack.last().and_then(|c| c.continue_target);
        stack.push(JumpCtx {
            break_target: join,
            continue_target: outer_cont,
        });

        let mut case_nodes: Vec<Node> = Vec::new();
        if let Some(body) = node.child_by_field_name("body") {
            for i in 0..body.named_child_count() {
                if let Some(c) = body.named_child(i) {
                    if c.kind() == "case_statement" {
                        case_nodes.push(c);
                    }
                }
            }
        }

        let mut prev_exit: Option<Vec<u32>> = None;
        let mut prev_not_break = false;
        let last = case_nodes.len() as isize - 1;
        for (idx, cnode) in case_nodes.iter().enumerate() {
            let cs = cnode.start_position().row as u32 + 1;
            let ce = cnode.end_position().row as u32 + 1;
            let (clabel, is_default) = match cnode.child_by_field_name("value") {
                Some(v) => (format!("case {}", collapse_node(&v, content)), false),
                None => ("default".to_string(), true),
            };
            let c = self.add(CfgNodeKind::Case, clabel.clone(), cs, ce);
            self.edge(sw, c, Some(clabel.clone()));

            let skip = if is_default { 0 } else { 1 };
            let cstmts: Vec<Node> = (0..cnode.named_child_count())
                .filter_map(|i| {
                    let ch = cnode.named_child(i)?;
                    if (i as usize) < skip || ch.kind() == "comment" {
                        return None;
                    }
                    Some(ch)
                })
                .collect();
            let (cin, cout) = self.seq(&cstmts, content, stack);
            self.link(&[c], &cin, None);
            let ends_break = cstmts
                .last()
                .map(|n| n.kind() == "break_statement")
                .unwrap_or(false);

            // 上个 case 未以 break 结束 → 直落到本 case
            if let Some(prev) = &prev_exit {
                if prev_not_break {
                    for t in prev {
                        self.edge(*t, c, Some("直落".to_string()));
                    }
                }
            }

            if (idx as isize) == last {
                // 末 case 未 break → 落到 switch 出口
                if !ends_break {
                    for t in &cout {
                        self.edge(*t, join, Some("直落".to_string()));
                    }
                }
            } else {
                prev_exit = Some(cout);
                prev_not_break = !ends_break;
            }
        }
        if case_nodes.is_empty() {
            self.edge(sw, join, None);
        }
        stack.pop();
        (vec![sw], vec![join])
    }

    fn build_return(&mut self, stmt: &Node, content: &str) -> u32 {
        let s = stmt.start_position().row as u32 + 1;
        let e = stmt.end_position().row as u32 + 1;
        let expr = stmt
            .named_child(0)
            .map(|c| collapse_node(&c, content))
            .filter(|x| !x.is_empty())
            .unwrap_or_default();
        let label = if expr.is_empty() {
            "返回".to_string()
        } else {
            format!("返回 {expr}")
        };
        let r = self.add(CfgNodeKind::Return, label, s, e);
        self.edge(r, self.exit_id, None);
        r
    }
}

/// CFG 边标签 → 视觉语义分类（seq=顺序流 / false=条件不成立 / back=回边或跳出）。
/// 全工程唯一分类点：函数级边在 `Cfg::edge` 构造时分类一次，下游直读。
pub fn edge_style(label: Option<&str>) -> &'static str {
    match label {
        Some("否") | Some("否则") => "false",
        Some("循环") | Some("continue") | Some("break") => "back",
        _ => "seq",
    }
}

/// 复合语句的语句子节点（剔除大括号与注释）
fn stmt_children<'a>(body: &Node<'a>) -> Vec<Node<'a>> {
    (0..body.named_child_count())
        .filter_map(|i| {
            let c = body.named_child(i)?;
            match c.kind() {
                "{" | "}" | "comment" => None,
                _ => Some(c),
            }
        })
        .collect()
}

/// 循环头结束行（头部跨行时覆盖到体起始前一行）
fn loop_header_end(node: &Node, body: Option<Node>, content: &str) -> u32 {
    let start = node.start_position().row as u32 + 1;
    match body {
        Some(b) => {
            let raw = content.get(node.start_byte()..b.start_byte()).unwrap_or("");
            start + raw.trim_end().matches('\n').count() as u32
        }
        None => node.end_position().row as u32 + 1,
    }
}

/// 压缩节点源码文本
fn collapse_node(node: &Node, content: &str) -> String {
    node.utf8_text(content.as_bytes())
        .map(collapse_ws)
        .unwrap_or_default()
}

/// 判定分支某一侧是否恒提前退出：从该侧入口出发的所有路径都不落到本 if 的
/// 汇合点（true_side=true 沿「是」边进入真分支，false 沿「否」边进入否则分支）。
/// return / break / continue 都会使路径离开本 if 的汇合流，均视为提前退出。
pub fn branch_side_always_exits(
    cfg: &CfgGraph,
    branch_id: u32,
    join_id: u32,
    true_side: bool,
) -> bool {
    let label = if true_side { "是" } else { "否" };
    let mut stack: Vec<u32> = cfg
        .edges
        .iter()
        .filter(|e| e.from == branch_id && e.label.as_deref() == Some(label))
        .map(|e| e.to)
        .collect();
    let mut seen: HashSet<u32> = HashSet::new();
    while let Some(id) = stack.pop() {
        if id == join_id {
            return false; // 该侧存在落到汇合点的放行路径
        }
        if !seen.insert(id) {
            continue;
        }
        for e in &cfg.edges {
            if e.from == id {
                stack.push(e.to);
            }
        }
    }
    true
}

/// 卫语句判定：若该 if 的真/假侧之一恒提前退出、永不落到汇合点，
/// 返回其后同作用域代码的执行条件文本——
/// 真侧恒退出 → `!(cond)`（假侧放行）；假侧恒退出（else 恒退出）→ `cond`（真侧放行）。
/// 两侧都放行（普通 if / if-else）或都不可达（后续不可达）→ None。
pub fn guard_condition(cfg: &CfgGraph, branch_id: u32, join_id: u32) -> Option<String> {
    let cond = cfg.nodes.iter().find(|n| n.id == branch_id)?.label.clone();
    if branch_side_always_exits(cfg, branch_id, join_id, true) {
        Some(format!("!({cond})"))
    } else if branch_side_always_exits(cfg, branch_id, join_id, false) {
        Some(cond)
    } else {
        None
    }
}

/// 按 if 语句的 1-based 起止行定位其 CFG 分支/汇合节点对
/// （顶层 if 的 Branch/Join 节点与语句同跨度，可唯一对应）。
pub fn if_branch_join(cfg: &CfgGraph, if_start_line: u32, if_end_line: u32) -> Option<(u32, u32)> {
    let branch = cfg.nodes.iter().find(|n| {
        n.kind == CfgNodeKind::Branch && n.start_line == if_start_line && n.end_line == if_end_line
    })?;
    let join = cfg.nodes.iter().find(|n| {
        n.kind == CfgNodeKind::Join
            && n.start_line == branch.start_line
            && n.end_line == branch.end_line
    })?;
    Some((branch.id, join.id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(src: &str, name: &str) -> (u32, u32) {
        let funcs = super::super::parse_functions(src);
        let f = funcs.iter().find(|f| f.name == name).expect("函数应存在");
        (f.start_line, f.end_line)
    }

    fn cfg_of(src: &str, name: &str) -> CfgGraph {
        let (s, e) = find(src, name);
        build_cfg(src, s, e).expect("应生成 CFG")
    }

    #[test]
    fn cfg_linear_and_return() {
        let src = "int add(int a, int b) {\n    int c = a + b;\n    return c;\n}\n";
        let g = cfg_of(src, "add");
        // entry / block / return / exit
        let kinds: Vec<&str> = g.nodes.iter().map(|n| n.kind.as_str()).collect();
        assert!(kinds.contains(&"entry"));
        assert!(kinds.contains(&"block"));
        assert!(kinds.contains(&"return"));
        assert!(kinds.contains(&"exit"));
        // return → exit
        let r = g
            .nodes
            .iter()
            .find(|n| n.kind == CfgNodeKind::Return)
            .unwrap();
        let x = g
            .nodes
            .iter()
            .find(|n| n.kind == CfgNodeKind::Exit)
            .unwrap();
        assert!(g.edges.iter().any(|e| e.from == r.id && e.to == x.id));
    }

    #[test]
    fn cfg_if_has_join_when_no_else() {
        let src = "int foo(int x) {\n    int y = 0;\n    if (x < 0) {\n        y = -1;\n    }\n    y = y + 1;\n    return y;\n}\n";
        let g = cfg_of(src, "foo");
        // 无 else 的 if：条件不成立应走一条标签为“否”的假边到汇合点
        let br = g
            .nodes
            .iter()
            .find(|n| n.kind == CfgNodeKind::Branch)
            .unwrap();
        assert!(
            g.edges
                .iter()
                .any(|e| e.from == br.id && e.label.as_deref() == Some("否")),
            "无 else 时应有一条假边到汇合点: {:?}",
            g.edges
        );
    }

    #[test]
    fn cfg_loop_has_back_edge_and_exit() {
        let src = "int sum(int n) {\n    int s = 0;\n    for (int i = 0; i < n; i++) {\n        s += i;\n    }\n    return s;\n}\n";
        let g = cfg_of(src, "sum");
        let l = g
            .nodes
            .iter()
            .find(|n| n.kind == CfgNodeKind::Loop)
            .unwrap();
        let join = g
            .nodes
            .iter()
            .find(|n| n.kind == CfgNodeKind::Join)
            .unwrap();
        assert!(g
            .edges
            .iter()
            .any(|e| e.from == l.id && e.to == join.id && e.label.as_deref() == Some("退出")));
        // 循环体内计算块有回边到循环头
        assert!(
            g.edges
                .iter()
                .any(|e| e.from == l.id && e.label.is_some() && e.to != join.id),
            "循环头应有条件边进入循环体"
        );
        let _ = join;
    }

    #[test]
    fn cfg_labels_are_semantic_not_source() {
        let src = "int foo(int x) {\n    int y = 0;\n    if (x < 0 && flag != 0) {\n        y = -1;\n    }\n    for (int i = 0; i < n; i++) {\n        bar(y);\n    }\n    switch (x) {\n        case 1:\n            y = 2;\n            break;\n        default:\n            y = 3;\n    }\n    return y;\n}\n";
        let g = cfg_of(src, "foo");
        let label_of = |k: CfgNodeKind| {
            g.nodes
                .iter()
                .find(|n| n.kind == k)
                .map(|n| n.label.clone())
                .unwrap_or_default()
        };
        // 声明只留「名字 = 初值」，不带类型与分号
        assert!(
            g.nodes
                .iter()
                .any(|n| n.kind == CfgNodeKind::Block && n.label == "y = 0"),
            "声明应归约为 y = 0: {:?}",
            g.nodes
        );
        // 条件节点只保留条件表达式本身
        assert_eq!(label_of(CfgNodeKind::Branch), "x < 0 && flag != 0");
        assert_eq!(label_of(CfgNodeKind::Loop), "i < n");
        assert_eq!(label_of(CfgNodeKind::Switch), "x");
        // 返回改中文，调用加「调用」前缀
        assert_eq!(label_of(CfgNodeKind::Return), "返回 y");
        assert!(
            g.nodes
                .iter()
                .any(|n| n.kind == CfgNodeKind::Block && n.label.starts_with("调用 bar(y)")),
            "调用语句应加「调用」前缀: {:?}",
            g.nodes
        );
        // 真边标签统一为「是」
        let br = g
            .nodes
            .iter()
            .find(|n| n.kind == CfgNodeKind::Branch)
            .unwrap();
        assert!(
            g.edges
                .iter()
                .any(|e| e.from == br.id && e.label.as_deref() == Some("是")),
            "if 真边应标注「是」: {:?}",
            g.edges
        );
    }

    // -----------------------------------------------------------------
    // 边视觉语义分类（edge_style）
    // -----------------------------------------------------------------

    #[test]
    fn edge_style_classifies_label_vocabulary() {
        // false：条件不成立分支
        assert_eq!(edge_style(Some("否")), "false");
        assert_eq!(edge_style(Some("否则")), "false");
        // back：循环回边与跳出
        for label in ["循环", "continue", "break"] {
            assert_eq!(edge_style(Some(label)), "back", "label={label}");
        }
        // seq：真分支 / case 值 / default / 直落 / 退出 / 无标签等其余全部
        for label in ["是", "case 1", "default", "直落", "退出"] {
            assert_eq!(edge_style(Some(label)), "seq", "label={label}");
        }
        assert_eq!(edge_style(None), "seq");
    }

    #[test]
    fn cfg_edges_carry_style_at_construction() {
        // 建边入口即分类：style 随 CfgEdge 一起产生，下游无需再解析 label
        let src = "int f(int n) {\n    int s = 0;\n    while (n > 0) {\n        if (n % 2 == 0) {\n            s += n;\n        } else {\n            s -= 1;\n        }\n        n--;\n    }\n    return s;\n}\n";
        let g = cfg_of(src, "f");
        assert!(
            g.edges
                .iter()
                .any(|e| e.style == "false"
                    && matches!(e.label.as_deref(), Some("否") | Some("否则"))),
            "假边应带 style=false: {:?}",
            g.edges
        );
        assert!(
            g.edges
                .iter()
                .any(|e| e.label.as_deref() == Some("是") && e.style == "seq"),
            "真边应带 style=seq: {:?}",
            g.edges
        );
        assert!(
            g.edges
                .iter()
                .any(|e| e.label.as_deref() == Some("循环") && e.style == "back"),
            "循环回边应带 style=back: {:?}",
            g.edges
        );
        // 顺序流（直落/退出/无标签/真边）不得被误分类
        assert!(
            g.edges.iter().filter(|e| e.style == "seq").count() >= 3,
            "应存在多条顺序流边: {:?}",
            g.edges
        );
    }
}
