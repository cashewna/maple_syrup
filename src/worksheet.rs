use crate::ast::{Expr, Statement, format_pretty_math};
use crate::env::{Environment, EvalOutput};
use crate::parser::parse_statements;
use crate::plot::{PlotData, sample_plot};
use crate::rational::Rational;
use crate::simplify::collect_symbols;
use std::collections::BTreeSet;
use std::time::Instant;

/// Rendered output payload for a single `WorksheetCell`.
#[derive(Clone, Debug, PartialEq)]
pub enum CellOutput {
    Text(String),
    MatrixGrid(Vec<Vec<String>>),
    Plot(PlotData),
    Suppressed,
    Error(String),
}

/// Preset fill mode for generating a matrix expression from the Matrix Palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatrixPresetKind {
    Zero,
    Identity,
    Sequential,
    Symbolic,
}

/// Configurable row/column matrix expression generator for the Matrix Palette.
pub struct MatrixTemplate;

impl MatrixTemplate {
    /// Build a Maple `Matrix([[...]])` statement string for the requested dimensions and preset.
    pub fn build_expr(rows: usize, cols: usize, kind: MatrixPresetKind) -> String {
        let r_count = rows.clamp(1, 6);
        let c_count = cols.clamp(1, 6);
        let row_strs: Vec<String> = (0..r_count)
            .map(|r| {
                let cells: Vec<String> = (0..c_count)
                    .map(|c| match kind {
                        MatrixPresetKind::Zero => "0".to_string(),
                        MatrixPresetKind::Identity => {
                            if r == c {
                                "1".to_string()
                            } else {
                                "0".to_string()
                            }
                        }
                        MatrixPresetKind::Sequential => (r * c_count + c + 1).to_string(),
                        MatrixPresetKind::Symbolic => format!("a{}{}", r + 1, c + 1),
                    })
                    .collect();
                format!("[{}]", cells.join(", "))
            })
            .collect();
        format!("Matrix([{}]);", row_strs.join(", "))
    }
}

/// Single evaluated `[>` entry in the Maple worksheet log.
#[derive(Clone, Debug, PartialEq)]
pub struct WorksheetCell {
    pub index: usize,
    pub input: String,
    pub assigned_name: Option<String>,
    pub result_expr: Option<Expr>,
    pub preview_plot: Option<PlotData>,
    pub output: CellOutput,
    pub elapsed_us: u128,
}

