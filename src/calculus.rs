use crate::ast::Expr;
use crate::rational::Rational;
use crate::simplify::{evalf, expand, extract_poly, simplify, subs};
use anyhow::{Result, bail};

/// Check whether `expr` contains free occurrences of `var`.
pub fn depends_on(expr: &Expr, var: &str) -> bool {
    match expr {
        Expr::Symbol(s) => s == var,
        Expr::UnaryNeg(a) => depends_on(a, var),
        Expr::Add(a, b)
        | Expr::Sub(a, b)
        | Expr::Mul(a, b)
        | Expr::Div(a, b)
        | Expr::Pow(a, b)
        | Expr::Equation(a, b)
        | Expr::Range(a, b) => depends_on(a, var) || depends_on(b, var),
        Expr::Call(_, args) | Expr::List(args) => args.iter().any(|a| depends_on(a, var)),
        Expr::Matrix(rows) => rows.iter().any(|r| r.iter().any(|c| depends_on(c, var))),
        Expr::Rational(_) | Expr::Float(_) => false,
    }
}

/// Differentiate `expr` with respect to `var` `order` times.
pub fn differentiate_n(expr: &Expr, var: &str, order: usize) -> Result<Expr> {
    let mut cur = expr.clone();
    for _ in 0..order {
        cur = differentiate(&cur, var)?;
    }
    Ok(cur)
}

/// Symbolically differentiate `expr` with respect to `var`.
pub fn differentiate(expr: &Expr, var: &str) -> Result<Expr> {
    let raw = diff_raw(expr, var)?;
    Ok(simplify(&raw))
}

