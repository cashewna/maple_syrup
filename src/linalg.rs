use crate::ast::Expr;
use crate::calculus::solve;
use crate::rational::Rational;
use crate::simplify::{evalf, simplify};
use anyhow::{Result, bail};

/// Construct a validated `Expr::Matrix` from Maple `Matrix(...)` arguments.
pub fn build_matrix(args: &[Expr]) -> Result<Expr> {
    if args.len() == 1 {
        return match &args[0] {
            Expr::List(rows_raw) => parse_nested_rows(rows_raw),
            Expr::Matrix(rows) => validate_matrix(rows.clone()),
            other => bail!("`Matrix` expects a list of rows, got `{other}`"),
        };
    }
    if args.len() == 3 {
        let rows = match &args[0] {
            Expr::Rational(r) if r.is_integer() && r.num > 0 => r.num as usize,
            other => bail!("invalid row count `{other}`"),
        };
        let cols = match &args[1] {
            Expr::Rational(r) if r.is_integer() && r.num > 0 => r.num as usize,
            other => bail!("invalid column count `{other}`"),
        };
        if let Expr::List(items) = &args[2] {
            if items.len() == rows && items.iter().all(|x| matches!(x, Expr::List(_))) {
                return parse_nested_rows(items);
            }
            if items.len() == rows * cols {
                let mut mat = Vec::with_capacity(rows);
                for r in 0..rows {
                    mat.push(items[r * cols..(r + 1) * cols].to_vec());
                }
                return validate_matrix(mat);
            }
        }
    }
    bail!("unsupported `Matrix(...)` arguments")
}

fn parse_nested_rows(rows_raw: &[Expr]) -> Result<Expr> {
    if rows_raw.is_empty() {
        bail!("matrix cannot have zero rows");
    }
    let mut rows = Vec::with_capacity(rows_raw.len());
    for row in rows_raw {
        match row {
            Expr::List(cells) => rows.push(cells.iter().map(simplify).collect()),
            other => bail!("matrix row must be a list `[...]`, got `{other}`"),
        }
    }
    validate_matrix(rows)
}

fn validate_matrix(rows: Vec<Vec<Expr>>) -> Result<Expr> {
    if rows.is_empty() || rows[0].is_empty() {
        bail!("matrix dimensions must be non-zero");
    }
    let cols = rows[0].len();
    for (idx, r) in rows.iter().enumerate() {
        if r.len() != cols {
            bail!(
                "inconsistent row length at row {}: expected {cols}, got {}",
                idx + 1,
                r.len()
            );
        }
    }
    Ok(Expr::Matrix(rows))
}

fn as_matrix(expr: &Expr) -> Result<&Vec<Vec<Expr>>> {
    match expr {
        Expr::Matrix(rows) => Ok(rows),
        other => bail!("expected a Matrix, got `{other}`"),
    }
}

fn try_as_rational_matrix(rows: &[Vec<Expr>]) -> Option<Vec<Vec<Rational>>> {
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let mut rrow = Vec::with_capacity(row.len());
        for cell in row {
            match simplify(cell) {
                Expr::Rational(r) => rrow.push(r),
                _ => return None,
            }
        }
        out.push(rrow);
    }
    Some(out)
}

fn from_rational_matrix(rows: Vec<Vec<Rational>>) -> Expr {
    Expr::Matrix(
        rows.into_iter()
            .map(|r| r.into_iter().map(Expr::Rational).collect())
            .collect(),
    )
}

/// Matrix transpose.
pub fn transpose(expr: &Expr) -> Result<Expr> {
    let rows = as_matrix(expr)?;
    let nrows = rows.len();
    let ncols = rows[0].len();
    let mut out = vec![vec![Expr::Rational(Rational::zero()); nrows]; ncols];
    for (i, row) in rows.iter().enumerate() {
        for (j, val) in row.iter().enumerate() {
            out[j][i] = val.clone();
        }
    }
    Ok(Expr::Matrix(out))
}

/// Matrix trace (sum of diagonal entries).
pub fn trace(expr: &Expr) -> Result<Expr> {
    let rows = as_matrix(expr)?;
    let n = rows.len();
    if rows[0].len() != n {
        bail!(
            "`trace` requires a square matrix, got {}x{}",
            n,
            rows[0].len()
        );
    }
    let mut acc = Expr::Rational(Rational::zero());
    for (i, row) in rows.iter().enumerate() {
        acc = Expr::Add(Box::new(acc), Box::new(row[i].clone()));
    }
    Ok(simplify(&acc))
}

