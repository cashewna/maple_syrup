use maple_gpui::ast::{Expr, to_latex};
use maple_gpui::env::{Environment, EvalOutput};
use maple_gpui::parser::parse_expr;
use maple_gpui::rational::Rational;
use maple_gpui::simplify::simplify;
use maple_gpui::worksheet::{CellOutput, MatrixPresetKind, MatrixTemplate, Worksheet};

fn eval_single_text(env: &mut Environment, input: &str) -> String {
    let mut outputs = env.eval_line(input).expect("evaluation should succeed");
    assert_eq!(outputs.len(), 1, "expected 1 output for `{input}`");
    match outputs.pop().unwrap() {
        EvalOutput::Value(expr) => expr.to_string(),
        other => panic!("expected Value output for `{input}`, got {other:?}"),
    }
}

#[test]
fn rational_exact_arithmetic_reduces_fractions() {
    let a = Rational::new(1, 3).unwrap();
    let b = Rational::new(1, 6).unwrap();
    let sum = a + b;
    assert_eq!(sum, Rational::new(1, 2).unwrap());
    assert_eq!(sum.to_string(), "1/2");

    let neg = Rational::new(2, -4).unwrap();
    assert_eq!(neg, Rational::new(-1, 2).unwrap());
    assert_eq!(neg.to_string(), "-1/2");

    let prod = Rational::new(3, 4).unwrap() * Rational::new(8, 9).unwrap();
    assert_eq!(prod, Rational::new(2, 3).unwrap());
}

#[test]
fn simplifier_reduces_identities_like_terms_and_powers() {
    let expr1 = parse_expr("(x + 0) * 1").unwrap();
    assert_eq!(simplify(&expr1).to_string(), "x");

    let expr2 = parse_expr("2*x + 3*x").unwrap();
    assert_eq!(simplify(&expr2).to_string(), "5*x");

    let expr3 = parse_expr("x^2 * x^3").unwrap();
    assert_eq!(simplify(&expr3).to_string(), "x^5");

    let expr4 = parse_expr("1/3 + 1/6").unwrap();
    assert_eq!(simplify(&expr4).to_string(), "1/2");
}

#[test]
fn environment_supports_maple_assignments_suppression_functions_and_history() {
    let mut env = Environment::new();

    // Assignment with semicolon produces output and binds variable.
    let out = eval_single_text(&mut env, "f := x^3 - 3*x;");
    assert_eq!(out, "x^3 - 3*x");
    assert!(env.variables.contains_key("f"));

    // Colon suppresses output while still binding variable.
    let suppressed = env.eval_line("secret := 42:").unwrap();
    assert_eq!(suppressed, vec![EvalOutput::Suppressed]);
    assert_eq!(
        env.variables.get("secret").unwrap(),
        &Expr::Rational(Rational::from_int(42))
    );

    // Arrow function definition and invocation.
    let g_def = eval_single_text(&mut env, "g := x -> x^2 + 2*x + 1;");
    assert!(g_def.contains("->") || g_def.contains("x^2"));
    assert_eq!(eval_single_text(&mut env, "g(3);"), "16");

    // History registers % (last output = 16) and %% (second last = g or 42).
    // Note: in Maple, colon-suppressed results still update % or only displayed?
    // Wait: after g(3) = 16, let's compute 10 + 5 = 15, then % = 15, %% = 16.
    assert_eq!(eval_single_text(&mut env, "10 + 5;"), "15");
    assert_eq!(eval_single_text(&mut env, "% + 1;"), "16");
    assert_eq!(eval_single_text(&mut env, "%% + 2;"), "17");
}