fn diff_raw(expr: &Expr, var: &str) -> Result<Expr> {
    if !depends_on(expr, var) {
        return Ok(Expr::Rational(Rational::zero()));
    }
    match expr {
        Expr::Rational(_) | Expr::Float(_) => Ok(Expr::Rational(Rational::zero())),
        Expr::Symbol(s) => {
            if s == var {
                Ok(Expr::Rational(Rational::one()))
            } else {
                Ok(Expr::Rational(Rational::zero()))
            }
        }
        Expr::UnaryNeg(u) => Ok(Expr::UnaryNeg(Box::new(diff_raw(u, var)?))),
        Expr::Add(u, v) => Ok(Expr::Add(
            Box::new(diff_raw(u, var)?),
            Box::new(diff_raw(v, var)?),
        )),
        Expr::Sub(u, v) => Ok(Expr::Sub(
            Box::new(diff_raw(u, var)?),
            Box::new(diff_raw(v, var)?),
        )),
        Expr::Mul(u, v) => {
            let du = diff_raw(u, var)?;
            let dv = diff_raw(v, var)?;
            Ok(Expr::Add(
                Box::new(Expr::Mul(Box::new(du), v.clone())),
                Box::new(Expr::Mul(u.clone(), Box::new(dv))),
            ))
        }
        Expr::Div(u, v) => {
            if !depends_on(v, var) {
                return Ok(Expr::Div(Box::new(diff_raw(u, var)?), v.clone()));
            }
            let du = diff_raw(u, var)?;
            let dv = diff_raw(v, var)?;
            let num = Expr::Sub(
                Box::new(Expr::Mul(Box::new(du), v.clone())),
                Box::new(Expr::Mul(u.clone(), Box::new(dv))),
            );
            let den = Expr::Pow(v.clone(), Box::new(Expr::Rational(Rational::from_int(2))));
            Ok(Expr::Div(Box::new(num), Box::new(den)))
        }
        Expr::Pow(u, v) => {
            let u_dep = depends_on(u, var);
            let v_dep = depends_on(v, var);
            match (u_dep, v_dep) {
                (false, false) => Ok(Expr::Rational(Rational::zero())),
                (true, false) => {
                    let du = diff_raw(u, var)?;
                    let exp_minus_1 =
                        Expr::Sub(v.clone(), Box::new(Expr::Rational(Rational::one())));
                    let power = Expr::Pow(u.clone(), Box::new(exp_minus_1));
                    Ok(Expr::Mul(
                        Box::new(Expr::Mul(v.clone(), Box::new(power))),
                        Box::new(du),
                    ))
                }
                (false, true) => {
                    let dv = diff_raw(v, var)?;
                    if matches!(u.as_ref(), Expr::Symbol(s) if s == "e") {
                        Ok(Expr::Mul(Box::new(expr.clone()), Box::new(dv)))
                    } else {
                        let ln_u = Expr::Call("ln".to_string(), vec![(**u).clone()]);
                        Ok(Expr::Mul(
                            Box::new(Expr::Mul(Box::new(expr.clone()), Box::new(ln_u))),
                            Box::new(dv),
                        ))
                    }
                }
                (true, true) => {
                    let du = diff_raw(u, var)?;
                    let dv = diff_raw(v, var)?;
                    let ln_u = Expr::Call("ln".to_string(), vec![(**u).clone()]);
                    let term1 = Expr::Mul(Box::new(dv), Box::new(ln_u));
                    let term2 = Expr::Div(Box::new(Expr::Mul(v.clone(), Box::new(du))), u.clone());
                    Ok(Expr::Mul(
                        Box::new(expr.clone()),
                        Box::new(Expr::Add(Box::new(term1), Box::new(term2))),
                    ))
                }
            }
        }
        Expr::Call(name, args) => {
            if args.len() != 1 {
                bail!(
                    "cannot differentiate `{name}` with {} arguments",
                    args.len()
                );
            }
            let u = &args[0];
            let du = diff_raw(u, var)?;
            let deriv = match name.as_str() {
                "sin" => Expr::Call("cos".to_string(), vec![u.clone()]),
                "cos" => Expr::UnaryNeg(Box::new(Expr::Call("sin".to_string(), vec![u.clone()]))),
                "tan" => Expr::Div(
                    Box::new(Expr::Rational(Rational::one())),
                    Box::new(Expr::Pow(
                        Box::new(Expr::Call("cos".to_string(), vec![u.clone()])),
                        Box::new(Expr::Rational(Rational::from_int(2))),
                    )),
                ),
                "asin" | "arcsin" => Expr::Div(
                    Box::new(Expr::Rational(Rational::one())),
                    Box::new(Expr::Call(
                        "sqrt".to_string(),
                        vec![Expr::Sub(
                            Box::new(Expr::Rational(Rational::one())),
                            Box::new(Expr::Pow(
                                Box::new(u.clone()),
                                Box::new(Expr::Rational(Rational::from_int(2))),
                            )),
                        )],
                    )),
                ),
                "acos" | "arccos" => Expr::UnaryNeg(Box::new(Expr::Div(
                    Box::new(Expr::Rational(Rational::one())),
                    Box::new(Expr::Call(
                        "sqrt".to_string(),
                        vec![Expr::Sub(
                            Box::new(Expr::Rational(Rational::one())),
                            Box::new(Expr::Pow(
                                Box::new(u.clone()),
                                Box::new(Expr::Rational(Rational::from_int(2))),
                            )),
                        )],
                    )),
                ))),
                "atan" | "arctan" => Expr::Div(
                    Box::new(Expr::Rational(Rational::one())),
                    Box::new(Expr::Add(
                        Box::new(Expr::Rational(Rational::one())),
                        Box::new(Expr::Pow(
                            Box::new(u.clone()),
                            Box::new(Expr::Rational(Rational::from_int(2))),
                        )),
                    )),
                ),
                "sinh" => Expr::Call("cosh".to_string(), vec![u.clone()]),
                "cosh" => Expr::Call("sinh".to_string(), vec![u.clone()]),
                "tanh" => Expr::Div(
                    Box::new(Expr::Rational(Rational::one())),
                    Box::new(Expr::Pow(
                        Box::new(Expr::Call("cosh".to_string(), vec![u.clone()])),
                        Box::new(Expr::Rational(Rational::from_int(2))),
                    )),
                ),
                "exp" => Expr::Call("exp".to_string(), vec![u.clone()]),
                "ln" | "log" => Expr::Div(
                    Box::new(Expr::Rational(Rational::one())),
                    Box::new(u.clone()),
                ),
                "sqrt" => Expr::Div(
                    Box::new(Expr::Rational(Rational::one())),
                    Box::new(Expr::Mul(
                        Box::new(Expr::Rational(Rational::from_int(2))),
                        Box::new(Expr::Call("sqrt".to_string(), vec![u.clone()])),
                    )),
                ),
                "abs" => Expr::Div(
                    Box::new(u.clone()),
                    Box::new(Expr::Call("abs".to_string(), vec![u.clone()])),
                ),
                _ => bail!("cannot differentiate unknown function `{name}`"),
            };
            Ok(Expr::Mul(Box::new(deriv), Box::new(du)))
        }
        Expr::List(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(diff_raw(item, var)?);
            }
            Ok(Expr::List(out))
        }
        Expr::Matrix(rows) => {
            let mut out = Vec::with_capacity(rows.len());
            for row in rows {
                let mut erow = Vec::with_capacity(row.len());
                for cell in row {
                    erow.push(diff_raw(cell, var)?);
                }
                out.push(erow);
            }
            Ok(Expr::Matrix(out))
        }
        _ => bail!("cannot differentiate `{expr}` with respect to `{var}`"),
    }
}

/// Symbolically integrate `expr` with respect to `var` (up to an omitted constant of integration).
pub fn integrate(expr: &Expr, var: &str) -> Result<Expr> {
    let s = simplify(expr);
    if let Some(res) = try_integrate(&s, var) {
        return Ok(simplify(&res));
    }
    let exp = expand(&s);
    if exp != s
        && let Some(res) = try_integrate(&exp, var)
    {
        return Ok(simplify(&res));
    }
    bail!("no closed-form antiderivative found for `{expr}` with respect to `{var}`")
}

