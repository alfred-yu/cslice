//! 需求草稿合规检查（纯函数 lint）。
//!
//! 检查项：空描述、"应/shall"句式、函数名主语、歧义词、空验证方法、原子性启发。
//! 仅产生警告/提示供人工评审参考，不阻断任何流程。

/// 单条检查结果：warning（应修复）| info（供参考）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintIssue {
    /// "warning" | "info"
    pub level: String,
    pub message: String,
}

/// 检查结果汇总（需求列表合规列用）
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LintSummary {
    pub warning_count: u32,
    pub info_count: u32,
}

/// 歧义词表（需求编写规范常见禁用主观/模糊词，可按评审意见扩充）
pub const AMBIGUOUS_WORDS: &[&str] = &[
    "等等",
    "适当",
    "尽可能",
    "必要时",
    "快速",
    "大概",
    "可能",
    "若干",
    "差不多",
    "某些",
];

/// 描述分句数（"；"与"。"计数之和）超过该值时提示拆分
const CLAUSE_LIMIT: usize = 5;

/// 对需求描述与验证方法做合规检查（纯函数，无 IO）。
/// func_name 为需求关联切片所属函数名（未关联切片时为 None，跳过主语检查）
pub fn lint_requirement(
    description: &str,
    verify_method: &str,
    func_name: Option<&str>,
) -> Vec<LintIssue> {
    let mut issues = Vec::new();
    let desc = description.trim();

    // 1) 空描述
    if desc.is_empty() {
        issues.push(LintIssue {
            level: "warning".into(),
            message: "需求描述为空".into(),
        });
    } else {
        // 2) 强制性句式：中文"应"或英文 "shall"/"should"
        if !desc.contains('应')
            && !desc.to_lowercase().contains("shall")
            && !desc.to_lowercase().contains("should")
        {
            issues.push(LintIssue {
                level: "warning".into(),
                message: "需求描述应使用\"应\"（或英文 \"shall\"/\"should\"）句式表述强制性要求"
                    .into(),
            });
        }
        // 3) 函数名主语：中文"函数 {name} 应…"或英文 "The {name} function shall/should …"
        if let Some(func) = func_name.map(str::trim).filter(|f| !f.is_empty()) {
            let zh_subject = format!("函数 {func}");
            let en_subject = format!("the {func} function").to_lowercase();
            if !desc.contains(&zh_subject) && !desc.to_lowercase().contains(&en_subject) {
                issues.push(LintIssue {
                    level: "warning".into(),
                    message: format!(
                        "描述应以函数名为主语（\"函数 {func} 应…\"或 \"The {func} function shall/should …\"）"
                    ),
                });
            }
        }
    }

    // 4) 歧义词扫描（仅描述文本）
    for word in AMBIGUOUS_WORDS {
        if desc.contains(word) {
            issues.push(LintIssue {
                level: "warning".into(),
                message: format!("包含歧义词「{word}」，应改为明确、可判定的表述"),
            });
        }
    }

    // 5) 空验证方法
    if verify_method.trim().is_empty() {
        issues.push(LintIssue {
            level: "warning".into(),
            message: "验证方法为空：每条需求应具有明确的验证方法".into(),
        });
    }

    // 6) 原子性启发：分句过多提示拆分（仅 info 级）
    if !desc.is_empty() {
        let clauses = desc.matches('；').count() + desc.matches('。').count();
        if clauses > CLAUSE_LIMIT {
            issues.push(LintIssue {
                level: "info".into(),
                message: format!(
                    "描述包含 {clauses} 处分句，建议评估是否应拆分为多条需求（原子性）"
                ),
            });
        }
    }

    issues
}