#[test]
fn environment_evaluates_expand_factor_subs_and_evalf() {
    let mut env = Environment::new();

    assert_eq!(
        eval_single_text(&mut env, "expand((x + 2)*(x - 3));"),
        "x^2 - x - 6"
    );
    assert_eq!(
        eval_single_text(&mut env, "factor(x^2 - 5*x + 6);"),
        "(x - 2)*(x - 3)"
    );
    assert_eq!(eval_single_text(&mut env, "subs(x = 4, x^2 + 1);"), "17");
    let pi_approx = eval_single_text(&mut env, "evalf(Pi);");
    let parsed_pi: f64 = pi_approx.parse().expect("evalf(Pi) should parse as f64");
    assert!((parsed_pi - std::f64::consts::PI).abs() < 1e-12);
}

#[test]
fn calculus_engine_differentiates_symbolically_including_higher_order() {
    let mut env = Environment::new();

    assert_eq!(
        eval_single_text(&mut env, "diff(x^4 - 3*x^2 + 7, x);"),
        "4*x^3 - 6*x"
    );

    // Product rule: check algebraic equivalence against independent literal `2*x*sin(x) + x^2*cos(x)`.
    let prod_diff = eval_single_text(&mut env, "diff(x^2 * sin(x), x);");
    let check_diff = eval_single_text(
        &mut env,
        &format!("simplify(({prod_diff}) - (2*x*sin(x) + x^2*cos(x)));"),
    );
    assert_eq!(check_diff, "0", "got derivative `{prod_diff}`");

    assert_eq!(
        eval_single_text(&mut env, "diff(ln(x^2 + 1), x);"),
        "2*x/(x^2 + 1)"
    );
    assert_eq!(eval_single_text(&mut env, "diff(sin(x), x$2);"), "-sin(x)");
    assert_eq!(eval_single_text(&mut env, "diff(sin(x), x, x);"), "-sin(x)");
}

#[test]
fn calculus_engine_integrates_indefinite_and_definite_integrals() {
    let mut env = Environment::new();

    assert_eq!(
        eval_single_text(&mut env, "int(3*x^2 + 2*x, x);"),
        "x^3 + x^2"
    );
    assert_eq!(
        eval_single_text(&mut env, "int(cos(2*x), x);"),
        "1/2*sin(2*x)"
    );
    assert_eq!(eval_single_text(&mut env, "int(x^2, x = 0..3);"), "9");
    assert_eq!(eval_single_text(&mut env, "int(1/x, x = 1..e);"), "1");
}

#[test]
fn calculus_engine_computes_limits_series_and_solutions() {
    let mut env = Environment::new();

    assert_eq!(eval_single_text(&mut env, "limit(sin(x)/x, x = 0);"), "1");
    assert_eq!(
        eval_single_text(&mut env, "limit((x^2 - 4)/(x - 2), x = 2);"),
        "4"
    );
    assert_eq!(
        eval_single_text(&mut env, "limit((1 - cos(x))/x^2, x = 0);"),
        "1/2"
    );

    assert_eq!(
        eval_single_text(&mut env, "series(exp(x), x = 0, 4);"),
        "1 + x + 1/2*x^2 + 1/6*x^3 + 1/24*x^4"
    );
    assert_eq!(
        eval_single_text(&mut env, "series(sin(x), x = 0, 5);"),
        "x - 1/6*x^3 + 1/120*x^5"
    );

    assert_eq!(eval_single_text(&mut env, "solve(2*x - 6 = 0, x);"), "[3]");
    assert_eq!(
        eval_single_text(&mut env, "solve(x^2 - 5*x + 6 = 0, x);"),
        "[2, 3]"
    );
    assert_eq!(
        eval_single_text(&mut env, "solve(x^2 - 2 = 0, x);"),
        "[-sqrt(2), sqrt(2)]"
    );
}

