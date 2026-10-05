//! 需求草稿内置模板：默认句式表与渲染。
//!
//! 两层模板：
//! - 行为短语层（behavior.*）：单个行为事实 → 动词短语，填进框架的 `{behavior}` 槽
//! - 句子框架层（frame.*）：主语、条件前缀、循环上下文、case/loop/条件编译/兜底句式
//!
//! 设计约定：
//! - 模板只负责填空；拼接顺序、英文首字母小写、编号清单格式、Call 去重、
//!   Break/Continue 守卫、空兜底等编排逻辑留在 crate 根模块
//! - 句式固定为内置默认表（只读，不支持运行时覆盖）

/// 行为短语层模板 key（填进 `{behavior}` 槽）
pub const BEHAVIOR_KEYS: &[&str] = &[
    "behavior.init",
    "behavior.assign",
    "behavior.compound_assign",
    "behavior.return",
    "behavior.return_void",
    "behavior.call",
    "behavior.break",
    "behavior.continue",
];

/// 句子框架层模板 key
pub const FRAME_KEYS: &[&str] = &[
    "frame.single",
    "frame.list",
    "frame.branch_empty",
    "frame.cond_prefix",
    "frame.else_prefix",
    "frame.loop_prefix",
    "frame.outer_loop_prefix",
    "frame.case_eq",
    "frame.case_default",
    "frame.loop",
    "frame.loop_nocond",
    "frame.loop_control",
    "frame.preproc",
    "frame.fallback",
];

/// 全部合法模板 key
pub fn all_keys() -> impl Iterator<Item = &'static str> {
    BEHAVIOR_KEYS.iter().chain(FRAME_KEYS.iter()).copied()
}

/// 模板正文中的占位符形态：`{` 与 `}` 之间为 ASCII 字母/数字/下划线才视为占位符；
/// 其余花括号段（空、含空白、嵌套等）视为字面文本原样保留。
fn is_ident(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// 查生效模板（内置默认表）。语言非法按中文处理（与草稿生成的 `language == "en"` 口径一致）。
pub fn lookup(language: &str, key: &str) -> &'static str {
    let lang = if language == "en" { "en" } else { "zh" };
    default_template(lang, key).unwrap_or_default()
}

/// 内置默认模板表。
fn default_template(lang: &str, key: &str) -> Option<&'static str> {
    if lang == "en" {
        Some(match key {
            "behavior.init" => "initialize the local variable {var} as {value}",
            "behavior.assign" => "set {lhs} to {rhs}",
            "behavior.compound_assign" => "execute {text}",
            "behavior.return" => "return {expr}",
            "behavior.return_void" => "return",
            "behavior.call" => "invoke the function {name}",
            "behavior.break" => "terminate the current loop",
            "behavior.continue" => "proceed to the next iteration",
            "frame.single" => "The {function_name} function shall {behavior}.",
            "frame.list" => {
                "The {function_name} function shall perform the following operations in order: {numbered}."
            }
            "frame.branch_empty" => {
                "The {function_name} function shall perform the processing logic within this branch."
            }
            "frame.cond_prefix" => "When {condition}, ",
            "frame.else_prefix" => "When none of the above conditions holds, ",
            "frame.loop_prefix" => "In each iteration of the loop where {parent_loop_cond} holds, ",
            "frame.outer_loop_prefix" => {
                "Within each iteration of the outer loop where {parent_loop_cond} holds, "
            }
            "frame.case_eq" => "When {condition} equals {case_value}, ",
            "frame.case_default" => "When {condition} does not equal any specified value, ",
            "frame.loop" => {
                "The {function_name} function shall iterate repeatedly while {loop_cond} holds{loop_control}."
            }
            "frame.loop_nocond" => {
                "The {function_name} function shall iterate repeatedly (the loop condition is taken from the source code)."
            }
            "frame.loop_control" => " (loop control: {loop_init}; {loop_cond}; {loop_update})",
            "frame.preproc" => {
                "This code segment of the {function_name} function is controlled by conditional compilation ({directive}); the {function_name} function shall execute the segment behavior when the corresponding compilation configuration is active, and the behavioral consistency across compilation configurations shall be confirmed by review."
            }
            "frame.fallback" => {
                "The {function_name} function shall implement the behavior expressed by the slice code (L{start_line}-L{end_line}). (This is a template fallback; supplement the specific behavior description manually.)"
            }
            _ => return None,
        })
    } else {
        Some(match key {
            "behavior.init" => "将局部变量 {var} 初始化为 {value}",
            "behavior.assign" => "将 {lhs} 赋值为 {rhs}",
            "behavior.compound_assign" => "执行 {text}",
            "behavior.return" => "返回 {expr}",
            "behavior.return_void" => "返回",
            "behavior.call" => "调用函数 {name}",
            "behavior.break" => "终止当前循环",
            "behavior.continue" => "进入下一次循环",
            "frame.single" => "函数 {function_name} 应{behavior}。",
            "frame.list" => "函数 {function_name} 应按顺序执行以下操作：{numbered}。",
            "frame.branch_empty" => "函数 {function_name} 应执行该分支内的处理逻辑。",
            "frame.cond_prefix" => "当 {condition} 时，",
            "frame.else_prefix" => "当上述条件均不成立时，",
            "frame.loop_prefix" => "在该循环（{parent_loop_cond}）的每次迭代中，",
            "frame.outer_loop_prefix" => "在外层循环（{parent_loop_cond}）的每次迭代中，",
            "frame.case_eq" => "当 {condition} 的取值等于 {case_value} 时，",
            "frame.case_default" => "当 {condition} 的取值不等于任何指定取值时，",
            "frame.loop" => {
                "函数 {function_name} 应在满足 {loop_cond} 的条件下重复执行循环迭代{loop_control}。"
            }
            "frame.loop_nocond" => "函数 {function_name} 应重复执行循环迭代（循环条件取自源码）。",
            "frame.loop_control" => "（循环控制：{loop_init}；{loop_cond}；{loop_update}）",
            "frame.preproc" => {
                "函数 {function_name} 的该段代码受条件编译控制（{directive}），在对应编译配置生效时应执行该段行为，并应通过审查确认各编译配置下的行为一致性。"
            }
            "frame.fallback" => {
                "函数 {function_name} 应实现切片代码（L{start_line}-L{end_line}）所表达的行为。（本条为模板兜底生成，请人工补充具体行为描述。）"
            }
            _ => return None,
        })
    }
}

