//! 需求草稿模板生成器（离线基线）—— cslice 的可选领域能力。
//!
//! 从 [`cslice_core::SlicePlanItem`]（切片类型 + 语义摘要）生成无歧义、可验证的
//! 需求草稿：
//! - 句式规范：中文"应"/英文 shall 句式表述强制性要求
//! - 无歧义：条件与行为均引用代码中的具体标识符/表达式，不使用主观词
//! - 可验证：描述与代码行为一致（逆向需求场景）
//! - 原子性：一个切片一条需求；多行为用编号清单（超限由上游工具提示拆分）
//! 语言由调用方传入（"zh" 中文 / "en" 英文），非法值按中文处理。
//!
//! 句式由内置默认模板表决定（见 [`templates`] 模块）。模板只负责填空；拼接顺序、
//! 英文首字母小写、编号清单格式、Call 去重、Break/Continue 守卫、空兜底等
//! 编排逻辑留在本模块。
//!
//! 本 crate 只依赖 cslice-core，且不提供任何 LLM/网络能力——AI 增强属调用方。

use cslice_core::{Behavior, BlockSummary, SlicePlanItem, SlicePlanKind};

pub mod lint;
pub mod templates;

/// 需求草稿：描述/验证方法两要素。
/// 需求不设独立标题——展示以 description 为准，
/// 需要标题形态的场合从 description 派生。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReqDraft {
    pub description: String,
    pub verify_method: String,
}

/// 对单个切片计划项生成需求草稿。
///
/// - 循环体嵌套拆分产生的片（内层循环/循环内分支/尾部计算片）的
///   `summary.parent_loop_cond` 携带直接父循环条件，描述加入
///   "（外层）循环每次迭代中"上下文，保证需求在其执行语境下无歧义；
/// - 条件（守卫条件按序 + if 分支条件）统一**后置**：
///   单一简单条件行内（"… if {cond}." / "…，当 {cond} 时。"）；
///   多条件、复合条件（含顶层 `||`/`&&`）或多行为清单时用结构化条件块——
///   "when:" / "当：" 引出编号清单，顶层项以 -AND- 连接，复合项拆 1a/1b
///   子项加括号并以 -AND-/-OR- 连接（-AND- 标识且，-OR- 标识或）。
pub fn generate_draft(func_name: &str, item: &SlicePlanItem, language: &str) -> ReqDraft {
    let mut conditions: Vec<String> = item.guard_conds.clone();
    let (draft, is_list) = generate_from_parts(
        func_name,
        item.kind,
        &item.summary,
        item.start_line,
        item.end_line,
        language,
        &mut conditions,
    );
    attach_conditions(draft, &conditions, is_list, language)
}

/// 兜底草稿：语义提取失败（空函数体/无法解析的行为）时使用。
/// `kind` 传 [`SlicePlanKind::from_code`] 的还原结果。
pub fn fallback_draft(
    func_name: &str,
    kind: SlicePlanKind,
    start_line: u32,
    end_line: u32,
    language: &str,
) -> ReqDraft {
    let en = language == "en";
    ReqDraft {
        description: templates::render(
            templates::lookup(language, "frame.fallback"),
            &[
                ("function_name", func_name),
                ("start_line", start_line.to_string().as_str()),
                ("end_line", end_line.to_string().as_str()),
            ],
        ),
        verify_method: verify_method_for(kind, en).into(),
    }
}

/// 按切片类型分派的草稿生成主体（与切片行范围解耦，兜底文案需要行范围）。
/// Branch 的条件表达式不进句子，追加进 `conditions` 由顶层统一后置渲染。
/// 返回 (草稿, 是否为多行为编号清单)。
fn generate_from_parts(
    func_name: &str,
    kind: SlicePlanKind,
    summary: &BlockSummary,
    start_line: u32,
    end_line: u32,
    language: &str,
    conditions: &mut Vec<String>,
) -> (ReqDraft, bool) {
    let phrases = behavior_phrases(summary, kind, language);
    let is_list = phrases.len() > 1;

    let draft = match kind {
        SlicePlanKind::Branch => {
            let d = branch_draft(func_name, summary, &phrases, language);
            if let Some(cond) = &summary.condition {
                conditions.push(cond.clone());
            }
            with_parent_context(d, summary, language)
        }
        SlicePlanKind::Loop => loop_draft(func_name, summary, language),
        SlicePlanKind::Case => {
            let d = case_draft(func_name, summary, &phrases, language);
            with_parent_context(d, summary, language)
        }
        SlicePlanKind::Preproc => preproc_draft(func_name, summary, language),
        SlicePlanKind::Computation => {
            computation_draft(func_name, summary, &phrases, start_line, end_line, language)
        }
    };
    (draft, is_list)
}