/// Matrix addition or subtraction (`sign = 1` or `-1`).
pub fn add_matrices(a: &Expr, b: &Expr, subtract: bool) -> Result<Expr> {
    let ma = as_matrix(a)?;
    let mb = as_matrix(b)?;
    if ma.len() != mb.len() || ma[0].len() != mb[0].len() {
        bail!(
            "matrix dimension mismatch: {}x{} vs {}x{}",
            ma.len(),
            ma[0].len(),
            mb.len(),
            mb[0].len()
        );
    }
    let mut out = Vec::with_capacity(ma.len());
    for (ra, rb) in ma.iter().zip(mb.iter()) {
        let mut row = Vec::with_capacity(ra.len());
        for (ca, cb) in ra.iter().zip(rb.iter()) {
            let cell = if subtract {
                Expr::Sub(Box::new(ca.clone()), Box::new(cb.clone()))
            } else {
                Expr::Add(Box::new(ca.clone()), Box::new(cb.clone()))
            };
            row.push(simplify(&cell));
        }
        out.push(row);
    }
    Ok(Expr::Matrix(out))
}

/// Scale matrix by scalar expression.
pub fn scale_matrix(scalar: &Expr, mat: &Expr) -> Result<Expr> {
    let rows = as_matrix(mat)?;
    let out = rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|c| simplify(&Expr::Mul(Box::new(scalar.clone()), Box::new(c.clone()))))
                .collect()
        })
        .collect();
    Ok(Expr::Matrix(out))
}

/// Matrix multiplication (`A * B` or `Multiply(A, B)`).
pub fn multiply_matrices(a: &Expr, b: &Expr) -> Result<Expr> {
    let ma = as_matrix(a)?;
    let mb = as_matrix(b)?;
    let r = ma.len();
    let k = ma[0].len();
    let c = mb[0].len();
    if k != mb.len() {
        bail!(
            "incompatible matrix multiplication dimensions: {}x{} * {}x{}",
            r,
            k,
            mb.len(),
            c
        );
    }
    let mut out = vec![vec![Expr::Rational(Rational::zero()); c]; r];
    for i in 0..r {
        for j in 0..c {
            let mut sum = Expr::Rational(Rational::zero());
            for p in 0..k {
                let prod = Expr::Mul(Box::new(ma[i][p].clone()), Box::new(mb[p][j].clone()));
                sum = Expr::Add(Box::new(sum), Box::new(prod));
            }
            out[i][j] = simplify(&sum);
        }
    }
    Ok(Expr::Matrix(out))
}

/// Exact matrix determinant (`det` / `Determinant`).
pub fn determinant(expr: &Expr) -> Result<Expr> {
    let rows = as_matrix(expr)?;
    let n = rows.len();
    if rows[0].len() != n {
        bail!(
            "`det` requires a square matrix, got {}x{}",
            n,
            rows[0].len()
        );
    }
    if let Some(mut rmat) = try_as_rational_matrix(rows) {
        let mut sign = 1i128;
        let mut det = Rational::one();
        for col in 0..n {
            let Some(pivot) = (col..n).find(|&r| !rmat[r][col].is_zero()) else {
                return Ok(Expr::Rational(Rational::zero()));
            };
            if pivot != col {
                rmat.swap(pivot, col);
                sign = -sign;
            }
            let piv_val = rmat[col][col];
            det = det.checked_mul(piv_val)?;
            for r in (col + 1)..n {
                let factor = rmat[r][col].checked_div(piv_val)?;
                if !factor.is_zero() {
                    let (top, bottom) = rmat.split_at_mut(r);
                    let piv_row = &top[col];
                    let target_row = &mut bottom[0];
                    for (cell, piv_cell) in target_row[col..n].iter_mut().zip(&piv_row[col..n]) {
                        let delta = factor.checked_mul(*piv_cell)?;
                        *cell = cell.checked_sub(delta)?;
                    }
                }
            }
        }
        return Ok(Expr::Rational(det.checked_mul(Rational::from_int(sign))?));
    }

    // Symbolic Laplace cofactor fallback for small symbolic matrices.
    if n > 6 {
        bail!("symbolic determinant only supported up to 6x6 matrices");
    }
    Ok(symbolic_det(rows))
}