#[test]
fn linear_algebra_engine_performs_exact_matrix_operations() {
    let mut env = Environment::new();

    assert_eq!(
        eval_single_text(&mut env, "A := Matrix([[1, 2], [3, 4]]);"),
        "Matrix([[1, 2], [3, 4]])"
    );
    assert_eq!(eval_single_text(&mut env, "A[2, 1];"), "3");
    assert_eq!(eval_single_text(&mut env, "det(A);"), "-2");
    assert_eq!(
        eval_single_text(&mut env, "Transpose(A);"),
        "Matrix([[1, 3], [2, 4]])"
    );
    assert_eq!(
        eval_single_text(&mut env, "Inverse(A);"),
        "Matrix([[-2, 1], [3/2, -1/2]])"
    );
    assert_eq!(eval_single_text(&mut env, "trace(A);"), "5");

    assert_eq!(
        eval_single_text(
            &mut env,
            "rref(Matrix([[1, 2, -1, -4], [2, 3, -1, -11], [-2, 0, -3, 22]]));"
        ),
        "Matrix([[1, 0, 0, -8], [0, 1, 0, 1], [0, 0, 1, -2]])"
    );

    assert_eq!(
        eval_single_text(&mut env, "Eigenvalues(Matrix([[4, 1], [2, 3]]));"),
        "[2, 5]"
    );
}

#[test]
fn worksheet_evaluates_plots_and_matrices() {
    let mut ws = Worksheet::new();

    let plot_cells = ws.evaluate("plot(sin(x), x = -Pi..Pi);");
    assert_eq!(plot_cells.len(), 1);
    match &plot_cells[0].output {
        CellOutput::Plot(plot_data) => {
            assert_eq!(plot_data.series.len(), 1);
            let pts = &plot_data.series[0].points;
            assert!(
                pts.len() > 100,
                "expected >100 plot samples, got {}",
                pts.len()
            );
            for &(x, y) in pts {
                assert!(
                    x.is_finite() && y.is_finite(),
                    "non-finite sample ({x}, {y})"
                );
                assert!(
                    (-1.05..=1.05).contains(&y),
                    "sin(x) out of bounds at x={x}: y={y}"
                );
            }
        }
        other => panic!("expected Plot output, got {other:?}"),
    }

    let mat_cells = ws.evaluate("Matrix([[1, 2], [3, 4]]);");
    assert_eq!(mat_cells.len(), 1);
    assert_eq!(
        mat_cells[0].output,
        CellOutput::MatrixGrid(vec![
            vec!["1".to_string(), "2".to_string()],
            vec!["3".to_string(), "4".to_string()],
        ])
    );

    let err_cells = ws.evaluate("1 / ;");
    assert_eq!(err_cells.len(), 1);
    assert!(matches!(err_cells[0].output, CellOutput::Error(_)));
}

#[test]
fn worksheet_execute_all_replays_cells_from_clean_kernel() {
    let mut ws = Worksheet::new();
    ws.evaluate("a := 2;");
    ws.evaluate("a * 10;");
    assert_eq!(ws.cells[1].output, CellOutput::Text("20".to_string()));

    ws.cells[0].input = "a := 7;".to_string();
    ws.execute_all();

    assert_eq!(ws.cells.len(), 2);
    assert_eq!(ws.cells[0].index, 1);
    assert_eq!(ws.cells[0].output, CellOutput::Text("7".to_string()));
    assert_eq!(ws.cells[1].index, 2);
    assert_eq!(ws.cells[1].output, CellOutput::Text("70".to_string()));
}

#[test]
fn matrix_template_generates_zero_identity_sequential_and_symbolic_presets() {
    assert_eq!(
        MatrixTemplate::build_expr(2, 2, MatrixPresetKind::Identity),
        "Matrix([[1, 0], [0, 1]]);"
    );
    assert_eq!(
        MatrixTemplate::build_expr(2, 3, MatrixPresetKind::Zero),
        "Matrix([[0, 0, 0], [0, 0, 0]]);"
    );
    assert_eq!(
        MatrixTemplate::build_expr(2, 2, MatrixPresetKind::Sequential),
        "Matrix([[1, 2], [3, 4]]);"
    );
    assert_eq!(
        MatrixTemplate::build_expr(2, 2, MatrixPresetKind::Symbolic),
        "Matrix([[a11, a12], [a21, a22]]);"
    );
}