/// 条件后置：单一简单条件行内；多条件、复合条件或多行为清单用结构化
/// "when:"/"当：" 块（编号项 -AND- 连接，复合项拆 1a/1b 子项加括号）。
fn attach_conditions(
    draft: ReqDraft,
    conditions: &[String],
    is_list: bool,
    language: &str,
) -> ReqDraft {
    if conditions.is_empty() {
        return draft;
    }
    let en = language == "en";
    let compound = conditions.iter().any(|c| split_top_level(c).is_some());
    let base = draft
        .description
        .trim_end_matches(['.', '。'])
        .to_string();
    let description = if conditions.len() == 1 && !compound && !is_list {
        // 单一简单条件：行内
        let cond = &conditions[0];
        if en {
            format!("{base} if {cond}.")
        } else {
            format!("{base}，当 {cond} 时。")
        }
    } else {
        let intro = if en { " when:" } else { "，当：" };
        format!("{base}{intro}\n{}", render_condition_block(conditions))
    };
    ReqDraft { description, verify_method: draft.verify_method }
}

/// 结构化条件块：顶层项按序编号（1. 2. …），项间 -AND- 连接（守卫与分支
/// 条件是合取关系）；含顶层 `||`/`&&` 的项拆为 1a/1b 子项加括号，
/// 连接符 -OR-/-AND- 反映实际逻辑。
fn render_condition_block(conditions: &[String]) -> String {
    let items: Vec<String> = conditions
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let n = i + 1;
            match split_top_level(c) {
                None => format!("{n}.{c}"),
                Some((parts, is_or)) => {
                    let op = if is_or { "-OR-" } else { "-AND-" };
                    // 首子项紧跟 "1.("，其余子项与连接符同缩进（4 空格），"   )" 收括号
                    let subs: Vec<String> = parts
                        .iter()
                        .enumerate()
                        .map(|(j, p)| {
                            let label = format!("{}{}.{}", n, sub_letter(j), p);
                            if j == 0 {
                                label
                            } else {
                                format!("    {label}")
                            }
                        })
                        .collect();
                    format!("{n}.({}\n   )", subs.join(&format!("\n    {op}\n")))
                }
            }
        })
        .collect();
    items.join("\n-AND-\n")
}

/// 子项编号字母：1a、1b、1c …
fn sub_letter(j: usize) -> char {
    (b'a' + j as u8) as char
}

/// 按 0 层括号深度拆分条件表达式：优先按 `||`（析取，-OR-），
/// 无 0 层 `||` 时按 `&&`（合取，-AND-）。原子条件返回 None。
fn split_top_level(cond: &str) -> Option<(Vec<String>, bool)> {
    let bytes = cond.as_bytes();
    let mut depth = 0i32;
    let mut or_pos: Vec<usize> = Vec::new();
    let mut and_pos: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            b'|' if depth == 0 && i + 1 < bytes.len() && bytes[i + 1] == b'|' => {
                or_pos.push(i);
                i += 1;
            }
            b'&' if depth == 0 && i + 1 < bytes.len() && bytes[i + 1] == b'&' => {
                and_pos.push(i);
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    if !or_pos.is_empty() {
        Some((split_at(cond, &or_pos), true))
    } else if !and_pos.is_empty() {
        Some((split_at(cond, &and_pos), false))
    } else {
        None
    }
}

/// 按运算符位置（0 层深度）切分并修剪各段空白
fn split_at(cond: &str, pos: &[usize]) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    let mut start = 0usize;
    for &p in pos {
        parts.push(cond[start..p].trim().to_string());
        start = p + 2;
    }
    parts.push(cond[start..].trim().to_string());
    parts
}

/// 前缀 + 句子拼接：英文句子首字母小写（与既有句式一致），中文直接拼接
fn join_prefixed(prefix: &str, sentence: String, en: bool) -> String {
    if en {
        format!("{}{}", prefix, lowercase_first(&sentence))
    } else {
        format!("{prefix}{sentence}")
    }
}

/// 单行为直述 / 多行为编号清单（无前缀形态）
fn single_or_list(func_name: &str, phrases: &[String], language: &str) -> String {
    if phrases.len() == 1 {
        templates::render(
            templates::lookup(language, "frame.single"),
            &[("function_name", func_name), ("behavior", &phrases[0])],
        )
    } else {
        let numbered = join_numbered(phrases, language == "en");
        templates::render(
            templates::lookup(language, "frame.list"),
            &[
                ("function_name", func_name),
                ("numbered", numbered.as_str()),
            ],
        )
    }
}

/// 循环体内分支/选择片描述加父循环上下文前缀
fn with_parent_context(mut draft: ReqDraft, summary: &BlockSummary, language: &str) -> ReqDraft {
    let Some(pc) = summary
        .parent_loop_cond
        .as_deref()
        .filter(|s| !s.is_empty())
    else {
        return draft;
    };
    let prefix = templates::render(
        templates::lookup(language, "frame.loop_prefix"),
        &[("parent_loop_cond", pc)],
    );
    draft.description = join_prefixed(&prefix, draft.description, language == "en");
    draft
}

