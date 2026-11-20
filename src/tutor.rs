/// Interactive Maple Education tutors (`DEtools[DEplot]`), Study Guides, and Help browser data.

#[derive(Clone, Debug, PartialEq)]
pub struct DeSystemPreset {
    pub id: &'static str,
    pub name: &'static str,
    pub eq1: &'static str,
    pub eq2: &'static str,
    pub default_ic: (f64, f64),
    pub x_range: (f64, f64),
    pub y_range: (f64, f64),
    pub t_span: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DePlotScene {
    pub preset: DeSystemPreset,
    pub x0: f64,
    pub y0: f64,
    /// `(x, y, dx_norm, dy_norm)` direction-field sample points on phase plane `(x, v)`.
    pub field_arrows: Vec<(f64, f64, f64, f64)>,
    /// RK4 integrated phase trajectory `(x(t), y(t))`.
    pub trajectory: Vec<(f64, f64)>,
}

const DE_PRESETS: [DeSystemPreset; 5] = [
    DeSystemPreset {
        id: "damped-oscillator",
        name: "Damped harmonic oscillator",
        eq1: "dx/dt = v",
        eq2: "dv/dt = -x - 0.35*v",
        default_ic: (1.6, 0.0),
        x_range: (-2.2, 2.2),
        y_range: (-2.2, 2.2),
        t_span: 14.0,
    },
    DeSystemPreset {
        id: "lotka-volterra",
        name: "Lotka-Volterra predator-prey",
        eq1: "dx/dt = x*(1.2 - 0.8*y)",
        eq2: "dy/dt = y*(-0.9 + 0.6*x)",
        default_ic: (1.8, 0.9),
        x_range: (0.0, 3.5),
        y_range: (0.0, 3.2),
        t_span: 12.0,
    },
    DeSystemPreset {
        id: "simple-pendulum",
        name: "Nonlinear simple pendulum",
        eq1: "dθ/dt = ω",
        eq2: "dω/dt = -sin(θ) - 0.12*ω",
        default_ic: (2.4, 0.2),
        x_range: (-3.2, 3.2),
        y_range: (-2.6, 2.6),
        t_span: 16.0,
    },
    DeSystemPreset {
        id: "van-der-pol",
        name: "Van der Pol limit-cycle oscillator",
        eq1: "dx/dt = y",
        eq2: "dy/dt = 1.2*(1 - x^2)*y - x",
        default_ic: (0.4, 0.0),
        x_range: (-2.8, 2.8),
        y_range: (-3.2, 3.2),
        t_span: 14.0,
    },
    DeSystemPreset {
        id: "saddle-linear",
        name: "Coupled linear saddle system",
        eq1: "dx/dt = 0.7*x + 0.3*y",
        eq2: "dy/dt = 0.3*x - 0.7*y",
        default_ic: (-1.4, 1.5),
        x_range: (-2.5, 2.5),
        y_range: (-2.5, 2.5),
        t_span: 3.5,
    },
];

pub fn de_presets() -> &'static [DeSystemPreset] {
    &DE_PRESETS
}

fn eval_de_rhs(id: &str, x: f64, y: f64) -> (f64, f64) {
    match id {
        "damped-oscillator" => (y, -x - 0.35 * y),
        "lotka-volterra" => (x * (1.2 - 0.8 * y), y * (-0.9 + 0.6 * x)),
        "simple-pendulum" => (y, -x.sin() - 0.12 * y),
        "van-der-pol" => (y, 1.2 * (1.0 - x * x) * y - x),
        "saddle-linear" => (0.7 * x + 0.3 * y, 0.3 * x - 0.7 * y),
        _ => (y, -x),
    }
}

