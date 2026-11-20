use crate::ast::{Expr, Statement, Terminator};
use crate::calculus::{differentiate_n, integrate, integrate_definite, limit, series, solve};
use crate::linalg::{
    add_matrices, build_matrix, determinant, eigenvalues, inverse, matrix_pow, multiply_matrices,
    rref, scale_matrix, trace, transpose,
};
use crate::parser::parse_statements;
use crate::plot::{PlotData, sample_plot};
use crate::rational::Rational;
use crate::simplify::{evalf, expand, factor, simplify, subs};
use anyhow::{Result, bail};
use std::collections::{BTreeMap, HashSet};

/// User-defined Maple arrow function (`name := (p1, ...) -> body`).
#[derive(Clone, Debug, PartialEq)]
pub struct UserFunction {
    pub params: Vec<String>,
    pub body: Expr,
}

/// Evaluated output from a single Maple statement.
#[derive(Clone, Debug, PartialEq)]
pub enum EvalOutput {
    Value(Expr),
    Plot(PlotData),
    Suppressed,
}

/// Stateful CAS evaluation environment with bindings, user functions, and `%`/`%%` history.
#[derive(Clone, Debug, Default)]
pub struct Environment {
    pub variables: BTreeMap<String, Expr>,
    pub functions: BTreeMap<String, UserFunction>,
    pub last: Option<Expr>,
    pub second_last: Option<Expr>,
}

impl Environment {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn restart(&mut self) {
        self.variables.clear();
        self.functions.clear();
        self.last = None;
        self.second_last = None;
    }

    pub fn eval_line(&mut self, input: &str) -> Result<Vec<EvalOutput>> {
        let stmts = parse_statements(input)?;
        let mut results = Vec::with_capacity(stmts.len());
        for stmt in &stmts {
            results.push(self.eval_statement(stmt)?);
        }
        Ok(results)
    }

    pub fn eval_statement(&mut self, stmt: &Statement) -> Result<EvalOutput> {
        match stmt {
            Statement::Restart { terminator } => {
                self.restart();
                let val = Expr::Symbol("restarted".to_string());
                if *terminator == Terminator::Colon {
                    Ok(EvalOutput::Suppressed)
                } else {
                    Ok(EvalOutput::Value(val))
                }
            }
            Statement::Assignment {
                name,
                value,
                terminator,
            } => {
                let evaluated = self.eval_expr(value)?;
                self.variables.insert(name.clone(), evaluated.clone());
                self.record_history(evaluated.clone());
                if *terminator == Terminator::Colon {
                    Ok(EvalOutput::Suppressed)
                } else {
                    Ok(EvalOutput::Value(evaluated))
                }
            }
            Statement::FunctionDef {
                name,
                params,
                body,
                terminator,
            } => {
                let func = UserFunction {
                    params: params.clone(),
                    body: body.clone(),
                };
                self.functions.insert(name.clone(), func);
                let repr = Expr::Call(
                    "__arrow__".to_string(),
                    vec![
                        Expr::List(params.iter().cloned().map(Expr::Symbol).collect()),
                        body.clone(),
                    ],
                );
                self.record_history(repr.clone());
                if *terminator == Terminator::Colon {
                    Ok(EvalOutput::Suppressed)
                } else {
                    Ok(EvalOutput::Value(repr))
                }
            }
            Statement::EvalExpr { expr, terminator } => {
                let output = self.eval_expr_or_plot(expr)?;
                match output {
                    EvalOutput::Plot(plot) => Ok(EvalOutput::Plot(plot)),
                    EvalOutput::Value(val) => {
                        self.record_history(val.clone());
                        if *terminator == Terminator::Colon {
                            Ok(EvalOutput::Suppressed)
                        } else {
                            Ok(EvalOutput::Value(val))
                        }
                    }
                    EvalOutput::Suppressed => Ok(EvalOutput::Suppressed),
                }
            }
        }
    }

    fn record_history(&mut self, val: Expr) {
        self.second_last = self.last.take();
        self.last = Some(val);
    }