/// 英文首字母小写（上下文前缀拼接用）
fn lowercase_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_lowercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// 计算片草稿：循环体嵌套拆分产生的尾部计算片（带 parent_loop_cond）
/// 描述加循环上下文；普通计算片沿用原有句式
fn computation_draft(
    func_name: &str,
    summary: &BlockSummary,
    phrases: &[String],
    start_line: u32,
    end_line: u32,
    language: &str,
) -> ReqDraft {
    let en = language == "en";
    let pc = summary
        .parent_loop_cond
        .as_deref()
        .filter(|s| !s.is_empty());
    if let Some(pc) = pc {
        // 循环内计算片（嵌套块之后的直接语句）
        if !phrases.is_empty() {
            let prefix = templates::render(
                templates::lookup(language, "frame.loop_prefix"),
                &[("parent_loop_cond", pc)],
            );
            return ReqDraft {
                description: join_prefixed(
                    &prefix,
                    single_or_list(func_name, phrases, language),
                    en,
                ),
                verify_method: test_verify(en).into(),
            };
        }
    }
    // 普通计算片（无父循环上下文）
    if phrases.is_empty() {
        fallback_draft(
            func_name,
            SlicePlanKind::Computation,
            start_line,
            end_line,
            language,
        )
    } else {
        ReqDraft {
            description: single_or_list(func_name, phrases, language),
            verify_method: test_verify(en).into(),
        }
    }
}

/// 分支块草稿：有条件分支（if / else-if）与无条件分支（else）。
/// 条件表达式不进句子——由顶层 [`attach_conditions`] 统一后置渲染
/// （行内或结构化 when: 块）；else 无条件表达式，保留前导句式。
fn branch_draft(
    func_name: &str,
    summary: &BlockSummary,
    phrases: &[String],
    language: &str,
) -> ReqDraft {
    let en = language == "en";
    let prefix: Option<String> = if summary.is_else {
        Some(templates::lookup(language, "frame.else_prefix").to_string())
    } else {
        None
    };

    let sentence: Option<String> = if phrases.is_empty() {
        // 有引导语（else/条件）但无行为事实：执行分支内处理逻辑；否则交由兜底
        if summary.is_else || summary.condition.is_some() {
            Some(templates::render(
                templates::lookup(language, "frame.branch_empty"),
                &[("function_name", func_name)],
            ))
        } else {
            None
        }
    } else {
        Some(single_or_list(func_name, phrases, language))
    };

    match sentence {
        Some(s) => {
            let description = match &prefix {
                Some(p) => join_prefixed(p, s, en),
                None => s,
            };
            ReqDraft {
                description,
                verify_method: test_verify(en).into(),
            }
        }
        None => fallback_draft(func_name, SlicePlanKind::Branch, 0, 0, language),
    }
}

/// 循环块草稿：循环片仅含循环头行（迭代控制），描述循环条件与 for 三段式
/// 控制信息，不描述循环体行为（体内直接语句与嵌套结构由各自的片覆盖）；
/// 内层循环（parent_loop_cond 非空）描述加外层循环上下文
fn loop_draft(func_name: &str, summary: &BlockSummary, language: &str) -> ReqDraft {
    let en = language == "en";
    let cond = summary.loop_cond.as_deref().filter(|s| !s.is_empty());
    let pc = summary
        .parent_loop_cond
        .as_deref()
        .filter(|s| !s.is_empty());

    // for 三段式控制说明：三段齐备才渲染，填进 frame.loop 的 {loop_control} 槽；
    // 缺段（如 for(;;)）为空串
    let control = match (&summary.loop_init, &summary.loop_cond, &summary.loop_update) {
        (Some(i), Some(c), Some(u)) => templates::render(
            templates::lookup(language, "frame.loop_control"),
            &[
                ("loop_init", i.as_str()),
                ("loop_cond", c.as_str()),
                ("loop_update", u.as_str()),
            ],
        ),
        _ => String::new(),
    };

    let sentence = match cond {
        Some(c) => templates::render(
            templates::lookup(language, "frame.loop"),
            &[
                ("function_name", func_name),
                ("loop_cond", c),
                ("loop_control", control.as_str()),
            ],
        ),
        None => templates::render(
            templates::lookup(language, "frame.loop_nocond"),
            &[("function_name", func_name)],
        ),
    };

    // 外层循环上下文前缀：内层循环需求在其执行语境下描述
    let description = match pc {
        Some(p) => {
            let prefix = templates::render(
                templates::lookup(language, "frame.outer_loop_prefix"),
                &[("parent_loop_cond", p)],
            );
            join_prefixed(&prefix, sentence, en)
        }
        None => sentence,
    };

    ReqDraft {
        description,
        verify_method: test_verify(en).into(),
    }
}

