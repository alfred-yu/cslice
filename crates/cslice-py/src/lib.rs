//! `cslice` Python 扩展模块（PyO3 绑定）。
//!
//! 仅做类型转换与便捷封装：切分逻辑全部在 `cslice-core`（纯 Rust，可独立复用）。

use cslice_core as core;
use cslice_draft as draft;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyTuple;

// ---------------------------------------------------------------------------
// Python 类型：core 类型 → Python 对象
// ---------------------------------------------------------------------------

/// 一个 C 函数定义（按出现顺序解析）。
#[pyclass(get_all, skip_from_py_object, name = "FunctionDef")]
#[derive(Clone)]
struct PyFunctionDef {
    /// 函数名
    name: String,
    /// 签名文本：函数定义起始到函数体 '{' 之前（含返回类型、函数名、参数表）
    signature: String,
    /// 1-based 起始行（含）
    start_line: u32,
    /// 1-based 结束行（含）
    end_line: u32,
}

impl From<core::FunctionDef> for PyFunctionDef {
    fn from(f: core::FunctionDef) -> Self {
        Self {
            name: f.name,
            signature: f.signature,
            start_line: f.start_line,
            end_line: f.end_line,
        }
    }
}

#[pymethods]
impl PyFunctionDef {
    fn __repr__(&self) -> String {
        format!(
            "cslice.FunctionDef(name={:?}, start_line={}, end_line={})",
            self.name, self.start_line, self.end_line
        )
    }
}

/// 逻辑块内的单一行为事实。`kind` 决定哪些字段有值：
/// init → var/value；assign → lhs/rhs；compound_assign → text；
/// return → expr（裸 return 为空串）；call → name；break/continue 无附加字段。
#[pyclass(get_all, skip_from_py_object, name = "Behavior")]
#[derive(Clone, Default)]
struct PyBehavior {
    /// "init" | "assign" | "compound_assign" | "return" | "call" | "break" | "continue"
    kind: String,
    /// init：被初始化的变量
    var: Option<String>,
    /// init：初始值表达式
    value: Option<String>,
    /// assign：赋值左值
    lhs: Option<String>,
    /// assign：赋值右值
    rhs: Option<String>,
    /// compound_assign：复合赋值原文本（y += i）
    text: Option<String>,
    /// return：返回表达式（裸 return 为空串）
    expr: Option<String>,
    /// call：被调函数名
    name: Option<String>,
}

impl From<&core::Behavior> for PyBehavior {
    fn from(b: &core::Behavior) -> Self {
        match b {
            core::Behavior::Init { var, value } => PyBehavior {
                kind: "init".into(),
                var: Some(var.clone()),
                value: Some(value.clone()),
                ..Default::default()
            },
            core::Behavior::Assign { lhs, rhs } => PyBehavior {
                kind: "assign".into(),
                lhs: Some(lhs.clone()),
                rhs: Some(rhs.clone()),
                ..Default::default()
            },
            core::Behavior::CompoundAssign { text } => PyBehavior {
                kind: "compound_assign".into(),
                text: Some(text.clone()),
                ..Default::default()
            },
            core::Behavior::Return { expr } => PyBehavior {
                kind: "return".into(),
                expr: Some(expr.clone()),
                ..Default::default()
            },
            core::Behavior::Call { name } => PyBehavior {
                kind: "call".into(),
                name: Some(name.clone()),
                ..Default::default()
            },
            core::Behavior::Break => PyBehavior {
                kind: "break".into(),
                ..Default::default()
            },
            core::Behavior::Continue => PyBehavior {
                kind: "continue".into(),
                ..Default::default()
            },
        }
    }
}

#[pymethods]
impl PyBehavior {
    fn __repr__(&self) -> String {
        let mut parts = vec![format!("kind={:?}", self.kind)];
        for (k, v) in [
            ("var", &self.var),
            ("value", &self.value),
            ("lhs", &self.lhs),
            ("rhs", &self.rhs),
            ("text", &self.text),
            ("expr", &self.expr),
            ("name", &self.name),
        ] {
            if let Some(v) = v {
                parts.push(format!("{k}={v:?}"));
            }
        }
        format!("cslice.Behavior({})", parts.join(", "))
    }
}

/// 切片语义摘要：从 AST 提取的"代码事实"。
#[pyclass(get_all, skip_from_py_object, name = "BlockSummary")]
#[derive(Clone)]
struct PyBlockSummary {
    /// if/else-if/switch 条件表达式文本（去外层括号）；提取失败为 None
    condition: Option<String>,
    /// 是否为无条件分支（else / default）
    is_else: bool,
    /// case 取值文本（default 分支为 None）
    case_value: Option<String>,
    /// 循环头完整文本（如 "for (int i = 0; i < 10; i++)"），仅 loop 类切片
    loop_header: Option<String>,
    /// for 三段式：init / condition / update 文本（均可能缺省）
    loop_init: Option<String>,
    loop_cond: Option<String>,
    loop_update: Option<String>,
    /// 条件编译指令首行（如 "#ifdef DEBUG"），仅 preproc 类切片
    preproc_directive: Option<String>,
    /// 父循环条件文本：循环体嵌套拆分产生的内层片携带外层循环条件
    parent_loop_cond: Option<String>,
    /// 行为清单（赋值/初始化/return/调用/跳转，按源码顺序）
    behaviors: Vec<PyBehavior>,
    /// 行为语句总数
    behavior_count: u32,
}