/// Evaluate a definite integral `int(expr, var = a..b)`.
pub fn integrate_definite(expr: &Expr, var: &str, a: &Expr, b: &Expr) -> Result<Expr> {
    let s = simplify(expr);
    if let Ok(antideriv) = integrate(&s, var) {
        let fb = subs(var, b, &antideriv);
        let fa = subs(var, a, &antideriv);
        return Ok(simplify(&Expr::Sub(Box::new(fb), Box::new(fa))));
    }

    // Numeric fallback via Composite Simpson's Rule when symbolic antiderivative is unavailable.
    let lo = evalf(a)?;
    let hi = evalf(b)?;
    let n = 1000usize;
    let h = (hi - lo) / (n as f64);
    let eval_at = |x_val: f64| -> Result<f64> {
        let point = Expr::Float(x_val);
        evalf(&subs(var, &point, &s))
    };
    let mut sum = eval_at(lo)? + eval_at(hi)?;
    for k in 1..n {
        let xk = lo + (k as f64) * h;
        let weight = if k % 2 == 0 { 2.0 } else { 4.0 };
        sum += weight * eval_at(xk)?;
    }
    let val = sum * h / 3.0;
    if !val.is_finite() {
        bail!("definite integral diverged or produced non-finite value");
    }
    Ok(Expr::Float(val))
}

fn try_integrate(expr: &Expr, var: &str) -> Option<Expr> {
    if !depends_on(expr, var) {
        return Some(Expr::Mul(
            Box::new(expr.clone()),
            Box::new(Expr::Symbol(var.to_string())),
        ));
    }
    match expr {
        Expr::UnaryNeg(u) => try_integrate(u, var).map(|r| Expr::UnaryNeg(Box::new(r))),
        Expr::Add(a, b) => Some(Expr::Add(
            Box::new(try_integrate(a, var)?),
            Box::new(try_integrate(b, var)?),
        )),
        Expr::Sub(a, b) => Some(Expr::Sub(
            Box::new(try_integrate(a, var)?),
            Box::new(try_integrate(b, var)?),
        )),
        Expr::Symbol(s) if s == var => Some(Expr::Div(
            Box::new(Expr::Pow(
                Box::new(Expr::Symbol(var.to_string())),
                Box::new(Expr::Rational(Rational::from_int(2))),
            )),
            Box::new(Expr::Rational(Rational::from_int(2))),
        )),
        Expr::Mul(_, _) | Expr::Div(_, _) => integrate_product_or_quotient(expr, var),
        Expr::Pow(base, exp) => integrate_pow(base, exp, var),
        Expr::Call(name, args) if args.len() == 1 => integrate_call(name, &args[0], var),
        _ => None,
    }
}

fn split_const_and_var_factors(expr: &Expr, var: &str) -> (Expr, Expr) {
    let mut const_parts = Vec::new();
    let mut var_parts = Vec::new();
    collect_split_mul(expr, false, var, &mut const_parts, &mut var_parts);
    let c = combine_mul_list(const_parts);
    let v = combine_mul_list(var_parts);
    (simplify(&c), simplify(&v))
}

fn collect_split_mul(
    expr: &Expr,
    invert: bool,
    var: &str,
    consts: &mut Vec<Expr>,
    vars: &mut Vec<Expr>,
) {
    match expr {
        Expr::Mul(a, b) => {
            collect_split_mul(a, invert, var, consts, vars);
            collect_split_mul(b, invert, var, consts, vars);
        }
        Expr::Div(a, b) => {
            collect_split_mul(a, invert, var, consts, vars);
            collect_split_mul(b, !invert, var, consts, vars);
        }
        other => {
            let item = if invert {
                Expr::Div(
                    Box::new(Expr::Rational(Rational::one())),
                    Box::new(other.clone()),
                )
            } else {
                other.clone()
            };
            if depends_on(other, var) {
                vars.push(item);
            } else {
                consts.push(item);
            }
        }
    }
}

fn combine_mul_list(items: Vec<Expr>) -> Expr {
    if items.is_empty() {
        return Expr::Rational(Rational::one());
    }
    let mut iter = items.into_iter();
    let mut acc = iter.next().unwrap();
    for item in iter {
        acc = Expr::Mul(Box::new(acc), Box::new(item));
    }
    acc
}