/// switch case 块草稿：引用 switch 条件与 case 取值
fn case_draft(
    func_name: &str,
    summary: &BlockSummary,
    phrases: &[String],
    language: &str,
) -> ReqDraft {
    let en = language == "en";
    let switch_cond = summary.condition.as_deref().unwrap_or(if en {
        "the selection expression"
    } else {
        "选择表达式"
    });
    let prefix = match summary.case_value.as_deref() {
        Some(v) => templates::render(
            templates::lookup(language, "frame.case_eq"),
            &[("condition", switch_cond), ("case_value", v)],
        ),
        None => templates::render(
            templates::lookup(language, "frame.case_default"),
            &[("condition", switch_cond)],
        ),
    };

    let sentence = if phrases.is_empty() {
        templates::render(
            templates::lookup(language, "frame.branch_empty"),
            &[("function_name", func_name)],
        )
    } else {
        single_or_list(func_name, phrases, language)
    };

    ReqDraft {
        description: join_prefixed(&prefix, sentence, en),
        verify_method: test_verify(en).into(),
    }
}

/// 条件编译块草稿：审查导向（编译配置一致性需人工确认）
fn preproc_draft(func_name: &str, summary: &BlockSummary, language: &str) -> ReqDraft {
    let en = language == "en";
    let directive = summary.preproc_directive.as_deref().unwrap_or(if en {
        "conditional compilation directive"
    } else {
        "条件编译指令"
    });
    ReqDraft {
        description: templates::render(
            templates::lookup(language, "frame.preproc"),
            &[("function_name", func_name), ("directive", directive)],
        ),
        verify_method: review_verify(en).into(),
    }
}

/// 编号清单拼接（中文"1) a；\n2) b" / 英文 "1) a;\n2) b"，不带结尾标点）
fn join_numbered(phrases: &[String], en: bool) -> String {
    let sep = if en { ";\n" } else { "；\n" };
    phrases
        .iter()
        .enumerate()
        .map(|(i, p)| format!("{}) {}", i + 1, p))
        .collect::<Vec<_>>()
        .join(sep)
}

/// 行为短语映射（按源码顺序；调用已嵌在其他行为中时不单列）
fn behavior_phrases(summary: &BlockSummary, kind: SlicePlanKind, language: &str) -> Vec<String> {
    let mut phrases = Vec::new();
    for b in &summary.behaviors {
        match b {
            Behavior::Init { var, value } => phrases.push(templates::render(
                templates::lookup(language, "behavior.init"),
                &[("var", var), ("value", value)],
            )),
            Behavior::Assign { lhs, rhs } => phrases.push(templates::render(
                templates::lookup(language, "behavior.assign"),
                &[("lhs", lhs), ("rhs", rhs)],
            )),
            Behavior::CompoundAssign { text } => phrases.push(templates::render(
                templates::lookup(language, "behavior.compound_assign"),
                &[("text", text)],
            )),
            Behavior::Return { expr } => {
                if expr.is_empty() {
                    phrases.push(templates::lookup(language, "behavior.return_void").to_string());
                } else {
                    phrases.push(templates::render(
                        templates::lookup(language, "behavior.return"),
                        &[("expr", expr)],
                    ));
                }
            }
            Behavior::Call { name } => {
                // 调用已出现在其他行为文本中（如赋值/返回表达式）时不重复单列
                let nested = summary.behaviors.iter().any(|o| match o {
                    Behavior::Assign { lhs, rhs } => {
                        lhs.contains(name.as_str()) || rhs.contains(name.as_str())
                    }
                    Behavior::Init { var, value } => {
                        var.contains(name.as_str()) || value.contains(name.as_str())
                    }
                    Behavior::Return { expr } => expr.contains(name.as_str()),
                    Behavior::CompoundAssign { text } => text.contains(name.as_str()),
                    _ => false,
                });
                if !nested {
                    phrases.push(templates::render(
                        templates::lookup(language, "behavior.call"),
                        &[("name", name)],
                    ));
                }
            }
            Behavior::Break => {
                // 循环头独立成片后，break 位于循环体的计算片中
                if matches!(kind, SlicePlanKind::Loop | SlicePlanKind::Computation) {
                    phrases.push(templates::lookup(language, "behavior.break").to_string());
                }
            }
            Behavior::Continue => {
                if matches!(kind, SlicePlanKind::Loop | SlicePlanKind::Computation) {
                    phrases.push(templates::lookup(language, "behavior.continue").to_string());
                }
            }
        }
    }
    phrases
}

/// 验证方法：可测试需求（分支/循环/case/计算片的默认口径）
fn test_verify(en: bool) -> &'static str {
    if en {
        "Test"
    } else {
        "测试"
    }
}