fn symbolic_det(rows: &[Vec<Expr>]) -> Expr {
    let n = rows.len();
    if n == 1 {
        return simplify(&rows[0][0]);
    }
    if n == 2 {
        return simplify(&Expr::Sub(
            Box::new(Expr::Mul(
                Box::new(rows[0][0].clone()),
                Box::new(rows[1][1].clone()),
            )),
            Box::new(Expr::Mul(
                Box::new(rows[0][1].clone()),
                Box::new(rows[1][0].clone()),
            )),
        ));
    }
    let mut acc = Expr::Rational(Rational::zero());
    for col in 0..n {
        if matches!(rows[0][col], Expr::Rational(r) if r.is_zero()) {
            continue;
        }
        let minor: Vec<Vec<Expr>> = rows[1..]
            .iter()
            .map(|r| {
                r.iter()
                    .enumerate()
                    .filter_map(|(j, v)| (j != col).then_some(v.clone()))
                    .collect()
            })
            .collect();
        let term = Expr::Mul(
            Box::new(rows[0][col].clone()),
            Box::new(symbolic_det(&minor)),
        );
        acc = if col % 2 == 0 {
            Expr::Add(Box::new(acc), Box::new(term))
        } else {
            Expr::Sub(Box::new(acc), Box::new(term))
        };
    }
    simplify(&acc)
}

/// Reduced Row Echelon Form (`rref` / `ReducedRowEchelonForm`).
pub fn rref(expr: &Expr) -> Result<Expr> {
    let rows = as_matrix(expr)?;
    if let Some(rmat) = try_as_rational_matrix(rows) {
        return Ok(from_rational_matrix(rref_rational(rmat)?));
    }
    Ok(Expr::Matrix(rref_symbolic(rows.clone())))
}

fn rref_rational(mut mat: Vec<Vec<Rational>>) -> Result<Vec<Vec<Rational>>> {
    let nrows = mat.len();
    let ncols = mat[0].len();
    let mut lead = 0usize;
    for r in 0..nrows {
        if lead >= ncols {
            break;
        }
        let mut i = r;
        while mat[i][lead].is_zero() {
            i += 1;
            if i == nrows {
                i = r;
                lead += 1;
                if lead == ncols {
                    return Ok(mat);
                }
            }
        }
        mat.swap(i, r);
        let div = mat[r][lead];
        if !div.is_zero() {
            for cell in &mut mat[r] {
                *cell = cell.checked_div(div)?;
            }
        }
        for other in 0..nrows {
            if other != r && !mat[other][lead].is_zero() {
                let factor = mat[other][lead];
                let piv_row = mat[r].clone();
                for (cell, piv_cell) in mat[other].iter_mut().zip(&piv_row) {
                    let sub = factor.checked_mul(*piv_cell)?;
                    *cell = cell.checked_sub(sub)?;
                }
            }
        }
        lead += 1;
    }
    Ok(mat)
}

fn rref_symbolic(mut mat: Vec<Vec<Expr>>) -> Vec<Vec<Expr>> {
    let nrows = mat.len();
    let ncols = mat[0].len();
    let mut lead = 0usize;
    for r in 0..nrows {
        if lead >= ncols {
            break;
        }
        let mut i = r;
        while matches!(mat[i][lead], Expr::Rational(x) if x.is_zero()) {
            i += 1;
            if i == nrows {
                i = r;
                lead += 1;
                if lead == ncols {
                    return mat;
                }
            }
        }
        mat.swap(i, r);
        let div = mat[r][lead].clone();
        for cell in &mut mat[r] {
            *cell = simplify(&Expr::Div(Box::new(cell.clone()), Box::new(div.clone())));
        }
        for other in 0..nrows {
            if other != r && !matches!(mat[other][lead], Expr::Rational(x) if x.is_zero()) {
                let factor = mat[other][lead].clone();
                let piv_row = mat[r].clone();
                for (cell, piv_cell) in mat[other].iter_mut().zip(&piv_row) {
                    let prod = Expr::Mul(Box::new(factor.clone()), Box::new(piv_cell.clone()));
                    *cell = simplify(&Expr::Sub(Box::new(cell.clone()), Box::new(prod)));
                }
            }
        }
        lead += 1;
    }
    mat
}