/// Integrate the selected planar ODE system using 4th-order Runge-Kutta and sample slope arrows.
pub fn simulate_de_scene(preset_idx: usize, x0: f64, y0: f64) -> DePlotScene {
    let preset = DE_PRESETS
        .get(preset_idx)
        .cloned()
        .unwrap_or_else(|| DE_PRESETS[0].clone());

    let (xmin, xmax) = preset.x_range;
    let (ymin, ymax) = preset.y_range;
    let mut field_arrows = Vec::new();
    let nx = 13usize;
    let ny = 11usize;
    for ix in 0..nx {
        let fx = ix as f64 / (nx - 1) as f64;
        let px = xmin + fx * (xmax - xmin);
        for iy in 0..ny {
            let fy = iy as f64 / (ny - 1) as f64;
            let py = ymin + fy * (ymax - ymin);
            let (vx, vy) = eval_de_rhs(preset.id, px, py);
            let norm = (vx * vx + vy * vy).sqrt().max(1e-6);
            field_arrows.push((px, py, vx / norm, vy / norm));
        }
    }

    let steps = 220usize;
    let dt = preset.t_span / steps as f64;
    let mut x = x0;
    let mut y = y0;
    let mut trajectory = Vec::with_capacity(steps + 1);
    trajectory.push((x, y));
    for _ in 0..steps {
        let (k1x, k1y) = eval_de_rhs(preset.id, x, y);
        let (k2x, k2y) = eval_de_rhs(preset.id, x + 0.5 * dt * k1x, y + 0.5 * dt * k1y);
        let (k3x, k3y) = eval_de_rhs(preset.id, x + 0.5 * dt * k2x, y + 0.5 * dt * k2y);
        let (k4x, k4y) = eval_de_rhs(preset.id, x + dt * k3x, y + dt * k3y);
        x += (dt / 6.0) * (k1x + 2.0 * k2x + 2.0 * k3x + k4x);
        y += (dt / 6.0) * (k1y + 2.0 * k2y + 2.0 * k3y + k4y);
        if !x.is_finite() || !y.is_finite() || x.abs() > 1e4 || y.abs() > 1e4 {
            break;
        }
        trajectory.push((x, y));
    }

    DePlotScene {
        preset,
        x0,
        y0,
        field_arrows,
        trajectory,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StudyGuideProblem {
    pub number: usize,
    pub prompt_text: &'static str,
    pub maple_cmd: &'static str,
    pub hint: &'static str,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StudyGuideChapter {
    pub id: &'static str,
    pub guide: &'static str,
    pub title: &'static str,
    pub intro: &'static str,
    pub problems: &'static [StudyGuideProblem],
}

const PRECALC_CH1_PROBLEMS: [StudyGuideProblem; 10] = [
    StudyGuideProblem {
        number: 1,
        prompt_text: "2*x - 3 < 5",
        maple_cmd: "solve(2*x - 3 < 5, x);",
        hint: "Add 3 to both sides and divide by 2.",
    },
    StudyGuideProblem {
        number: 2,
        prompt_text: "|2*x - 3| < 5",
        maple_cmd: "solve(|2*x - 3| < 5, x);",
        hint: "Rewrite |A| < 5 as the compound inequality -5 < A < 5.",
    },
    StudyGuideProblem {
        number: 3,
        prompt_text: "|2*x - 3| < |5*x - 4|",
        maple_cmd: "solve(|2*x - 3| < |5*x - 4|, x);",
        hint: "Square both nonnegative sides and solve the resulting quadratic inequality.",
    },
    StudyGuideProblem {
        number: 4,
        prompt_text: "5 <= |6*x - 1|",
        maple_cmd: "solve(5 <= |6*x - 1|, x);",
        hint: "Split |6x - 1| >= 5 into two outer half-lines: 6x - 1 <= -5 or 6x - 1 >= 5.",
    },
    StudyGuideProblem {
        number: 5,
        prompt_text: "2/x - 1 <= 7",
        maple_cmd: "solve(2/x - 1 <= 7, x);",
        hint: "Watch the sign flip when x < 0 vs x > 0.",
    },
    StudyGuideProblem {
        number: 6,
        prompt_text: "|2/x - 1| <= 7",
        maple_cmd: "solve(|2/x - 1| <= 7, x);",
        hint: "Multiply by |x| > 0 (x != 0) or test regions around poles.",
    },
    StudyGuideProblem {
        number: 7,
        prompt_text: "|2/x - 1| < |5*x + 8|",
        maple_cmd: "solve(|2/x - 1| < |5*x + 8|, x);",
        hint: "Compare critical points where numerator or denominator changes sign.",
    },
    StudyGuideProblem {
        number: 8,
        prompt_text: "5 <= |2/x - 1|",
        maple_cmd: "solve(5 <= |2/x - 1|, x);",
        hint: "Equivalent to 2/x - 1 <= -5 or 2/x - 1 >= 5.",
    },
    StudyGuideProblem {
        number: 9,
        prompt_text: "3 < 2*x - 3 < 7",
        maple_cmd: "solve(3 < 2*x - 3 < 7, x);",
        hint: "Add 3 to all three parts simultaneously, then divide by 2.",
    },
    StudyGuideProblem {
        number: 10,
        prompt_text: "3 < |2/x - 1| < 7",
        maple_cmd: "solve(3 < |2/x - 1| < 7, x);",
        hint: "Intersect 3 < |2/x - 1| with |2/x - 1| < 7 (excluding x = 0).",
    },
];

const CALC_PROBLEMS: [StudyGuideProblem; 4] = [
    StudyGuideProblem {
        number: 1,
        prompt_text: "d/dx (x^3 * sin(x))",
        maple_cmd: "diff(x^3*sin(x), x);",
        hint: "Product rule + power & trig derivatives.",
    },
    StudyGuideProblem {
        number: 2,
        prompt_text: "∫_0^Pi sin(x) dx",
        maple_cmd: "int(sin(x), x = 0..Pi);",
        hint: "Fundamental Theorem of Calculus with F(x) = -cos(x).",
    },
    StudyGuideProblem {
        number: 3,
        prompt_text: "lim_{x->0} sin(x)/x",
        maple_cmd: "limit(sin(x)/x, x = 0);",
        hint: "L'Hopital's rule / squeeze theorem limit.",
    },
    StudyGuideProblem {
        number: 4,
        prompt_text: "Partial Fractions of 1/(x^2 - 1)",
        maple_cmd: "convert(1/(x^2 - 1), parfrac, x);",
        hint: "Factor x^2 - 1 = (x - 1)(x + 1) and solve for residues.",
    },
];

const MV_LINALG_PROBLEMS: [StudyGuideProblem; 4] = [
    StudyGuideProblem {
        number: 1,
        prompt_text: "3D Surface: z = sin(x)*cos(y)",
        maple_cmd: "plot3d(sin(x)*cos(y), x = -Pi..Pi, y = -Pi..Pi);",
        hint: "Visualizes wave interference surface in 3D space.",
    },
    StudyGuideProblem {
        number: 2,
        prompt_text: "3x3 Eigenvalues of [[4,1,0],[1,4,0],[0,0,7]]",
        maple_cmd: "Eigenvalues(Matrix([[4, 1, 0], [1, 4, 0], [0, 0, 7]]));",
        hint: "Roots of det(lambda*I - A) = 0.",
    },
    StudyGuideProblem {
        number: 3,
        prompt_text: "2nd-Order Harmonic ODE y'' + y = 0",
        maple_cmd: "dsolve(diff(y, x$2) + y = 0, y);",
        hint: "Characteristic equation r^2 + 1 = 0 yields cos(x), sin(x).",
    },
    StudyGuideProblem {
        number: 4,
        prompt_text: "Complex Arithmetic & Conjugate",
        maple_cmd: "(2 + 3*I)*(1 - I);",
        hint: "Expand using I^2 = -1.",
    },
];

const STUDY_GUIDE_CHAPTERS: [StudyGuideChapter; 3] = [
    StudyGuideChapter {
        id: "precalc-ch1",
        guide: "Precalculus Study Guide",
        title: "Chapter 1: Solving Inequalities",
        intro: "Linear, compound, absolute-value, and quadratic inequalities with exact symbolic interval solutions.",
        problems: &PRECALC_CH1_PROBLEMS,
    },
    StudyGuideChapter {
        id: "calc-study-guide",
        guide: "Calculus Study Guide",
        title: "Single-Variable Calculus & Residues",
        intro: "Differentiation, definite integration, symbolic limits, Taylor series, and partial fraction decomposition.",
        problems: &CALC_PROBLEMS,
    },
    StudyGuideChapter {
        id: "multivar-linalg",
        guide: "Multivariate Calculus & Linear Algebra",
        title: "3D Surfaces, Eigenanalysis & Differential Equations",
        intro: "3D surface plots, exact 3x3 eigenvalues/eigenvectors, complex algebra, and linear constant-coefficient ODEs.",
        problems: &MV_LINALG_PROBLEMS,
    },
];

pub fn study_guide_chapters() -> &'static [StudyGuideChapter] {
    &STUDY_GUIDE_CHAPTERS
}

#[derive(Clone, Debug, PartialEq)]
pub struct HelpArticle {
    pub id: &'static str,
    pub category: &'static str,
    pub title: &'static str,
    pub summary: &'static str,
    pub example_cmd: &'static str,
}

const HELP_ARTICLES: [HelpArticle; 8] = [
    HelpArticle {
        id: "study-guides",
        category: "Getting Started > Study Guides",
        title: "Study Guides Overview",
        summary: "Interactive Maple e-books for Precalculus, Single-Variable Calculus, Multivariate Calculus, and Linear Algebra. Open Precalculus Ch. 1 for Solving Inequalities with click-to-run worksheet prompts.",
        example_cmd: "solve(abs(2*x - 3) < 5, x);",
    },
    HelpArticle {
        id: "deplot",
        category: "Differential Equations > DEtools",
        title: "DEtools[DEplot][interactive]",
        summary: "Interactive planar ODE phase-portrait tutor plotting direction-field arrows and RK4 trajectories for oscillators, predator-prey, and nonlinear systems.",
        example_cmd: "dsolve(diff(y, x$2) + y = 0, y);",
    },
    HelpArticle {
        id: "dsolve",
        category: "Differential Equations",
        title: "dsolve — Symbolic ODE Solver",
        summary: "Solves first-order and second-order linear homogeneous ordinary differential equations, including initial-value problems `[ode, y(0) = c]`.",
        example_cmd: "dsolve([diff(y, x) = 3*y, y(0) = 2], y);",
    },
    HelpArticle {
        id: "solve-ineq",
        category: "Mathematics > Algebra",
        title: "solve — Equations & Inequalities",
        summary: "Finds exact symbolic roots for polynomials and exact interval / ray solutions (`And`, `Or`) for linear, absolute-value, and quadratic inequalities.",
        example_cmd: "solve(x^2 - 5*x + 6 <= 0, x);",
    },
    HelpArticle {
        id: "plot3d",
        category: "Graphics > 3D Plotting",
        title: "plot3d — 3D Surface Plotting",
        summary: "Samples a two-variable scalar field z = f(x, y) over rectangular domains x = a..b, y = c..d and renders a shaded 3D wireframe with interactive yaw/pitch rotation.",
        example_cmd: "plot3d(sin(x)*cos(y), x = -Pi..Pi, y = -Pi..Pi);",
    },
    HelpArticle {
        id: "parfrac",
        category: "Mathematics > Calculus",
        title: "convert(..., parfrac, x) — Partial Fractions",
        summary: "Decomposes proper and improper rational polynomials into partial fractions over rational linear factors and quadratic remainders.",
        example_cmd: "convert(1/(x^2 - 1), parfrac, x);",
    },
    HelpArticle {
        id: "sum",
        category: "Mathematics > Discrete Math",
        title: "sum — Finite & Symbolic Summations",
        summary: "Computes exact finite integer sums as well as closed-form Faulhaber polynomials for symbolic upper bounds `1..n`.",
        example_cmd: "sum(k^2, k = 1..5);",
    },
    HelpArticle {
        id: "linalg",
        category: "Mathematics > Linear Algebra",
        title: "Eigenvalues, Eigenvectors, NullSpace, Rank",
        summary: "Exact rational Gauss-Jordan RREF, matrix rank, null-space vector bases, characteristic-polynomial eigenvalues up to 5x5, and eigenvector subspaces.",
        example_cmd: "Eigenvalues(Matrix([[4, 1, 0], [1, 4, 0], [0, 0, 7]]));",
    },
];

pub fn help_articles() -> &'static [HelpArticle] {
    &HELP_ARTICLES
}