/// 验证方法：条件编译段需审查各编译配置下的行为一致性
fn review_verify(en: bool) -> &'static str {
    if en {
        "Review"
    } else {
        "审查"
    }
}

/// 验证方法推断：条件编译段需审查，其余可测试。
/// 仅兜底草稿按切片类型推断；各类型构造器已知自身语义，直接调用 test_verify/review_verify
fn verify_method_for(kind: SlicePlanKind, en: bool) -> &'static str {
    match kind {
        SlicePlanKind::Preproc => review_verify(en),
        _ => test_verify(en),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cslice_core;

    /// 对源码中目标函数的第 idx 个切片计划项生成草稿（指定语言）
    fn draft_for_lang(src: &str, func: &str, idx: usize, language: &str) -> ReqDraft {
        let funcs = cslice_core::parse_functions(src);
        let f = funcs.iter().find(|f| f.name == func).expect("函数应存在");
        let items =
            cslice_core::plan_function_slices(src, f.start_line, f.end_line).expect("应生成计划");
        generate_draft(func, &items[idx], language)
    }

    /// 中文模式
    fn draft_for(src: &str, func: &str, idx: usize) -> ReqDraft {
        draft_for_lang(src, func, idx, "zh")
    }

    /// 目标函数的完整切片计划项列表（守卫条件测试用）
    fn plan_items_all(src: &str, func: &str) -> Vec<cslice_core::SlicePlanItem> {
        let funcs = cslice_core::parse_functions(src);
        let f = funcs.iter().find(|f| f.name == func).expect("函数应存在");
        cslice_core::plan_function_slices(src, f.start_line, f.end_line).expect("应生成计划")
    }

    #[test]
    fn branch_single_return() {
        let src = "int foo(int x) {\n    int y = 0;\n    if (x < 0) {\n        return -1;\n    }\n    y = x;\n    return y;\n}\n";
        let d = draft_for(src, "foo", 1);
        assert_eq!(d.description, "函数 foo 应返回 -1，当 x < 0 时。");
        assert_eq!(d.verify_method, "测试");
    }

    #[test]
    fn branch_multi_behavior() {
        let src = "void f(int x) {\n    if (x > 0) {\n        int t = x;\n        t += 1;\n        log(t);\n    }\n}\n";
        // 行为：t 初始化为 x、t += 1、log 调用（log 不嵌在其他行为文本中 → 单列）
        let d = draft_for(src, "f", 0);
        assert!(
            d.description
                .contains("1) 将局部变量 t 初始化为 x；\n2) 执行 t += 1；\n3) 调用函数 log"),
            "{}",
            d.description
        );
    }

    #[test]
    fn else_branch() {
        let src = "int grade(int s) {\n    if (s >= 60) {\n        return 1;\n    } else {\n        return 0;\n    }\n}\n";
        let d = draft_for(src, "grade", 1);
        assert_eq!(d.description, "当上述条件均不成立时，函数 grade 应返回 0。");
    }

    #[test]
    fn else_if_chain_condition() {
        let src = "int grade(int s) {\n    if (s >= 90) {\n        return 4;\n    } else if (s >= 80) {\n        return 3;\n    } else {\n        return 0;\n    }\n}\n";
        let d = draft_for(src, "grade", 1);
        assert_eq!(d.description, "函数 grade 应返回 3，当 s >= 80 时。");
    }

    #[test]
    fn loop_for_with_control() {
        let src = "int sum(int n) {\n    int acc = 0;\n    for (int i = 0; i < n; i++) {\n        acc += i;\n    }\n    return acc;\n}\n";
        // 循环头独立成片：描述迭代控制，不描述循环体行为
        let d = draft_for(src, "sum", 1);
        assert!(
            d.description
                .contains("函数 sum 应在满足 i < n 的条件下重复执行循环迭代"),
            "{}",
            d.description
        );
        assert!(
            d.description
                .contains("（循环控制：int i = 0；i < n；i++）"),
            "{}",
            d.description
        );

        // 循环体直接语句独立成计算片，需求带所在循环上下文
        let body = draft_for(src, "sum", 2);
        assert_eq!(
            body.description,
            "在该循环（i < n）的每次迭代中，函数 sum 应执行 acc += i。"
        );
    }

    #[test]
    fn loop_infinite_fallback_condition() {
        let src = "void spin(void) {\n    for (;;) {\n        poll();\n    }\n}\n";
        let d = draft_for(src, "spin", 0);
        assert!(
            d.description
                .contains("函数 spin 应重复执行循环迭代（循环条件取自源码）"),
            "{}",
            d.description
        );
    }

    #[test]
    fn switch_case_and_default() {
        let src = "int op(int cmd, int v) {\n    switch (cmd) {\n    case 1:\n        v += 1;\n        break;\n    default:\n        v = 0;\n        break;\n    }\n    return v;\n}\n";
        let case = draft_for(src, "op", 0);
        assert_eq!(
            case.description,
            "当 cmd 的取值等于 1 时，函数 op 应执行 v += 1。"
        );

        let default = draft_for(src, "op", 1);
        assert_eq!(
            default.description,
            "当 cmd 的取值不等于任何指定取值时，函数 op 应将 v 赋值为 0。"
        );
    }

    #[test]
    fn preproc_review_method() {
        let src = "void configure(void) {\n    int a = 0;\n#ifdef DEBUG\n    log_debug();\n#endif\n    a = 1;\n}\n";
        let d = draft_for(src, "configure", 1);
        assert_eq!(d.verify_method, "审查");
        assert!(d.description.contains("#ifdef DEBUG"));
    }

    #[test]
    fn computation_single_and_multi() {
        // 单行为：函数签名 + return
        let single = "int add(int a, int b) {\n    return a + b;\n}\n";
        let d = draft_for(single, "add", 0);
        assert_eq!(d.description, "函数 add 应返回 a + b。");

        // 初始化声明独立成片：初始值影响逻辑，单独一条初始化需求
        let multi = "int calc(int x) {\n    int y = 0;\n    y = x * 2;\n    return y;\n}\n";
        let d = draft_for(multi, "calc", 0);
        assert_eq!(d.description, "函数 calc 应将局部变量 y 初始化为 0。");

        // 赋值独立成片
        let d = draft_for(multi, "calc", 1);
        assert_eq!(d.description, "函数 calc 应将 y 赋值为 x * 2。");

        // return 独立成片：返回结果单独一条需求
        let d = draft_for(multi, "calc", 2);
        assert_eq!(d.description, "函数 calc 应返回 y。");

        // 连续赋值聚成一片：多行为编号清单
        let seq =
            "int calc2(int x) {\n    int y = x;\n    y = y * 2;\n    y += 1;\n    return y;\n}\n";
        let d = draft_for(seq, "calc2", 1);
        assert!(
            d.description
                .contains("1) 将 y 赋值为 y * 2；\n2) 执行 y += 1"),
            "{}",
            d.description
        );

        // 声明 + 调用 + return（Core_ReadU32 模式）：调用行为单独成需求
        let reader = "static uint32_t Core_ReadU32(const uint8_t* p)\n{\n    uint32_t v;\n    (void)memcpy(&v, p, sizeof(v));\n    return v;\n}\n";
        let d = draft_for(reader, "Core_ReadU32", 0);
        assert_eq!(d.description, "函数 Core_ReadU32 应调用函数 memcpy。");
        let d = draft_for(reader, "Core_ReadU32", 1);
        assert_eq!(d.description, "函数 Core_ReadU32 应返回 v。");
    }

    #[test]
    fn guard_condition_trailing_inline_and_block() {
        // 单一原子守卫条件：行内后置（en "when …." / zh "，当 … 时。"）
        let src = "int f(const int* p, int n) {
    int s = 0;
    if (p == 0 || n <= 0) {
        return -1;
    }
    for (int i = 0; i < n; i++)
    {
        s += p[i];
    }
    return s;
}
";
        let items = plan_items_all(src, "f");
        // 片序：0 s 初始化（守卫前，无守卫）/ 1 卫语句自身 / 2 循环头 / 3 循环体 / 4 return
        assert!(items[0].guard_conds.is_empty());
        assert!(items[1].guard_conds.is_empty());
        let d = generate_draft("f", &items[2], "zh");
        assert_eq!(
            d.description,
            "函数 f 应在满足 i < n 的条件下重复执行循环迭代（循环控制：int i = 0；i < n；i++），当 !(p == 0 || n <= 0) 时。",
            "{}",
            d.description
        );
        let d_en = generate_draft("f", &items[2], "en");
        assert!(
            d_en.description.ends_with("if !(p == 0 || n <= 0)."),
            "{}",
            d_en.description
        );
        // 守卫之后的循环体计算片：循环语境前导，守卫条件行内后置
        let body = generate_draft("f", &items[3], "zh");
        assert_eq!(
            body.description,
            "在该循环（i < n）的每次迭代中，函数 f 应执行 s += p[i]，当 !(p == 0 || n <= 0) 时。",
            "{}",
            body.description
        );
    }

    #[test]
    fn guard_conditions_accumulate_into_when_block() {
        // 多个守卫条件累积 → 结构化 when: 块（编号项 -AND- 连接）
        let src = "int f(int a, int b) {
    if (a == 0) {
        return 1;
    }
    if (b == 0) {
        return 2;
    }
    return work(a, b);
}
";
        let items = plan_items_all(src, "f");
        // return 片累积两个守卫条件
        let d = generate_draft("f", &items[2], "zh");
        assert_eq!(
            d.description,
            "函数 f 应返回 work(a, b)，当：
1.!(a == 0)
-AND-
2.!(b == 0)",
            "{}",
            d.description
        );
        let d_en = generate_draft("f", &items[2], "en");
        assert_eq!(
            d_en.description,
            "The f function shall return work(a, b) when:
1.!(a == 0)
-AND-
2.!(b == 0)",
            "{}",
            d_en.description
        );
    }

    #[test]
    fn empty_function_falls_back() {
        let src = "void nop(void) {\n}\n";
        let d = draft_for(src, "nop", 0);
        assert!(d.description.contains("请人工补充"), "{}", d.description);
    }

    #[test]
    fn nested_loop_draft_with_parent_context() {
        // 用户用例：CRC32 位处理——内层循环/循环内分支的需求带父循环上下文
        let src = "static uint32_t Crc32(const uint8_t* data, uint32_t len)\n{\n    uint32_t crc = 0xFFFFFFFFu;\n    uint32_t i;\n    uint32_t k;\n    for (i = 0u; i < len; i++)\n    {\n        crc ^= (uint32_t)data[i];\n        for (k = 0; k < 8; k++)\n        {\n            if ((crc & 1u) != 0u)\n            {\n                crc = (crc >> 1) ^ 0xEDB88320u;\n            }\n            else\n            {\n                crc >>= 1;\n            }\n        }\n    }\n    return ~crc;\n}\n";
        // 片序：0 初始化 / 1 外层循环头 / 2 循环内计算片（crc ^=）/ 3 内层循环头 / 4 if / 5 else / 6 return
        let outer = draft_for(src, "Crc32", 1);
        assert!(
            outer
                .description
                .contains("函数 Crc32 应在满足 i < len 的条件下重复执行循环迭代"),
            "{}",
            outer.description
        );
        assert!(
            !outer.description.contains("外层循环"),
            "外层循环无父上下文: {}",
            outer.description
        );

        // 循环体直接语句独立成计算片，需求带所在循环上下文
        let xor = draft_for(src, "Crc32", 2);
        assert!(
            xor.description.starts_with(
                "在该循环（i < len）的每次迭代中，函数 Crc32 应执行 crc ^= (uint32_t)data[i]"
            ),
            "{}",
            xor.description
        );

        let inner = draft_for(src, "Crc32", 3);
        assert!(
            inner
                .description
                .starts_with("在外层循环（i < len）的每次迭代中，函数 Crc32 应在满足 k < 8 的条件下重复执行循环迭代"),
            "{}",
            inner.description
        );
        assert!(
            inner
                .description
                .contains("（循环控制：k = 0；k < 8；k++）"),
            "{}",
            inner.description
        );

        let if_branch = draft_for(src, "Crc32", 4);
        assert!(
            if_branch.description.starts_with(
                "在该循环（k < 8）的每次迭代中，函数 Crc32 应将 crc 赋值为 (crc >> 1) ^ 0xEDB88320u，当 (crc & 1u) != 0u 时。",
            ),
            "{}",
            if_branch.description
        );

        let else_branch = draft_for(src, "Crc32", 5);
        assert!(
            else_branch
                .description
                .starts_with("在该循环（k < 8）的每次迭代中，当上述条件均不成立时"),
            "{}",
            else_branch.description
        );
    }

    #[test]
    fn loop_tail_computation_with_parent_context() {
        // 嵌套块之后的直接语句：需求带所在循环上下文
        let src = "int f(int n) {\n    int s = 0;\n    int j;\n    for (j = 0; j < n; j++)\n    {\n        if (j % 2 == 0)\n        {\n            s += j;\n        }\n        s = s + 1;\n    }\n    return s;\n}\n";
        // 片序：0 初始化 / 1 for 头 / 2 if / 3 尾部计算片（s = s + 1）/ 4 return
        let d = draft_for(src, "f", 3);
        assert_eq!(
            d.description,
            "在该循环（j < n）的每次迭代中，函数 f 应将 s 赋值为 s + 1。"
        );
    }

    #[test]
    fn english_nested_loop_with_parent_context() {
        let src = "static uint32_t Crc32(const uint8_t* data, uint32_t len)\n{\n    uint32_t crc = 0xFFFFFFFFu;\n    uint32_t i;\n    uint32_t k;\n    for (i = 0u; i < len; i++)\n    {\n        crc ^= (uint32_t)data[i];\n        for (k = 0; k < 8; k++)\n        {\n            if ((crc & 1u) != 0u)\n            {\n                crc = (crc >> 1) ^ 0xEDB88320u;\n            }\n            else\n            {\n                crc >>= 1;\n            }\n        }\n    }\n    return ~crc;\n}\n";
        let inner = draft_for_lang(src, "Crc32", 3, "en");
        assert!(
            inner.description.starts_with(
                "Within each iteration of the outer loop where i < len holds, the Crc32 function shall iterate repeatedly while k < 8 holds"
            ),
            "{}",
            inner.description
        );
        assert!(
            inner
                .description
                .contains("(loop control: k = 0; k < 8; k++)"),
            "{}",
            inner.description
        );

        let if_branch = draft_for_lang(src, "Crc32", 4, "en");
        assert!(
            if_branch.description.starts_with(
                "In each iteration of the loop where k < 8 holds, the Crc32 function shall set crc to (crc >> 1) ^ 0xEDB88320u if (crc & 1u) != 0u.",
            ),
            "{}",
            if_branch.description
        );

        let tail_src = "int f(int n) {\n    int s = 0;\n    int j;\n    for (j = 0; j < n; j++)\n    {\n        if (j % 2 == 0)\n        {\n            s += j;\n        }\n        s = s + 1;\n    }\n    return s;\n}\n";
        let tail = draft_for_lang(tail_src, "f", 3, "en");
        assert_eq!(
            tail.description,
            "In each iteration of the loop where j < n holds, the f function shall set s to s + 1."
        );
    }

    #[test]
    fn english_computation_return() {
        let reader = "static uint32_t Core_ReadU32(const uint8_t* p)\n{\n    uint32_t v;\n    (void)memcpy(&v, p, sizeof(v));\n    return v;\n}\n";
        let d = draft_for_lang(reader, "Core_ReadU32", 0, "en");
        assert_eq!(
            d.description,
            "The Core_ReadU32 function shall invoke the function memcpy."
        );
        assert_eq!(d.verify_method, "Test");

        let d = draft_for_lang(reader, "Core_ReadU32", 1, "en");
        assert_eq!(d.description, "The Core_ReadU32 function shall return v.");
    }

    #[test]
    fn english_branch_and_else() {
        let src = "int grade(int s) {\n    if (s >= 60) {\n        return 1;\n    } else {\n        return 0;\n    }\n}\n";
        let d = draft_for_lang(src, "grade", 0, "en");
        assert_eq!(
            d.description,
            "The grade function shall return 1 if s >= 60."
        );
        assert_eq!(d.verify_method, "Test");

        let d = draft_for_lang(src, "grade", 1, "en");
        assert_eq!(
            d.description,
            "When none of the above conditions holds, the grade function shall return 0."
        );
    }

    #[test]
    fn english_multi_behavior_numbered() {
        // 初始化声明独立成片（英文单行为）
        let multi = "int calc(int x) {\n    int y = 0;\n    y = x * 2;\n    return y;\n}\n";
        let d = draft_for_lang(multi, "calc", 0, "en");
        assert_eq!(
            d.description,
            "The calc function shall initialize the local variable y as 0."
        );

        // 连续赋值聚成一片：英文多行为编号清单
        let seq =
            "int calc2(int x) {\n    int y = x;\n    y = y * 2;\n    y += 1;\n    return y;\n}\n";
        let d = draft_for_lang(seq, "calc2", 1, "en");
        assert!(
            d.description
                .contains("1) set y to y * 2;\n2) execute y += 1"),
            "{}",
            d.description
        );
        assert!(
            d.description
                .starts_with("The calc2 function shall perform"),
            "{}",
            d.description
        );
    }

    #[test]
    fn english_loop_with_control() {
        let src = "int sum(int n) {\n    int acc = 0;\n    for (int i = 0; i < n; i++) {\n        acc += i;\n    }\n    return acc;\n}\n";
        let d = draft_for_lang(src, "sum", 1, "en");
        assert!(
            d.description
                .contains("The sum function shall iterate repeatedly while i < n holds"),
            "{}",
            d.description
        );
        assert!(
            d.description
                .contains("(loop control: int i = 0; i < n; i++)"),
            "{}",
            d.description
        );
    }

    #[test]
    fn english_case_default_and_preproc() {
        let src = "int op(int cmd, int v) {\n    switch (cmd) {\n    case 1:\n        v += 1;\n        break;\n    default:\n        v = 0;\n        break;\n    }\n    return v;\n}\n";
        let d = draft_for_lang(src, "op", 0, "en");
        assert_eq!(
            d.description,
            "When cmd equals 1, the op function shall execute v += 1."
        );

        let pre = "void configure(void) {\n    int a = 0;\n#ifdef DEBUG\n    log_debug();\n#endif\n    a = 1;\n}\n";
        let d = draft_for_lang(pre, "configure", 1, "en");
        assert_eq!(d.verify_method, "Review");
    }

    #[test]
    fn english_fallback() {
        let src = "void nop(void) {\n}\n";
        let d = draft_for_lang(src, "nop", 0, "en");
        assert!(
            d.description.contains("template fallback"),
            "{}",
            d.description
        );
    }
}