/// 渲染模板：单遍扫描，`{ident}` 形态且 ident 在 vars 中则替换为实参；
/// 其余花括号段（空、非 ident、未知名）原样保留，不吞字面花括号。
pub fn render(template: &str, vars: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            if let Some(end) = template[i + 1..].find('}') {
                let inner = &template[i + 1..i + 1 + end];
                if is_ident(inner) {
                    if let Some((_, val)) = vars.iter().find(|(k, _)| *k == inner) {
                        out.push_str(val);
                        i += 1 + end + 1;
                        continue;
                    }
                }
            }
        }
        // 非占位符：逐字节复制（UTF-8 多字节字节不会是 ASCII '{'，逐字节安全）
        let ch_len = utf8_len(bytes[i]);
        out.push_str(&template[i..i + ch_len]);
        i += ch_len;
    }
    out
}

fn utf8_len(b: u8) -> usize {
    match b {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_replaces_known_placeholders() {
        let out = render(
            "将 {var} 初始化为 {value}",
            &[("var", "y"), ("value", "x * 2")],
        );
        assert_eq!(out, "将 y 初始化为 x * 2");
    }

    #[test]
    fn render_keeps_unknown_and_non_ident_braces() {
        // 未知占位符原样保留
        let out = render(
            "set {lhs} to {rhs} and {oops}",
            &[("lhs", "a"), ("rhs", "b")],
        );
        assert_eq!(out, "set a to b and {oops}");
        // 空/非 ident 花括号段原样保留（C 复合字面量等字面文本）
        let out = render("init {var} as {1, 2} {}", &[("var", "s")]);
        assert_eq!(out, "init s as {1, 2} {}");
        // 值文本里含 "{value}" 不会被二次展开（单遍扫描）
        let out = render(
            "set {lhs} to {rhs}",
            &[("lhs", "a"), ("rhs", "{lhs} literal")],
        );
        assert_eq!(out, "set a to {lhs} literal");
    }

    #[test]
    fn defaults_cover_every_key_in_both_languages() {
        for key in all_keys() {
            for lang in ["zh", "en"] {
                assert!(
                    default_template(lang, key).is_some(),
                    "默认表缺 {lang}/{key}"
                );
            }
        }
        // 语言非法按中文处理
        assert_eq!(
            lookup("fr", "behavior.init"),
            default_template("zh", "behavior.init").unwrap()
        );
    }
}