/// Exact matrix inverse via Gauss-Jordan `[A | I]` elimination.
pub fn inverse(expr: &Expr) -> Result<Expr> {
    let rows = as_matrix(expr)?;
    let n = rows.len();
    if rows[0].len() != n {
        bail!(
            "`Inverse` requires a square matrix, got {}x{}",
            n,
            rows[0].len()
        );
    }
    if let Some(rmat) = try_as_rational_matrix(rows) {
        let mut aug = Vec::with_capacity(n);
        for (i, row) in rmat.into_iter().enumerate() {
            let mut arow = row;
            for j in 0..n {
                arow.push(if i == j {
                    Rational::one()
                } else {
                    Rational::zero()
                });
            }
            aug.push(arow);
        }
        let reduced = rref_rational(aug)?;
        for (i, row) in reduced.iter().enumerate() {
            for (j, val) in row.iter().take(n).enumerate() {
                let expected = if i == j {
                    Rational::one()
                } else {
                    Rational::zero()
                };
                if *val != expected {
                    bail!("matrix is singular and not invertible");
                }
            }
        }
        let inv = reduced.into_iter().map(|r| r[n..].to_vec()).collect();
        return Ok(from_rational_matrix(inv));
    }

    // Symbolic Gauss-Jordan fallback.
    let mut aug = Vec::with_capacity(n);
    for (i, row) in rows.iter().enumerate() {
        let mut arow = row.clone();
        for j in 0..n {
            arow.push(Expr::Rational(if i == j {
                Rational::one()
            } else {
                Rational::zero()
            }));
        }
        aug.push(arow);
    }
    let reduced = rref_symbolic(aug);
    for (i, row) in reduced.iter().enumerate() {
        if !matches!(row[i], Expr::Rational(x) if x.is_one()) {
            bail!("matrix is singular and not invertible");
        }
    }
    Ok(Expr::Matrix(
        reduced.into_iter().map(|r| r[n..].to_vec()).collect(),
    ))
}

/// Matrix integer power `A^k` (supports negative exponents via `Inverse`).
pub fn matrix_pow(expr: &Expr, exp: i32) -> Result<Expr> {
    let rows = as_matrix(expr)?;
    let n = rows.len();
    if rows[0].len() != n {
        bail!("matrix power requires a square matrix");
    }
    if exp == 0 {
        let mut ident = vec![vec![Expr::Rational(Rational::zero()); n]; n];
        for (i, row) in ident.iter_mut().enumerate() {
            row[i] = Expr::Rational(Rational::one());
        }
        return Ok(Expr::Matrix(ident));
    }
    let (mut base, mut p) = if exp < 0 {
        (inverse(expr)?, exp.unsigned_abs())
    } else {
        (expr.clone(), exp as u32)
    };
    let mut acc: Option<Expr> = None;
    while p > 0 {
        if p % 2 == 1 {
            acc = Some(match acc {
                Some(cur) => multiply_matrices(&cur, &base)?,
                None => base.clone(),
            });
        }
        p /= 2;
        if p > 0 {
            base = multiply_matrices(&base, &base)?;
        }
    }
    Ok(acc.unwrap())
}

/// Exact eigenvalues for 1x1, 2x2, and triangular matrices.
pub fn eigenvalues(expr: &Expr) -> Result<Expr> {
    let rows = as_matrix(expr)?;
    let n = rows.len();
    if rows[0].len() != n {
        bail!(
            "`Eigenvalues` requires a square matrix, got {}x{}",
            n,
            rows[0].len()
        );
    }
    if n == 1 {
        return Ok(Expr::List(vec![simplify(&rows[0][0])]));
    }
    if n == 2 {
        let tr = trace(expr)?;
        let det = determinant(expr)?;
        let lam = Expr::Symbol("__lambda".to_string());
        let char_poly = Expr::Add(
            Box::new(Expr::Sub(
                Box::new(Expr::Pow(
                    Box::new(lam.clone()),
                    Box::new(Expr::Rational(Rational::from_int(2))),
                )),
                Box::new(Expr::Mul(Box::new(tr), Box::new(lam))),
            )),
            Box::new(det),
        );
        return solve(&char_poly, "__lambda");
    }
    let is_upper =
        (1..n).all(|i| (0..i).all(|j| matches!(rows[i][j], Expr::Rational(r) if r.is_zero())));
    let is_lower = (0..n)
        .all(|i| ((i + 1)..n).all(|j| matches!(rows[i][j], Expr::Rational(r) if r.is_zero())));
    if is_upper || is_lower {
        let mut diag: Vec<Expr> = (0..n).map(|i| simplify(&rows[i][i])).collect();
        diag.sort_by(|a, b| {
            evalf(a)
                .unwrap_or(0.0)
                .partial_cmp(&evalf(b).unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        return Ok(Expr::List(diag));
    }
    bail!("`Eigenvalues` currently supports 1x1, 2x2, and triangular matrices (got {n}x{n})")
}