    pub fn eval_expr_or_plot(&self, expr: &Expr) -> Result<EvalOutput> {
        if let Expr::Call(name, args) = expr
            && name == "plot"
        {
            if args.len() != 2 {
                bail!("`plot` expects `(expr_or_list, var = a..b)`");
            }
            let Expr::Equation(lhs, rhs) = &args[1] else {
                bail!("`plot` second argument must be `var = a..b`");
            };
            let var = Self::extract_var_name(lhs)?;
            let Expr::Range(raw_a, raw_b) = rhs.as_ref() else {
                bail!("`plot` range must be `var = a..b`, got `{}`", args[1]);
            };
            let mut shadowed = HashSet::new();
            let a = self.eval_expr_with_shadow(raw_a, &mut shadowed)?;
            let b = self.eval_expr_with_shadow(raw_b, &mut shadowed)?;
            let curve = self.eval_with_var_shadowed(&args[0], &var, &mut shadowed)?;
            return Ok(EvalOutput::Plot(sample_plot(&curve, &var, &a, &b)?));
        }
        Ok(EvalOutput::Value(self.eval_expr(expr)?))
    }

    pub fn eval_expr(&self, expr: &Expr) -> Result<Expr> {
        let mut shadowed = HashSet::new();
        self.eval_expr_with_shadow(expr, &mut shadowed)
    }