fn integrate_product_or_quotient(expr: &Expr, var: &str) -> Option<Expr> {
    let (c, v) = split_const_and_var_factors(expr, var);
    if !c.eq(&Expr::Rational(Rational::one())) {
        let inner = try_integrate(&v, var)?;
        return Some(Expr::Mul(Box::new(c), Box::new(inner)));
    }

    // 1 / (...) patterns
    if let Expr::Div(num, den) = expr
        && !depends_on(num, var)
    {
        if let Some((a, _b)) = extract_linear(den, var) {
            let ln_den = Expr::Call("ln".to_string(), vec![(**den).clone()]);
            return Some(
                Expr::Div(
                    Box::new((**num).clone()),
                    Box::new(Expr::Mul(
                        Box::new(a),
                        Box::new(Expr::Div(
                            Box::new(Expr::Rational(Rational::one())),
                            Box::new(ln_den),
                        )),
                    )),
                )
                .replace_div_by_mul_recip(),
            );
        }
        // Check `1 / (a*x^2 + b)` -> `atan`
        if let Some(poly) = extract_poly(den, var)
            && poly.len() == 3
            && poly[1].is_zero()
            && poly[0].is_positive()
            && poly[2].is_positive()
        {
            // int(1 / (c2*x^2 + c0)) = 1/sqrt(c0*c2) * atan(sqrt(c2/c0)*x)
            let c0 = Expr::Rational(poly[0]);
            let c2 = Expr::Rational(poly[2]);
            let scale = Expr::Call(
                "sqrt".to_string(),
                vec![Expr::Mul(Box::new(c0.clone()), Box::new(c2.clone()))],
            );
            let ratio = Expr::Call(
                "sqrt".to_string(),
                vec![Expr::Div(Box::new(c2), Box::new(c0))],
            );
            let arg = Expr::Mul(Box::new(ratio), Box::new(Expr::Symbol(var.to_string())));
            return Some(Expr::Div(
                Box::new(Expr::Mul(
                    Box::new((**num).clone()),
                    Box::new(Expr::Call("atan".to_string(), vec![arg])),
                )),
                Box::new(scale),
            ));
        }
        // 1 / (a*x + b)^n
        if let Expr::Pow(base, exp) = den.as_ref()
            && !depends_on(exp, var)
        {
            let neg_exp = simplify(&Expr::UnaryNeg(exp.clone()));
            if let Some(res) = integrate_pow(base, &neg_exp, var) {
                return Some(Expr::Mul(Box::new((**num).clone()), Box::new(res)));
            }
        }
        // 1 / sqrt(1 - x^2)
        if let Expr::Call(fn_name, args) = den.as_ref()
            && fn_name == "sqrt"
            && args.len() == 1
            && let Some(poly) = extract_poly(&args[0], var)
            && poly.len() == 3
            && poly[0].is_one()
            && poly[1].is_zero()
            && poly[2] == Rational::from_int(-1)
        {
            return Some(Expr::Mul(
                Box::new((**num).clone()),
                Box::new(Expr::Call(
                    "asin".to_string(),
                    vec![Expr::Symbol(var.to_string())],
                )),
            ));
        }
    }

    // Linear-in-denominator shortcut already handled cleanly below without helper trait:
    if let Expr::Div(num, den) = expr
        && !depends_on(num, var)
        && let Some((a, _b)) = extract_linear(den, var)
    {
        let ln_den = Expr::Call("ln".to_string(), vec![(**den).clone()]);
        return Some(Expr::Div(
            Box::new(Expr::Mul(Box::new((**num).clone()), Box::new(ln_den))),
            Box::new(a),
        ));
    }

    // Attempt u-substitution on `g'(x)/g(x)` or `f(g(x)) * g'(x)`.
    if let Some(u_sub) = try_u_substitution(expr, var) {
        return Some(u_sub);
    }

    // Attempt tabular integration by parts for `poly(x) * {exp, sin, cos}(a*x + b)`.
    if let Some(ibp) = try_tabular_ibp(expr, var) {
        return Some(ibp);
    }

    None
}

trait DivFixup {
    fn replace_div_by_mul_recip(self) -> Self;
}

impl DivFixup for Expr {
    fn replace_div_by_mul_recip(self) -> Self {
        self
    }
}

fn integrate_pow(base: &Expr, exp: &Expr, var: &str) -> Option<Expr> {
    if !depends_on(base, var) {
        // c^u where u is linear in var -> c^(a*x+b) / (a * ln(c))
        let (a, _b) = extract_linear(exp, var)?;
        if matches!(base, Expr::Symbol(s) if s == "e") {
            return Some(Expr::Div(
                Box::new(Expr::Pow(Box::new(base.clone()), Box::new(exp.clone()))),
                Box::new(a),
            ));
        }
        let ln_base = Expr::Call("ln".to_string(), vec![base.clone()]);
        return Some(Expr::Div(
            Box::new(Expr::Pow(Box::new(base.clone()), Box::new(exp.clone()))),
            Box::new(Expr::Mul(Box::new(a), Box::new(ln_base))),
        ));
    }

    if !depends_on(exp, var)
        && let Some((a, _b)) = extract_linear(base, var)
    {
        if matches!(exp, Expr::Rational(r) if *r == Rational::from_int(-1)) {
            let ln_base = Expr::Call("ln".to_string(), vec![base.clone()]);
            return Some(Expr::Div(Box::new(ln_base), Box::new(a)));
        }
        let new_exp = simplify(&Expr::Add(
            Box::new(exp.clone()),
            Box::new(Expr::Rational(Rational::one())),
        ));
        return Some(Expr::Div(
            Box::new(Expr::Pow(Box::new(base.clone()), Box::new(new_exp.clone()))),
            Box::new(Expr::Mul(Box::new(a), Box::new(new_exp))),
        ));
    }

    None
}