impl From<&core::BlockSummary> for PyBlockSummary {
    fn from(s: &core::BlockSummary) -> Self {
        Self {
            condition: s.condition.clone(),
            is_else: s.is_else,
            case_value: s.case_value.clone(),
            loop_header: s.loop_header.clone(),
            loop_init: s.loop_init.clone(),
            loop_cond: s.loop_cond.clone(),
            loop_update: s.loop_update.clone(),
            preproc_directive: s.preproc_directive.clone(),
            parent_loop_cond: s.parent_loop_cond.clone(),
            behaviors: s.behaviors.iter().map(Into::into).collect(),
            // 核心模型中该字段无赋值点（恒 0），按文档语义以行为清单长度为准
            behavior_count: s.behaviors.len() as u32,
        }
    }
}

/// 一个切片：函数体内可独立描述的原子行为代码块。
/// 全部切片构成深度优先森林：父片必先于子片出现（从顶向下、从外到内）。
#[pyclass(skip_from_py_object, name = "SlicePlanItem")]
#[derive(Clone)]
struct PySlicePlanItem {
    /// 函数内序号（0-based，等于列表下标）
    #[pyo3(get)]
    id: u32,
    /// 包裹本片的循环头片 id；顶层片为 None
    #[pyo3(get)]
    parent_id: Option<u32>,
    /// 嵌套深度：函数体顶层为 0，进入循环体 +1
    #[pyo3(get)]
    depth: u32,
    /// 1-based 起始行（含）
    #[pyo3(get)]
    start_line: u32,
    /// 1-based 结束行（含）
    #[pyo3(get)]
    end_line: u32,
    /// "computation" | "branch" | "loop" | "case" | "preproc"（见 cslice.KINDS）
    #[pyo3(get)]
    kind: String,
    /// 切片代码文本快照（按行提取，去除行尾 \r，不含结尾换行）
    #[pyo3(get)]
    code_text: String,
    /// 语义摘要
    #[pyo3(get)]
    summary: PyBlockSummary,
    /// 核心结构体（私有，供草稿生成直接消费，避免反向解析）
    core_item: core::SlicePlanItem,
}

impl PySlicePlanItem {
    fn new(item: &core::SlicePlanItem, source: &str) -> PyResult<Self> {
        let code_text = core::extract_lines(source, item.start_line as i64, item.end_line as i64)
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(Self {
            id: item.id,
            parent_id: item.parent_id,
            depth: item.depth,
            start_line: item.start_line,
            end_line: item.end_line,
            kind: item.kind.as_str().to_string(),
            code_text,
            summary: (&item.summary).into(),
            core_item: item.clone(),
        })
    }
}

#[pymethods]
impl PySlicePlanItem {
    fn __repr__(&self) -> String {
        format!(
            "cslice.SlicePlanItem(L{}-L{}, kind={:?})",
            self.start_line, self.end_line, self.kind
        )
    }
}

/// 单个函数的完整切片计划（[`slice_all`] 的返回项）。
#[pyclass(get_all, skip_from_py_object, name = "FunctionPlan")]
#[derive(Clone)]
struct PyFunctionPlan {
    /// 函数定义
    function: PyFunctionDef,
    /// 切片计划（按行有序）；个别函数计划失败时为空
    items: Vec<PySlicePlanItem>,
}

/// 需求草稿：描述/验证方法两要素（cslice.drafts 可选能力）。
/// 需求不设独立标题——展示以 description 为准，需要标题形态时从 description 派生。
#[pyclass(get_all, skip_from_py_object, name = "ReqDraft")]
#[derive(Clone)]
struct PyReqDraft {
    /// 需求描述
    description: String,
    /// 验证方法（条件编译 → "审查"/"Review"，其余 → "测试"/"Test"）
    verify_method: String,
}

impl From<draft::ReqDraft> for PyReqDraft {
    fn from(d: draft::ReqDraft) -> Self {
        Self {
            description: d.description,
            verify_method: d.verify_method,
        }
    }
}

#[pymethods]
impl PyReqDraft {
    fn __repr__(&self) -> String {
        format!(
            "cslice.ReqDraft(verify_method={:?}, description={:?})",
            self.verify_method, self.description
        )
    }
}

// ---------------------------------------------------------------------------
// 入口函数
// ---------------------------------------------------------------------------

/// 解析 C 源码中的全部函数定义（按出现顺序）。
/// tree-sitter 容错解析：语法错误时仍尽量返回可识别的函数。
#[pyfunction]
fn parse_functions(source: &str) -> Vec<PyFunctionDef> {
    core::parse_functions(source)
        .into_iter()
        .map(Into::into)
        .collect()
}