    fn eval_expr_with_shadow(&self, expr: &Expr, shadowed: &mut HashSet<String>) -> Result<Expr> {
        match expr {
            Expr::Rational(_) | Expr::Float(_) => Ok(simplify(expr)),
            Expr::Symbol(s) => match s.as_str() {
                "%" => Ok(self
                    .last
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("`%` history register is empty"))?),
                "%%" => Ok(self
                    .second_last
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("`%%` history register is empty"))?),
                _ if !shadowed.contains(s) => {
                    if let Some(bound) = self.variables.get(s) {
                        Ok(bound.clone())
                    } else {
                        Ok(Expr::Symbol(s.clone()))
                    }
                }
                _ => Ok(Expr::Symbol(s.clone())),
            },
            Expr::UnaryNeg(a) => {
                let va = self.eval_expr_with_shadow(a, shadowed)?;
                if matches!(va, Expr::Matrix(_)) {
                    scale_matrix(&Expr::Rational(Rational::from_int(-1)), &va)
                } else {
                    Ok(simplify(&Expr::UnaryNeg(Box::new(va))))
                }
            }
            Expr::Add(a, b) => {
                let va = self.eval_expr_with_shadow(a, shadowed)?;
                let vb = self.eval_expr_with_shadow(b, shadowed)?;
                if matches!((&va, &vb), (Expr::Matrix(_), Expr::Matrix(_))) {
                    add_matrices(&va, &vb, false)
                } else {
                    Ok(simplify(&Expr::Add(Box::new(va), Box::new(vb))))
                }
            }
            Expr::Sub(a, b) => {
                let va = self.eval_expr_with_shadow(a, shadowed)?;
                let vb = self.eval_expr_with_shadow(b, shadowed)?;
                if matches!((&va, &vb), (Expr::Matrix(_), Expr::Matrix(_))) {
                    add_matrices(&va, &vb, true)
                } else {
                    Ok(simplify(&Expr::Sub(Box::new(va), Box::new(vb))))
                }
            }
            Expr::Mul(a, b) => {
                let va = self.eval_expr_with_shadow(a, shadowed)?;
                let vb = self.eval_expr_with_shadow(b, shadowed)?;
                match (&va, &vb) {
                    (Expr::Matrix(_), Expr::Matrix(_)) => multiply_matrices(&va, &vb),
                    (Expr::Matrix(_), _) => scale_matrix(&vb, &va),
                    (_, Expr::Matrix(_)) => scale_matrix(&va, &vb),
                    _ => Ok(simplify(&Expr::Mul(Box::new(va), Box::new(vb)))),
                }
            }
            Expr::Div(a, b) => Ok(simplify(&Expr::Div(
                Box::new(self.eval_expr_with_shadow(a, shadowed)?),
                Box::new(self.eval_expr_with_shadow(b, shadowed)?),
            ))),
            Expr::Pow(a, b) => {
                let va = self.eval_expr_with_shadow(a, shadowed)?;
                let vb = self.eval_expr_with_shadow(b, shadowed)?;
                if matches!(va, Expr::Matrix(_)) {
                    let exp = Self::as_integer(&vb)
                        .and_then(|n| i32::try_from(n).ok())
                        .ok_or_else(|| anyhow::anyhow!("invalid matrix exponent `{vb}`"))?;
                    matrix_pow(&va, exp)
                } else {
                    Ok(simplify(&Expr::Pow(Box::new(va), Box::new(vb))))
                }
            }
            Expr::Equation(a, b) => Ok(Expr::Equation(
                Box::new(self.eval_expr_with_shadow(a, shadowed)?),
                Box::new(self.eval_expr_with_shadow(b, shadowed)?),
            )),
            Expr::Range(a, b) => Ok(Expr::Range(
                Box::new(self.eval_expr_with_shadow(a, shadowed)?),
                Box::new(self.eval_expr_with_shadow(b, shadowed)?),
            )),
            Expr::List(items) => {
                let mut evaluated = Vec::with_capacity(items.len());
                for item in items {
                    evaluated.push(self.eval_expr_with_shadow(item, shadowed)?);
                }
                Ok(Expr::List(evaluated))
            }
            Expr::Matrix(rows) => {
                let mut evaluated = Vec::with_capacity(rows.len());
                for row in rows {
                    let mut erow = Vec::with_capacity(row.len());
                    for cell in row {
                        erow.push(self.eval_expr_with_shadow(cell, shadowed)?);
                    }
                    evaluated.push(erow);
                }
                Ok(Expr::Matrix(evaluated))
            }
            Expr::Call(name, args) => self.eval_call(name, args, shadowed),
        }
    }

    fn eval_call(&self, name: &str, args: &[Expr], shadowed: &mut HashSet<String>) -> Result<Expr> {
        if name == "__arrow__" {
            return Ok(Expr::Call(name.to_string(), args.to_vec()));
        }

        if name == "subs" {
            return self.eval_subs(args, shadowed);
        }

        if matches!(
            name,
            "diff" | "int" | "integrate" | "limit" | "series" | "taylor" | "solve"
        ) {
            return self.eval_calculus_call(name, args, shadowed);
        }

        // User-defined arrow function invocation.
        if let Some(user_fn) = self.functions.get(name) {
            if args.len() != user_fn.params.len() {
                bail!(
                    "function `{name}` expects {} argument(s), got {}",
                    user_fn.params.len(),
                    args.len()
                );
            }
            let mut body = user_fn.body.clone();
            for (param, raw_arg) in user_fn.params.iter().zip(args.iter()) {
                let val = self.eval_expr_with_shadow(raw_arg, shadowed)?;
                body = subs(param, &val, &body);
            }
            return self.eval_expr_with_shadow(&body, shadowed);
        }

        // 1-based Maple indexing for lists and matrices (`A[i]` or `A[i, j]`).
        if name == "__index__" && args.len() >= 2 {
            let target = self.eval_expr_with_shadow(&args[0], shadowed)?;
            let mut idxs = Vec::with_capacity(args.len() - 1);
            for raw in &args[1..] {
                let v = self.eval_expr_with_shadow(raw, shadowed)?;
                match v {
                    Expr::Rational(r) if r.is_integer() && r.num >= 1 => idxs.push(r.num as usize),
                    _ => bail!("invalid 1-based index `{v}`"),
                }
            }
            return match target {
                Expr::List(items) if idxs.len() == 1 => {
                    let i = idxs[0];
                    items
                        .get(i - 1)
                        .cloned()
                        .ok_or_else(|| anyhow::anyhow!("list index {i} out of bounds"))
                }
                Expr::Matrix(rows) if idxs.len() == 2 => {
                    let (r, c) = (idxs[0], idxs[1]);
                    rows.get(r - 1)
                        .and_then(|row| row.get(c - 1))
                        .cloned()
                        .ok_or_else(|| anyhow::anyhow!("matrix index ({r}, {c}) out of bounds"))
                }
                Expr::Matrix(rows) if idxs.len() == 1 => {
                    let r = idxs[0];
                    rows.get(r - 1)
                        .cloned()
                        .map(Expr::List)
                        .ok_or_else(|| anyhow::anyhow!("matrix row index {r} out of bounds"))
                }
                _ => bail!("cannot index `{target}` with {idxs:?}"),
            };
        }

        let mut eval_args = Vec::with_capacity(args.len());
        for a in args {
            eval_args.push(self.eval_expr_with_shadow(a, shadowed)?);
        }

        match name {
            "expand" => {
                if eval_args.len() != 1 {
                    bail!("`expand` expects 1 argument");
                }
                Ok(expand(&eval_args[0]))
            }
            "factor" => {
                if eval_args.len() != 1 {
                    bail!("`factor` expects 1 argument");
                }
                Ok(factor(&eval_args[0]))
            }
            "simplify" => {
                if eval_args.len() != 1 {
                    bail!("`simplify` expects 1 argument");
                }
                Ok(simplify(&eval_args[0]))
            }
            "evalf" => {
                if eval_args.len() != 1 {
                    bail!("`evalf` expects 1 argument");
                }
                Ok(Expr::Float(evalf(&eval_args[0])?))
            }
            "Matrix" => build_matrix(&eval_args),
            "det" | "Determinant" => {
                if eval_args.len() != 1 {
                    bail!("`{name}` expects 1 matrix argument");
                }
                determinant(&eval_args[0])
            }
            "Transpose" | "transpose" => {
                if eval_args.len() != 1 {
                    bail!("`{name}` expects 1 matrix argument");
                }
                transpose(&eval_args[0])
            }
            "Inverse" | "inverse" => {
                if eval_args.len() != 1 {
                    bail!("`{name}` expects 1 matrix argument");
                }
                inverse(&eval_args[0])
            }
            "rref" | "ReducedRowEchelonForm" => {
                if eval_args.len() != 1 {
                    bail!("`{name}` expects 1 matrix argument");
                }
                rref(&eval_args[0])
            }
            "trace" | "Trace" => {
                if eval_args.len() != 1 {
                    bail!("`{name}` expects 1 matrix argument");
                }
                trace(&eval_args[0])
            }
            "Eigenvalues" | "eigenvalues" => {
                if eval_args.len() != 1 {
                    bail!("`{name}` expects 1 matrix argument");
                }
                eigenvalues(&eval_args[0])
            }
            "Multiply" => {
                if eval_args.len() != 2 {
                    bail!("`Multiply` expects 2 matrix arguments");
                }
                multiply_matrices(&eval_args[0], &eval_args[1])
            }
            _ => Ok(simplify(&Expr::Call(name.to_string(), eval_args))),
        }
    }

    fn eval_subs(&self, args: &[Expr], shadowed: &mut HashSet<String>) -> Result<Expr> {
        if args.len() < 2 {
            bail!("`subs` expects substitution equation(s) followed by target expression");
        }
        let mut pairs: Vec<(String, Expr)> = Vec::new();
        for eq_arg in &args[..args.len() - 1] {
            match eq_arg {
                Expr::Equation(lhs, rhs) => {
                    let Expr::Symbol(var) = lhs.as_ref() else {
                        bail!("`subs` left-hand side must be a symbol, got `{lhs}`");
                    };
                    let val = self.eval_expr_with_shadow(rhs, shadowed)?;
                    pairs.push((var.clone(), val));
                }
                Expr::List(items) => {
                    for item in items {
                        match item {
                            Expr::Equation(lhs, rhs) => {
                                let Expr::Symbol(var) = lhs.as_ref() else {
                                    bail!("`subs` left-hand side must be a symbol, got `{lhs}`");
                                };
                                let val = self.eval_expr_with_shadow(rhs, shadowed)?;
                                pairs.push((var.clone(), val));
                            }
                            other => bail!("`subs` list item must be an equation, got `{other}`"),
                        }
                    }
                }
                other => bail!("`subs` expects `var = value`, got `{other}`"),
            }
        }

        // Prevent pre-substituting the target variable if it happens to be bound globally.
        let mut newly_shadowed = Vec::new();
        for (v, _) in &pairs {
            if shadowed.insert(v.clone()) {
                newly_shadowed.push(v.clone());
            }
        }
        let mut result = self.eval_expr_with_shadow(&args[args.len() - 1], shadowed)?;
        for v in newly_shadowed {
            shadowed.remove(&v);
        }
        for (v, val) in pairs {
            result = subs(&v, &val, &result);
        }
        Ok(result)
    }

    /// Helper for converting an expression to a positive rational or integer when needed.
    pub fn as_integer(expr: &Expr) -> Option<i128> {
        match expr {
            Expr::Rational(r) if r.is_integer() => Some(r.num),
            _ => None,
        }
    }

    pub fn rational_from_i128(n: i128) -> Expr {
        Expr::Rational(Rational::from_int(n))
    }

    fn eval_with_var_shadowed(
        &self,
        expr: &Expr,
        var: &str,
        shadowed: &mut HashSet<String>,
    ) -> Result<Expr> {
        let inserted = shadowed.insert(var.to_string());
        let res = self.eval_expr_with_shadow(expr, shadowed);
        if inserted {
            shadowed.remove(var);
        }
        res
    }

    fn extract_var_name(expr: &Expr) -> Result<String> {
        match expr {
            Expr::Symbol(s) => Ok(s.clone()),
            other => bail!("expected variable symbol, got `{other}`"),
        }
    }

    fn eval_calculus_call(
        &self,
        name: &str,
        args: &[Expr],
        shadowed: &mut HashSet<String>,
    ) -> Result<Expr> {
        match name {
            "diff" => {
                if args.len() < 2 {
                    bail!("`diff` expects expression and differentiation variable(s)");
                }
                let mut steps: Vec<(String, usize)> = Vec::new();
                for spec in &args[1..] {
                    match spec {
                        Expr::Symbol(v) => steps.push((v.clone(), 1)),
                        Expr::Call(op, sargs) if op == "$" && sargs.len() == 2 => {
                            let v = Self::extract_var_name(&sargs[0])?;
                            let ord_expr = self.eval_expr_with_shadow(&sargs[1], shadowed)?;
                            let ord = Self::as_integer(&ord_expr)
                                .filter(|&n| (0..=32).contains(&n))
                                .ok_or_else(|| {
                                    anyhow::anyhow!("invalid derivative order `{ord_expr}`")
                                })? as usize;
                            steps.push((v, ord));
                        }
                        other => bail!("invalid differentiation variable `{other}`"),
                    }
                }
                let mut inserted = Vec::new();
                for (v, _) in &steps {
                    if shadowed.insert(v.clone()) {
                        inserted.push(v.clone());
                    }
                }
                let expr_res = self.eval_expr_with_shadow(&args[0], shadowed);
                for v in inserted {
                    shadowed.remove(&v);
                }
                let mut cur = expr_res?;
                for (v, ord) in steps {
                    cur = differentiate_n(&cur, &v, ord)?;
                }
                Ok(cur)
            }
            "int" | "integrate" => {
                if args.len() != 2 {
                    bail!("`{name}` expects `(expr, var)` or `(expr, var = a..b)`");
                }
                match &args[1] {
                    Expr::Symbol(var) => {
                        let expr = self.eval_with_var_shadowed(&args[0], var, shadowed)?;
                        integrate(&expr, var)
                    }
                    Expr::Equation(lhs, rhs) => {
                        let var = Self::extract_var_name(lhs)?;
                        let Expr::Range(raw_a, raw_b) = rhs.as_ref() else {
                            bail!("definite integral expects `var = a..b`, got `{}`", args[1]);
                        };
                        let a = self.eval_expr_with_shadow(raw_a, shadowed)?;
                        let b = self.eval_expr_with_shadow(raw_b, shadowed)?;
                        let expr = self.eval_with_var_shadowed(&args[0], &var, shadowed)?;
                        integrate_definite(&expr, &var, &a, &b)
                    }
                    other => bail!("invalid integration specification `{other}`"),
                }
            }
            "limit" => {
                if args.len() != 2 {
                    bail!("`limit` expects `(expr, var = point)`");
                }
                let Expr::Equation(lhs, rhs) = &args[1] else {
                    bail!("`limit` second argument must be `var = point`");
                };
                let var = Self::extract_var_name(lhs)?;
                let point = self.eval_expr_with_shadow(rhs, shadowed)?;
                let expr = self.eval_with_var_shadowed(&args[0], &var, shadowed)?;
                limit(&expr, &var, &point)
            }
            "series" | "taylor" => {
                if args.len() < 2 || args.len() > 3 {
                    bail!("`{name}` expects `(expr, var = point[, order])`");
                }
                let (var, point) = match &args[1] {
                    Expr::Symbol(v) => (v.clone(), Expr::Rational(Rational::zero())),
                    Expr::Equation(lhs, rhs) => {
                        let v = Self::extract_var_name(lhs)?;
                        let p = self.eval_expr_with_shadow(rhs, shadowed)?;
                        (v, p)
                    }
                    other => bail!(
                        "`{name}` second argument must be `var` or `var = point`, got `{other}`"
                    ),
                };
                let order = if args.len() == 3 {
                    let ord_expr = self.eval_expr_with_shadow(&args[2], shadowed)?;
                    Self::as_integer(&ord_expr)
                        .filter(|&n| (0..=16).contains(&n))
                        .ok_or_else(|| anyhow::anyhow!("invalid series order `{ord_expr}`"))?
                        as usize
                } else {
                    5
                };
                let expr = self.eval_with_var_shadowed(&args[0], &var, shadowed)?;
                series(&expr, &var, &point, order)
            }
            "solve" => {
                if args.is_empty() || args.len() > 2 {
                    bail!("`solve` expects `(equation_or_expr, var)`");
                }
                let var = if args.len() == 2 {
                    Self::extract_var_name(&args[1])?
                } else {
                    let mut syms = std::collections::BTreeSet::new();
                    let raw = self.eval_expr_with_shadow(&args[0], shadowed)?;
                    crate::simplify::collect_symbols(&raw, &mut syms);
                    syms.into_iter()
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("no variable found to solve for"))?
                };
                let eq_expr = self.eval_with_var_shadowed(&args[0], &var, shadowed)?;
                solve(&eq_expr, &var)
            }
            _ => unreachable!(),
        }
    }
}