fn integrate_call(name: &str, arg: &Expr, var: &str) -> Option<Expr> {
    let (a, _b) = extract_linear(arg, var)?;
    let prim = match name {
        "sin" => Expr::UnaryNeg(Box::new(Expr::Call("cos".to_string(), vec![arg.clone()]))),
        "cos" => Expr::Call("sin".to_string(), vec![arg.clone()]),
        "exp" => Expr::Call("exp".to_string(), vec![arg.clone()]),
        "sqrt" => {
            let three_halves = Expr::Rational(Rational::new(3, 2).unwrap());
            return Some(Expr::Div(
                Box::new(Expr::Mul(
                    Box::new(Expr::Rational(Rational::from_int(2))),
                    Box::new(Expr::Pow(Box::new(arg.clone()), Box::new(three_halves))),
                )),
                Box::new(Expr::Mul(
                    Box::new(Expr::Rational(Rational::from_int(3))),
                    Box::new(a),
                )),
            ));
        }
        "ln" | "log" => {
            // int(ln(a*x+b)) = ((a*x+b)*ln(a*x+b) - (a*x+b)) / a
            let ln_arg = Expr::Call("ln".to_string(), vec![arg.clone()]);
            Expr::Sub(
                Box::new(Expr::Mul(Box::new(arg.clone()), Box::new(ln_arg))),
                Box::new(arg.clone()),
            )
        }
        _ => return None,
    };
    Some(Expr::Div(Box::new(prim), Box::new(a)))
}

fn extract_linear(expr: &Expr, var: &str) -> Option<(Expr, Expr)> {
    let s = expand(expr);
    let deriv = differentiate(&s, var).ok()?;
    if depends_on(&deriv, var) || matches!(deriv, Expr::Rational(r) if r.is_zero()) {
        return None;
    }
    let intercept = simplify(&subs(var, &Expr::Rational(Rational::zero()), &s));
    Some((deriv, intercept))
}

fn try_u_substitution(expr: &Expr, var: &str) -> Option<Expr> {
    // Check `num / den` -> `c * ln(den)` when `num / den' = c` (constant).
    if let Expr::Div(num, den) = expr
        && depends_on(den, var)
        && let Ok(dden) = differentiate(den, var)
        && !matches!(dden, Expr::Rational(r) if r.is_zero())
    {
        let ratio = simplify(&Expr::Div(num.clone(), Box::new(dden)));
        if !depends_on(&ratio, var) {
            return Some(Expr::Mul(
                Box::new(ratio),
                Box::new(Expr::Call("ln".to_string(), vec![(**den).clone()])),
            ));
        }
    }

    // Check product `A * B` where `A` or `B` has an inner `u` whose derivative divides the other factor.
    let mut factors = Vec::new();
    collect_mul_factors(expr, &mut factors);
    if factors.len() < 2 {
        return None;
    }
    for idx in 0..factors.len() {
        let cand = &factors[idx];
        let inner = match cand {
            Expr::Call(_, args) if args.len() == 1 && depends_on(&args[0], var) => args[0].clone(),
            Expr::Pow(base, exp) if !depends_on(exp, var) && depends_on(base, var) => {
                (**base).clone()
            }
            Expr::Pow(base, exp) if !depends_on(base, var) && depends_on(exp, var) => {
                (**exp).clone()
            }
            _ => continue,
        };
        let Ok(du) = differentiate(&inner, var) else {
            continue;
        };
        if matches!(du, Expr::Rational(r) if r.is_zero()) {
            continue;
        }
        let mut rest_factors = Vec::new();
        for (j, f) in factors.iter().enumerate() {
            if j != idx {
                rest_factors.push(f.clone());
            }
        }
        let rest = combine_mul_list(rest_factors);
        let ratio = simplify(&Expr::Div(Box::new(rest), Box::new(du)));
        if !depends_on(&ratio, var) {
            let u_sym = "__u_sub__";
            let substituted = subs_expr_once(cand, &inner, &Expr::Symbol(u_sym.to_string()));
            if let Some(u_int) = try_integrate(&substituted, u_sym) {
                let back = subs(u_sym, &inner, &u_int);
                return Some(Expr::Mul(Box::new(ratio), Box::new(back)));
            }
        }
    }
    None
}

fn subs_expr_once(expr: &Expr, target: &Expr, replacement: &Expr) -> Expr {
    if expr == target {
        return replacement.clone();
    }
    match expr {
        Expr::Call(name, args) => Expr::Call(
            name.clone(),
            args.iter()
                .map(|a| subs_expr_once(a, target, replacement))
                .collect(),
        ),
        Expr::Pow(b, e) => Expr::Pow(
            Box::new(subs_expr_once(b, target, replacement)),
            Box::new(subs_expr_once(e, target, replacement)),
        ),
        _ => expr.clone(),
    }
}

fn collect_mul_factors(expr: &Expr, out: &mut Vec<Expr>) {
    match expr {
        Expr::Mul(a, b) => {
            collect_mul_factors(a, out);
            collect_mul_factors(b, out);
        }
        other => out.push(other.clone()),
    }
}