#[test]
fn worksheet_cell_context_actions_match_output_kind() {
    let mut ws = Worksheet::new();
    let mat_cell = ws.evaluate("Matrix([[1, 2], [3, 4]]);").pop().unwrap();
    let mat_actions = mat_cell.context_actions();
    assert!(mat_actions.contains(&("Determinant", "det(%);".to_string())));
    assert!(mat_actions.contains(&("Inverse", "Inverse(%);".to_string())));
    assert!(mat_actions.contains(&("Transpose", "Transpose(%);".to_string())));
    assert!(mat_actions.contains(&("RREF", "rref(%);".to_string())));
    assert!(mat_actions.contains(&("Eigenvalues", "Eigenvalues(%);".to_string())));
    assert!(mat_actions.contains(&("Trace", "trace(%);".to_string())));

    let expr_cell = ws.evaluate("x^2 + 3*x;").pop().unwrap();
    let expr_actions = expr_cell.context_actions();
    assert!(expr_actions.contains(&("Differentiate (d/dx)", "diff(%, x);".to_string())));
    assert!(expr_actions.contains(&("Integrate (∫ dx)", "int(%, x);".to_string())));
    assert!(expr_actions.contains(&("Simplify", "simplify(%);".to_string())));
    assert!(expr_actions.contains(&("Expand", "expand(%);".to_string())));
    assert!(expr_actions.contains(&("Factor", "factor(%);".to_string())));
    assert!(expr_actions.contains(&("Evaluate Numerically", "evalf(%);".to_string())));
    assert!(expr_actions.contains(&("Solve = 0", "solve(% = 0, x);".to_string())));
    assert!(expr_actions.contains(&("Taylor Series (deg 5)", "series(%, x = 0, 5);".to_string())));
    assert!(expr_actions.contains(&("Plot 2D (-Pi..Pi)", "plot(%, x = -Pi..Pi);".to_string())));

    let sup_cell = ws.evaluate("z := 1:").pop().unwrap();
    assert!(sup_cell.context_actions().is_empty());
}

#[test]
fn worksheet_cell_tracks_assignment_pretty_math_and_preview_plot() {
    let mut ws = Worksheet::new();
    let cell = ws.evaluate("f := x^3*sin(x);").pop().unwrap();

    assert_eq!(cell.assigned_name.as_deref(), Some("f"));
    assert!(cell.result_expr.is_some());
    assert_eq!(cell.formatted_output(), "f := x³ sin(x)");
    assert!(
        cell.preview_plot.is_some(),
        "univariate expression in x should generate a Context Panel preview plot"
    );
}

#[test]
fn ast_exports_latex_for_fractions_powers_and_matrices() {
    let expr = parse_expr("2*x/(x^2 + 1)").unwrap();
    assert_eq!(to_latex(&expr, None), r"\frac{2 x}{x^{2} + 1}");

    let assigned = parse_expr("x^3 * sin(x)").unwrap();
    assert_eq!(to_latex(&assigned, Some("f")), r"f := x^{3} \sin(x)");
}

#[test]
fn worksheet_supports_clear_outputs_and_delete_cell_renumbering() {
    let mut ws = Worksheet::new();
    ws.evaluate("a := 10;");
    ws.evaluate("b := 20;");
    ws.evaluate("a + b;");
    assert_eq!(ws.cells.len(), 3);

    ws.delete_cell(1);
    assert_eq!(ws.cells.len(), 2);
    assert_eq!(ws.cells[0].index, 1);
    assert_eq!(ws.cells[1].index, 2);
    assert_eq!(ws.cells[1].input, "a + b;");

    ws.clear_outputs();
    assert_eq!(ws.cells.len(), 2);
    assert_eq!(ws.cells[0].output, CellOutput::Suppressed);
    assert_eq!(ws.cells[1].output, CellOutput::Suppressed);
    assert!(ws.cells[0].result_expr.is_none());
}