/// 汇总问题计数（需求列表合规列用）
pub fn summarize(issues: &[LintIssue]) -> LintSummary {
    let mut summary = LintSummary::default();
    for issue in issues {
        if issue.level == "warning" {
            summary.warning_count += 1;
        } else {
            summary.info_count += 1;
        }
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    fn warnings(issues: &[LintIssue]) -> Vec<&str> {
        issues
            .iter()
            .filter(|i| i.level == "warning")
            .map(|i| i.message.as_str())
            .collect()
    }

    #[test]
    fn compliant_requirement_has_no_issues() {
        let issues = lint_requirement("当 x < 0 时，函数 foo 应返回 -1。", "测试", None);
        assert!(issues.is_empty(), "合规需求不应有问题: {issues:?}");
    }

    #[test]
    fn empty_description_and_method_warn() {
        let issues = lint_requirement("", "", None);
        let ws = warnings(&issues);
        assert!(ws.iter().any(|m| m.contains("需求描述为空")), "{ws:?}");
        assert!(ws.iter().any(|m| m.contains("验证方法为空")), "{ws:?}");
        // 描述为空时不做句式检查（避免同一条需求双重警告）
        assert!(
            !ws.iter().any(|m| m.contains("句式表述强制性要求")),
            "{ws:?}"
        );
    }

    #[test]
    fn missing_shall_style_warns() {
        let issues = lint_requirement("函数 foo 执行计算并输出结果。", "测试", None);
        let ws = warnings(&issues);
        assert!(
            ws.iter().any(|m| m.contains("句式表述强制性要求")),
            "缺少\"应\"字应告警: {ws:?}"
        );
    }

    #[test]
    fn english_shall_style_accepted() {
        // 英文 shall 句式不告警
        let issues = lint_requirement("The foo function shall return -1 when x < 0.", "Test", None);
        assert!(
            issues.iter().all(|i| i.level != "warning"),
            "英文 shall 句式应为合规: {issues:?}"
        );
        // 大小写不敏感（句首 Shall）
        let issues = lint_requirement("Shall return 1.", "Test", None);
        assert!(
            issues.iter().all(|i| i.level != "warning"),
            "句首 Shall 应被接受: {issues:?}"
        );
    }

    #[test]
    fn subject_must_be_function_name() {
        // 中文：描述含"函数 foo 应…"即合规
        let ok = lint_requirement("当 x < 0 时，函数 foo 应返回 -1。", "测试", Some("foo"));
        assert!(warnings(&ok).is_empty(), "{ok:?}");
        // 英文：固定句式 The {func} function shall/should …
        let ok_en = lint_requirement(
            "When x < 0, the foo function shall return -1.",
            "Test",
            Some("foo"),
        );
        assert!(warnings(&ok_en).is_empty(), "{ok_en:?}");
        // should 句式接受（强制性句式 + 主语均通过）
        let ok_should = lint_requirement("The foo function should return -1.", "Test", Some("foo"));
        assert!(warnings(&ok_should).is_empty(), "{ok_should:?}");
        // 主语不是函数名（旧句式 "The function foo"）→ 告警
        let bad_en = lint_requirement("The function foo shall return -1.", "Test", Some("foo"));
        assert!(
            warnings(&bad_en).iter().any(|m| m.contains("函数名为主语")),
            "{bad_en:?}"
        );
        // 缺主语（泛指主语）→ 告警
        let bad = lint_requirement("当 x < 0 时应返回 -1。", "测试", Some("foo"));
        assert!(
            warnings(&bad).iter().any(|m| m.contains("函数名为主语")),
            "{bad:?}"
        );
        // 未关联切片（func_name=None）不检查主语
        let no_func = lint_requirement("当 x < 0 时应返回 -1。", "测试", None);
        assert!(!warnings(&no_func)
            .iter()
            .any(|m| m.contains("函数名为主语")));
    }

    #[test]
    fn ambiguous_words_detected_in_description_only() {
        let issues = lint_requirement("系统应尽可能及时完成处理。", "测试", None);
        let ws = warnings(&issues);
        assert!(ws.iter().any(|m| m.contains("「尽可能」")), "{ws:?}");
        // 正常词不受影响："应"字存在、无其他歧义词
        let ok = lint_requirement("函数 foo 应返回 -1。", "测试", None);
        assert!(warnings(&ok).is_empty());
    }

    #[test]
    fn clause_count_triggers_info_only() {
        // 6 项编号列表：5 个"；" + 1 个"。" = 6 > 5 → info
        let desc = "函数 f 应按顺序执行以下操作：\n1) a；\n2) b；\n3) c；\n4) d；\n5) e；\n6) f。";
        let issues = lint_requirement(desc, "测试", None);
        assert!(
            issues
                .iter()
                .any(|i| i.level == "info" && i.message.contains("原子性")),
            "{issues:?}"
        );
        assert!(
            warnings(&issues).is_empty(),
            "分句多应为 info 而非 warning: {issues:?}"
        );
        // 5 项列表：4"；"+1"。"= 5，不触发
        let desc5 = "函数 f 应按顺序执行以下操作：\n1) a；\n2) b；\n3) c；\n4) d；\n5) e。";
        let issues5 = lint_requirement(desc5, "测试", None);
        assert!(
            !issues5.iter().any(|i| i.message.contains("原子性")),
            "{issues5:?}"
        );
    }

    #[test]
    fn summarize_counts_by_level() {
        let issues = vec![
            LintIssue {
                level: "warning".into(),
                message: "a".into(),
            },
            LintIssue {
                level: "warning".into(),
                message: "b".into(),
            },
            LintIssue {
                level: "info".into(),
                message: "c".into(),
            },
        ];
        let s = summarize(&issues);
        assert_eq!(s.warning_count, 2);
        assert_eq!(s.info_count, 1);
    }
}