fn try_tabular_ibp(expr: &Expr, var: &str) -> Option<Expr> {
    let mut factors = Vec::new();
    collect_mul_factors(expr, &mut factors);
    if factors.len() < 2 {
        return None;
    }

    // Find one transcendental factor `exp/sin/cos(a*x + b)` and verify remaining product is polynomial in `var`.
    for idx in 0..factors.len() {
        let trans = &factors[idx];
        let is_trans = match trans {
            Expr::Call(n, a) if a.len() == 1 && matches!(n.as_str(), "exp" | "sin" | "cos") => {
                extract_linear(&a[0], var).is_some()
            }
            Expr::Pow(b, e) if matches!(b.as_ref(), Expr::Symbol(s) if s == "e") => {
                extract_linear(e, var).is_some()
            }
            _ => false,
        };
        if !is_trans {
            continue;
        }
        let mut poly_factors = Vec::new();
        for (j, f) in factors.iter().enumerate() {
            if j != idx {
                poly_factors.push(f.clone());
            }
        }
        let poly_expr = expand(&combine_mul_list(poly_factors));
        let poly_coeffs = extract_poly(&poly_expr, var)?;
        if poly_coeffs.len() > 12 {
            return None;
        }
        let mut u = poly_expr;
        let mut v = trans.clone();
        let mut acc = Expr::Rational(Rational::zero());
        let mut sign = 1i128;
        for _ in 0..poly_coeffs.len() {
            v = try_integrate(&v, var)?;
            let term = Expr::Mul(Box::new(u.clone()), Box::new(v.clone()));
            acc = if sign > 0 {
                Expr::Add(Box::new(acc), Box::new(term))
            } else {
                Expr::Sub(Box::new(acc), Box::new(term))
            };
            u = differentiate(&u, var).ok()?;
            if matches!(u, Expr::Rational(r) if r.is_zero()) {
                break;
            }
            sign = -sign;
        }
        return Some(acc);
    }
    None
}

fn split_fraction(expr: &Expr) -> (Expr, Expr) {
    match expr {
        Expr::Div(num, den) => ((**num).clone(), (**den).clone()),
        _ => (expr.clone(), Expr::Rational(Rational::one())),
    }
}

fn is_zero_expr(expr: &Expr) -> bool {
    if matches!(expr, Expr::Rational(r) if r.is_zero()) {
        return true;
    }
    if matches!(expr, Expr::Float(v) if v.abs() < 1e-12) {
        return true;
    }
    if let Ok(v) = evalf(expr) {
        return v.abs() < 1e-12;
    }
    false
}

/// Evaluate symbolic `limit(expr, var = point)` using algebraic simplification and L'Hôpital's rule.
pub fn limit(expr: &Expr, var: &str, point: &Expr) -> Result<Expr> {
    if matches!(point, Expr::Symbol(s) if s == "infinity")
        || matches!(point, Expr::UnaryNeg(inner) if matches!(inner.as_ref(), Expr::Symbol(s) if s == "infinity"))
    {
        return limit_at_infinity(expr, var, point);
    }

    let s = simplify(expr);
    let (mut num, mut den) = split_fraction(&s);

    for _step in 0..8 {
        let sub_num = simplify(&subs(var, point, &num));
        let sub_den = simplify(&subs(var, point, &den));
        if !is_zero_expr(&sub_den) {
            return Ok(simplify(&Expr::Div(Box::new(sub_num), Box::new(sub_den))));
        }
        if !is_zero_expr(&sub_num) {
            bail!("limit `{expr}` as `{var} -> {point}` diverges (non-zero over zero)");
        }
        // 0 / 0 indeterminate form -> apply L'Hôpital's rule.
        num = differentiate(&num, var)?;
        den = differentiate(&den, var)?;
    }

    bail!("could not resolve indeterminate limit `{expr}` as `{var} -> {point}`")
}

fn limit_at_infinity(expr: &Expr, var: &str, point: &Expr) -> Result<Expr> {
    let s = simplify(expr);
    let (num, den) = split_fraction(&s);
    if let (Some(pn), Some(pd)) = (extract_poly(&num, var), extract_poly(&den, var)) {
        let dn = pn.len().saturating_sub(1);
        let dd = pd.len().saturating_sub(1);
        if dn < dd {
            return Ok(Expr::Rational(Rational::zero()));
        }
        if dn == dd {
            return Ok(Expr::Rational(pn[dn] / pd[dd]));
        }
        let neg_inf = matches!(point, Expr::UnaryNeg(_));
        let ratio = (pn[dn] / pd[dd]).to_f64();
        let sign_pos = if neg_inf && !(dn - dd).is_multiple_of(2) {
            ratio < 0.0
        } else {
            ratio > 0.0
        };
        return Ok(if sign_pos {
            Expr::Symbol("infinity".to_string())
        } else {
            Expr::UnaryNeg(Box::new(Expr::Symbol("infinity".to_string())))
        });
    }
    bail!("unsupported limit at infinity for `{expr}`")
}