/// 按行范围定位函数并生成逻辑块切片计划（1-based 含端点，须精确匹配函数起止行）。
/// 找不到匹配的函数定义时返回 None（行范围与源码不一致等）。
/// 每片附带 code_text（从源码提取的代码文本快照）与 summary（语义摘要）。
#[pyfunction]
fn plan_function_slices(
    source: &str,
    start_line: u32,
    end_line: u32,
) -> PyResult<Option<Vec<PySlicePlanItem>>> {
    let Some(items) = core::plan_function_slices(source, start_line, end_line) else {
        return Ok(None);
    };
    let mapped = items
        .iter()
        .map(|item| PySlicePlanItem::new(item, source))
        .collect::<PyResult<Vec<_>>>()?;
    Ok(Some(mapped))
}

/// 从源码中提取 1-based 行范围 [start_line, end_line] 的代码文本。
/// 每行去除行尾 \r，以 \n 连接（不含结尾换行）。行范围越界抛 ValueError。
#[pyfunction]
fn extract_lines(source: &str, start_line: i64, end_line: i64) -> PyResult<String> {
    core::extract_lines(source, start_line, end_line)
        .map_err(|e| PyValueError::new_err(e.to_string()))
}

/// 一次性切分源码中的全部函数：对 [`parse_functions`] 的每个函数生成切片计划，
/// 每片附带 code_text。个别函数计划失败（解析容错的边缘情况）时其 items 为空。
#[pyfunction]
fn slice_all(source: &str) -> PyResult<Vec<PyFunctionPlan>> {
    let mut out = Vec::new();
    for f in core::parse_functions(source) {
        let items = match core::plan_function_slices(source, f.start_line, f.end_line) {
            Some(plan) => plan
                .iter()
                .map(|item| PySlicePlanItem::new(item, source))
                .collect::<PyResult<Vec<_>>>()?,
            None => Vec::new(),
        };
        out.push(PyFunctionPlan {
            function: f.into(),
            items,
        });
    }
    Ok(out)
}

/// 全部切片类型编码（与 SlicePlanItem.kind 一致）
const KINDS: [&str; 5] = ["computation", "branch", "loop", "case", "preproc"];

// ---------------------------------------------------------------------------
// 草稿生成（cslice.drafts 可选能力，离线确定性；由 Python 侧 drafts 子模块暴露）
// ---------------------------------------------------------------------------

/// 从单个切片计划项生成需求草稿（模板路径，离线确定）。
/// language：默认 "en" 英文 shall 句式；传 "zh" 得中文"应"句式，其他值按中文处理。
#[pyfunction]
#[pyo3(signature = (func_name, item, language = "en"))]
fn draft_generate_draft(func_name: &str, item: &PySlicePlanItem, language: &str) -> PyReqDraft {
    draft::generate_draft(func_name, &item.core_item, language).into()
}

/// 兜底草稿：语义提取失败（空函数体/无法解析的行为）时使用。
/// kind 传切片类型字符串，未知值按核心口径回退为 computation。
#[pyfunction]
#[pyo3(signature = (func_name, kind, start_line, end_line, language = "en"))]
fn draft_fallback_draft(
    func_name: &str,
    kind: &str,
    start_line: u32,
    end_line: u32,
    language: &str,
) -> PyReqDraft {
    draft::fallback_draft(
        func_name,
        core::SlicePlanKind::from_code(kind),
        start_line,
        end_line,
        language,
    )
    .into()
}

/// 批量：对同一函数的多个切片计划项生成草稿（按列表顺序）。
#[pyfunction]
#[pyo3(signature = (func_name, items, language = "en"))]
fn draft_generate_drafts(
    func_name: &str,
    items: Vec<PyRef<'_, PySlicePlanItem>>,
    language: &str,
) -> Vec<PyReqDraft> {
    items
        .iter()
        .map(|i| draft::generate_draft(func_name, &i.core_item, language).into())
        .collect()
}

/// cslice：C 函数逻辑块切分引擎（基于 tree-sitter）。
#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyFunctionDef>()?;
    m.add_class::<PyBehavior>()?;
    m.add_class::<PyBlockSummary>()?;
    m.add_class::<PySlicePlanItem>()?;
    m.add_class::<PyFunctionPlan>()?;
    m.add_function(wrap_pyfunction!(parse_functions, m)?)?;
    m.add_function(wrap_pyfunction!(plan_function_slices, m)?)?;
    m.add_function(wrap_pyfunction!(extract_lines, m)?)?;
    m.add_function(wrap_pyfunction!(slice_all, m)?)?;
    m.add("KINDS", PyTuple::new(m.py(), KINDS)?)?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add_class::<PyReqDraft>()?;
    m.add_function(wrap_pyfunction!(draft_generate_draft, m)?)?;
    m.add_function(wrap_pyfunction!(draft_fallback_draft, m)?)?;
    m.add_function(wrap_pyfunction!(draft_generate_drafts, m)?)?;
    Ok(())
}