impl WorksheetCell {
    /// Return formatted 2D-style mathematical string for the evaluated result.
    pub fn formatted_output(&self) -> String {
        if let Some(expr) = &self.result_expr {
            return format_pretty_math(expr, self.assigned_name.as_deref());
        }
        match &self.output {
            CellOutput::Text(t) => t.clone(),
            CellOutput::MatrixGrid(rows) => format!(
                "[{}]",
                rows.iter()
                    .map(|r| format!("[{}]", r.join(", ")))
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
            CellOutput::Plot(p) => p.title.clone(),
            CellOutput::Suppressed => String::new(),
            CellOutput::Error(e) => e.clone(),
        }
    }

    /// Return context-sensitive one-click Maple actions for this cell's output.
    pub fn context_actions(&self) -> Vec<(&'static str, String)> {
        match &self.output {
            CellOutput::MatrixGrid(_) => vec![
                ("Determinant", "det(%);".to_string()),
                ("Inverse", "Inverse(%);".to_string()),
                ("Transpose", "Transpose(%);".to_string()),
                ("RREF", "rref(%);".to_string()),
                ("Eigenvalues", "Eigenvalues(%);".to_string()),
                ("Trace", "trace(%);".to_string()),
            ],
            CellOutput::Text(_) => vec![
                ("Differentiate (d/dx)", "diff(%, x);".to_string()),
                ("Integrate (∫ dx)", "int(%, x);".to_string()),
                ("Simplify", "simplify(%);".to_string()),
                ("Expand", "expand(%);".to_string()),
                ("Factor", "factor(%);".to_string()),
                ("Evaluate Numerically", "evalf(%);".to_string()),
                ("Solve = 0", "solve(% = 0, x);".to_string()),
                ("Taylor Series (deg 5)", "series(%, x = 0, 5);".to_string()),
                ("Plot 2D (-Pi..Pi)", "plot(%, x = -Pi..Pi);".to_string()),
            ],
            CellOutput::Plot(_) | CellOutput::Suppressed | CellOutput::Error(_) => Vec::new(),
        }
    }
}

/// Stateful worksheet managing the CAS `Environment` and evaluated `WorksheetCell` history.
#[derive(Clone, Debug, Default)]
pub struct Worksheet {
    pub env: Environment,
    pub cells: Vec<WorksheetCell>,
}

impl Worksheet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Evaluate one or more Maple statements and append resulting cells to the worksheet.
    pub fn evaluate(&mut self, input: &str) -> Vec<WorksheetCell> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }
        let started = Instant::now();
        let mut added = Vec::new();

        match parse_statements(trimmed) {
            Err(err) => {
                let cell = self.push_cell(
                    trimmed.to_string(),
                    None,
                    None,
                    None,
                    CellOutput::Error(err.to_string()),
                    started.elapsed().as_micros(),
                );
                added.push(cell);
            }
            Ok(stmts) => {
                let single_stmt = stmts.len() == 1;
                for stmt in &stmts {
                    let stmt_start = Instant::now();
                    let label = if single_stmt {
                        trimmed.to_string()
                    } else {
                        format_statement_label(stmt)
                    };
                    let assigned_name = match stmt {
                        Statement::Assignment { name, .. }
                        | Statement::FunctionDef { name, .. } => Some(name.clone()),
                        _ => None,
                    };
                    let (result_expr, preview_plot, output) = match self.env.eval_statement(stmt) {
                        Ok(EvalOutput::Value(val)) => {
                            let preview = build_preview_plot(&val);
                            let cell_out = match &val {
                                Expr::Matrix(rows) => CellOutput::MatrixGrid(
                                    rows.iter()
                                        .map(|r| r.iter().map(|c| c.to_string()).collect())
                                        .collect(),
                                ),
                                _ => CellOutput::Text(val.to_string()),
                            };
                            (Some(val), preview, cell_out)
                        }
                        Ok(EvalOutput::Plot(plot)) => (None, None, CellOutput::Plot(plot)),
                        Ok(EvalOutput::Suppressed) => (None, None, CellOutput::Suppressed),
                        Err(err) => (None, None, CellOutput::Error(err.to_string())),
                    };
                    let cell = self.push_cell(
                        label,
                        assigned_name,
                        result_expr,
                        preview_plot,
                        output,
                        stmt_start.elapsed().as_micros(),
                    );
                    added.push(cell);
                }
            }
        }
        added
    }

    pub fn restart(&mut self) {
        self.env.restart();
    }

    pub fn clear_cells(&mut self) {
        self.cells.clear();
    }

    /// Reset cell outputs while preserving input expressions.
    pub fn clear_outputs(&mut self) {
        self.env.restart();
        for cell in &mut self.cells {
            cell.output = CellOutput::Suppressed;
            cell.result_expr = None;
            cell.preview_plot = None;
            cell.elapsed_us = 0;
        }
    }

    /// Remove a cell by zero-based index and renumber remaining cells `(1)..(n)`.
    pub fn delete_cell(&mut self, cell_idx: usize) {
        if cell_idx < self.cells.len() {
            self.cells.remove(cell_idx);
            for (idx, cell) in self.cells.iter_mut().enumerate() {
                cell.index = idx + 1;
            }
        }
    }

    /// Re-execute every cell in order against a freshly restarted kernel.
    pub fn execute_all(&mut self) {
        let inputs: Vec<String> = self.cells.iter().map(|c| c.input.clone()).collect();
        self.env.restart();
        self.cells.clear();
        for input in inputs {
            self.evaluate(&input);
        }
    }

    /// Load and run a demonstration worksheet starter session.
    pub fn load_preset(&mut self, preset: &str) -> Vec<WorksheetCell> {
        let script = match preset {
            "calculus" => {
                "f := x^3 * sin(x);\n\
                 diff(f, x);\n\
                 int(x^2, x = 0..3);\n\
                 limit((1 - cos(x))/x^2, x = 0);"
            }
            "linalg" => {
                "A := Matrix([[1, 2], [3, 4]]);\n\
                 det(A);\n\
                 Inverse(A);\n\
                 rref(Matrix([[1, 2, -1, -4], [2, 3, -1, -11], [-2, 0, -3, 22]]));\n\
                 Eigenvalues(Matrix([[4, 1], [2, 3]]));"
            }
            "series" => {
                "series(exp(x), x = 0, 5);\n\
                 series(sin(x), x = 0, 5);\n\
                 solve(x^2 - 5*x + 6 = 0, x);"
            }
            "plot" => "plot([sin(x), cos(x), x/3], x = -Pi..Pi);",
            _ => return Vec::new(),
        };
        self.evaluate(script)
    }

    fn push_cell(
        &mut self,
        input: String,
        assigned_name: Option<String>,
        result_expr: Option<Expr>,
        preview_plot: Option<PlotData>,
        output: CellOutput,
        elapsed_us: u128,
    ) -> WorksheetCell {
        let index = self.cells.len() + 1;
        let cell = WorksheetCell {
            index,
            input,
            assigned_name,
            result_expr,
            preview_plot,
            output,
            elapsed_us,
        };
        self.cells.push(cell.clone());
        cell
    }
}

fn build_preview_plot(expr: &Expr) -> Option<PlotData> {
    match expr {
        Expr::Matrix(_) | Expr::List(_) | Expr::Equation(_, _) | Expr::Range(_, _) => return None,
        Expr::Call(name, _) if name == "__arrow__" => return None,
        _ => {}
    }
    let mut syms = BTreeSet::new();
    collect_symbols(expr, &mut syms);
    if syms.len() != 1 || !syms.contains("x") {
        return None;
    }
    let lo = Expr::Rational(Rational::from_int(-10));
    let hi = Expr::Rational(Rational::from_int(10));
    sample_plot(expr, "x", &lo, &hi).ok()
}

fn format_statement_label(stmt: &crate::ast::Statement) -> String {
    use crate::ast::{Statement, Terminator};
    let term = |t: Terminator| if t == Terminator::Colon { ":" } else { ";" };
    match stmt {
        Statement::Assignment {
            name,
            value,
            terminator,
        } => format!("{name} := {value}{}", term(*terminator)),
        Statement::FunctionDef {
            name,
            params,
            body,
            terminator,
        } => {
            if params.len() == 1 {
                format!("{name} := {} -> {body}{}", params[0], term(*terminator))
            } else {
                format!(
                    "{name} := ({}) -> {body}{}",
                    params.join(", "),
                    term(*terminator)
                )
            }
        }
        Statement::EvalExpr { expr, terminator } => format!("{expr}{}", term(*terminator)),
        Statement::Restart { terminator } => format!("restart{}", term(*terminator)),
    }
}