/// Compute Taylor / Maclaurin series expansion of `expr` around `var = point` up to `order`.
pub fn series(expr: &Expr, var: &str, point: &Expr, order: usize) -> Result<Expr> {
    let mut cur = simplify(expr);
    let mut fact: i128 = 1;
    let mut terms = Vec::new();
    let shifted = if is_zero_expr(point) {
        Expr::Symbol(var.to_string())
    } else {
        simplify(&Expr::Sub(
            Box::new(Expr::Symbol(var.to_string())),
            Box::new(point.clone()),
        ))
    };

    for k in 0..=order {
        if k > 1 {
            fact = fact
                .checked_mul(k as i128)
                .ok_or_else(|| anyhow::anyhow!("factorial overflow in series order {k}"))?;
        }
        let val = simplify(&subs(var, point, &cur));
        if !is_zero_expr(&val) {
            let coeff = simplify(&Expr::Div(
                Box::new(val),
                Box::new(Expr::Rational(Rational::from_int(fact))),
            ));
            let term = match k {
                0 => coeff,
                1 => simplify(&Expr::Mul(Box::new(coeff), Box::new(shifted.clone()))),
                deg => simplify(&Expr::Mul(
                    Box::new(coeff),
                    Box::new(Expr::Pow(
                        Box::new(shifted.clone()),
                        Box::new(Expr::Rational(Rational::from_int(deg as i128))),
                    )),
                )),
            };
            terms.push(term);
        }
        if k < order {
            cur = differentiate(&cur, var)?;
        }
    }

    if terms.is_empty() {
        return Ok(Expr::Rational(Rational::zero()));
    }
    // Preserve ascending power order (`1 + x + 1/2*x^2 + ...`) by folding directly with `Add`.
    let mut iter = terms.into_iter();
    let mut acc = iter.next().unwrap();
    for t in iter {
        acc = Expr::Add(Box::new(acc), Box::new(t));
    }
    Ok(acc)
}

/// Symbolically solve an equation or zero-expression for `var`, returning a Maple `List` of roots.
pub fn solve(expr: &Expr, var: &str) -> Result<Expr> {
    let target = match expr {
        Expr::Equation(lhs, rhs) => simplify(&Expr::Sub(lhs.clone(), rhs.clone())),
        other => simplify(other),
    };

    if let Some(poly) = extract_poly(&target, var) {
        let roots = solve_polynomial(&poly)?;
        return Ok(Expr::List(roots));
    }

    if let Some(single) = solve_elementary(&target, var) {
        return Ok(Expr::List(vec![simplify(&single)]));
    }

    bail!("unable to symbolically solve `{expr}` for `{var}`")
}

fn solve_polynomial(poly: &[Rational]) -> Result<Vec<Expr>> {
    let deg = poly.len().saturating_sub(1);
    match deg {
        0 => Ok(Vec::new()),
        1 => {
            let root = -(poly[0] / poly[1]);
            Ok(vec![Expr::Rational(root)])
        }
        2 => Ok(solve_quadratic(poly[2], poly[1], poly[0])),
        _ => solve_higher_poly(poly),
    }
}

fn solve_quadratic(a: Rational, b: Rational, c: Rational) -> Vec<Expr> {
    let disc = b * b - Rational::from_int(4) * a * c;
    let two_a = Rational::from_int(2) * a;
    if disc.is_zero() {
        return vec![Expr::Rational(-(b / two_a))];
    }
    if disc.is_positive() {
        let sqrt_d = simplify(&Expr::Call("sqrt".to_string(), vec![Expr::Rational(disc)]));
        let r1 = simplify(&Expr::Div(
            Box::new(Expr::Sub(
                Box::new(Expr::Rational(-b)),
                Box::new(sqrt_d.clone()),
            )),
            Box::new(Expr::Rational(two_a)),
        ));
        let r2 = simplify(&Expr::Div(
            Box::new(Expr::Add(Box::new(Expr::Rational(-b)), Box::new(sqrt_d))),
            Box::new(Expr::Rational(two_a)),
        ));
        let mut pair = vec![r1, r2];
        pair.sort_by(|x, y| {
            evalf(x)
                .unwrap_or(0.0)
                .partial_cmp(&evalf(y).unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        pair
    } else {
        let sqrt_neg_d = simplify(&Expr::Call("sqrt".to_string(), vec![Expr::Rational(-disc)]));
        let imag_part = Expr::Mul(
            Box::new(sqrt_neg_d),
            Box::new(Expr::Symbol("I".to_string())),
        );
        let r1 = simplify(&Expr::Div(
            Box::new(Expr::Sub(
                Box::new(Expr::Rational(-b)),
                Box::new(imag_part.clone()),
            )),
            Box::new(Expr::Rational(two_a)),
        ));
        let r2 = simplify(&Expr::Div(
            Box::new(Expr::Add(Box::new(Expr::Rational(-b)), Box::new(imag_part))),
            Box::new(Expr::Rational(two_a)),
        ));
        vec![r1, r2]
    }
}

fn solve_higher_poly(poly: &[Rational]) -> Result<Vec<Expr>> {
    // Factor out rational roots iteratively and finish quadratic remainder if reached.
    let mut rem = poly.to_vec();
    let mut roots = Vec::new();
    while rem.len() > 1 && rem[0].is_zero() {
        roots.push(Expr::Rational(Rational::zero()));
        rem.remove(0);
    }
    while rem.len() > 3 {
        let c0 = rem[0].num.unsigned_abs();
        let cn = rem[rem.len() - 1].num.unsigned_abs();
        let mut found = None;
        for p in small_divisors(c0) {
            for q in small_divisors(cn) {
                if let Ok(r) = Rational::new(p, q) {
                    if eval_poly_rat(&rem, r).is_zero() {
                        found = Some(r);
                        break;
                    }
                    if eval_poly_rat(&rem, -r).is_zero() {
                        found = Some(-r);
                        break;
                    }
                }
            }
            if found.is_some() {
                break;
            }
        }
        if let Some(r) = found {
            roots.push(Expr::Rational(r));
            rem = divide_poly_linear(&rem, r);
        } else {
            break;
        }
    }
    if rem.len() == 2 {
        roots.push(Expr::Rational(-(rem[0] / rem[1])));
    } else if rem.len() == 3 {
        roots.extend(solve_quadratic(rem[2], rem[1], rem[0]));
    } else if roots.is_empty() {
        bail!(
            "cannot find closed-form roots for polynomial of degree {}",
            poly.len() - 1
        );
    }
    roots.sort_by(|x, y| {
        evalf(x)
            .unwrap_or(0.0)
            .partial_cmp(&evalf(y).unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(roots)
}

fn small_divisors(n: u128) -> Vec<i128> {
    let mut out = Vec::new();
    let mut d = 1u128;
    while d * d <= n && out.len() < 64 {
        if n.is_multiple_of(d) {
            out.push(d as i128);
            if d * d != n {
                out.push((n / d) as i128);
            }
        }
        d += 1;
    }
    out
}

fn eval_poly_rat(poly: &[Rational], r: Rational) -> Rational {
    let mut acc = Rational::zero();
    for &c in poly.iter().rev() {
        acc = acc * r + c;
    }
    acc
}

fn divide_poly_linear(poly: &[Rational], root: Rational) -> Vec<Rational> {
    let n = poly.len();
    let mut q = vec![Rational::zero(); n - 1];
    let mut carry = poly[n - 1];
    q[n - 2] = carry;
    for i in (1..n - 1).rev() {
        carry = poly[i] + carry * root;
        q[i - 1] = carry;
    }
    q
}

fn solve_elementary(expr: &Expr, var: &str) -> Option<Expr> {
    // Isolates `var` when `expr` splits into `f(var_part) + const = 0`.
    isolate_var(expr, &Expr::Rational(Rational::zero()), var)
}

fn isolate_var(lhs: &Expr, rhs: &Expr, var: &str) -> Option<Expr> {
    if let Expr::Symbol(s) = lhs
        && s == var
    {
        return Some(rhs.clone());
    }
    match lhs {
        Expr::UnaryNeg(a) => isolate_var(a, &Expr::UnaryNeg(Box::new(rhs.clone())), var),
        Expr::Add(a, b) => {
            if !depends_on(b, var) {
                isolate_var(a, &Expr::Sub(Box::new(rhs.clone()), b.clone()), var)
            } else if !depends_on(a, var) {
                isolate_var(b, &Expr::Sub(Box::new(rhs.clone()), a.clone()), var)
            } else {
                None
            }
        }
        Expr::Sub(a, b) => {
            if !depends_on(b, var) {
                isolate_var(a, &Expr::Add(Box::new(rhs.clone()), b.clone()), var)
            } else if !depends_on(a, var) {
                isolate_var(b, &Expr::Sub(a.clone(), Box::new(rhs.clone())), var)
            } else {
                None
            }
        }
        Expr::Mul(a, b) => {
            if !depends_on(b, var) {
                isolate_var(a, &Expr::Div(Box::new(rhs.clone()), b.clone()), var)
            } else if !depends_on(a, var) {
                isolate_var(b, &Expr::Div(Box::new(rhs.clone()), a.clone()), var)
            } else {
                None
            }
        }
        Expr::Div(a, b) if !depends_on(b, var) => {
            isolate_var(a, &Expr::Mul(Box::new(rhs.clone()), b.clone()), var)
        }
        Expr::Call(name, args) if args.len() == 1 => {
            let inv = match name.as_str() {
                "exp" => Expr::Call("ln".to_string(), vec![rhs.clone()]),
                "ln" | "log" => Expr::Call("exp".to_string(), vec![rhs.clone()]),
                "sin" => Expr::Call("arcsin".to_string(), vec![rhs.clone()]),
                "cos" => Expr::Call("arccos".to_string(), vec![rhs.clone()]),
                "tan" => Expr::Call("arctan".to_string(), vec![rhs.clone()]),
                "sqrt" => Expr::Pow(
                    Box::new(rhs.clone()),
                    Box::new(Expr::Rational(Rational::from_int(2))),
                ),
                _ => return None,
            };
            isolate_var(&args[0], &inv, var)
        }
        _ => None,
    }
}
