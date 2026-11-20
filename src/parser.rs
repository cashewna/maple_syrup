use crate::ast::{Expr, RelOp, Statement, Terminator};
use crate::rational::Rational;
use anyhow::{Result, bail};

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Int(i128),
    Float(f64),
    Ident(String),
    Percent,
    DoublePercent,
    Assign,
    Arrow,
    Range,
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    Equals,
    Less,
    LessEq,
    Greater,
    GreaterEq,
    Dollar,
    Comma,
    Semicolon,
    Colon,
    LParen,
    RParen,
    LBracket,
    RBracket,
}

fn tokenize(input: &str) -> Result<Vec<Token>> {
    let bytes = input.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        let ch = bytes[i] as char;
        if ch.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if ch == '#' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if ch == '%' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'%' {
                tokens.push(Token::DoublePercent);
                i += 2;
            } else {
                tokens.push(Token::Percent);
                i += 1;
            }
            continue;
        }
        if ch.is_ascii_digit()
            || (ch == '.' && i + 1 < bytes.len() && (bytes[i + 1] as char).is_ascii_digit())
        {
            let start = i;
            let mut is_float = false;
            while i < bytes.len() && (bytes[i] as char).is_ascii_digit() {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b'.' && !(i + 1 < bytes.len() && bytes[i + 1] == b'.')
            {
                is_float = true;
                i += 1;
                while i < bytes.len() && (bytes[i] as char).is_ascii_digit() {
                    i += 1;
                }
            }
            // Match scientific notation only when followed by optional +/- and at least one digit.
            if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
                let mut j = i + 1;
                if j < bytes.len() && (bytes[j] == b'+' || bytes[j] == b'-') {
                    j += 1;
                }
                if j < bytes.len() && (bytes[j] as char).is_ascii_digit() {
                    is_float = true;
                    i = j;
                    while i < bytes.len() && (bytes[i] as char).is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            let slice = &input[start..i];
            if is_float {
                let val: f64 = slice
                    .parse()
                    .map_err(|_| anyhow::anyhow!("invalid float literal `{slice}`"))?;
                tokens.push(Token::Float(val));
            } else {
                let val: i128 = slice
                    .parse()
                    .map_err(|_| anyhow::anyhow!("invalid integer literal `{slice}`"))?;
                tokens.push(Token::Int(val));
            }
            continue;
        }
        if ch.is_ascii_alphabetic() || ch == '_' {
            let start = i;
            i += 1;
            while i < bytes.len()
                && ((bytes[i] as char).is_ascii_alphanumeric() || bytes[i] == b'_')
            {
                i += 1;
            }
            tokens.push(Token::Ident(input[start..i].to_string()));
            continue;
        }
        // Multi-character operators
        if i + 1 < bytes.len() {
            let two = &input[i..i + 2];
            match two {
                ":=" => {
                    tokens.push(Token::Assign);
                    i += 2;
                    continue;
                }
                "->" => {
                    tokens.push(Token::Arrow);
                    i += 2;
                    continue;
                }
                ".." => {
                    tokens.push(Token::Range);
                    i += 2;
                    continue;
                }
                "**" => {
                    tokens.push(Token::Caret);
                    i += 2;
                    continue;
                }
                "<=" => {
                    tokens.push(Token::LessEq);
                    i += 2;
                    continue;
                }
                ">=" => {
                    tokens.push(Token::GreaterEq);
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }

        let tok = match ch {
            '+' => Token::Plus,
            '-' => Token::Minus,
            '*' => Token::Star,
            '/' => Token::Slash,
            '^' => Token::Caret,
            '=' => Token::Equals,
            '<' => Token::Less,
            '>' => Token::Greater,
            '$' => Token::Dollar,
            ',' => Token::Comma,
            ';' => Token::Semicolon,
            ':' => Token::Colon,
            '(' => Token::LParen,
            ')' => Token::RParen,
            '[' => Token::LBracket,
            ']' => Token::RBracket,
            other => bail!("unexpected character `{other}`"),
        };
        tokens.push(tok);
        i += 1;
    }

    Ok(tokens)
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn peek_n(&self, offset: usize) -> Option<&Token> {
        self.tokens.get(self.pos + offset)
    }

    fn next(&mut self) -> Option<Token> {
        let tok = self.tokens.get(self.pos).cloned();
        if tok.is_some() {
            self.pos += 1;
        }
        tok
    }

    fn consume_terminator(&mut self) -> Terminator {
        match self.peek() {
            Some(Token::Semicolon) => {
                self.pos += 1;
                Terminator::Semicolon
            }
            Some(Token::Colon) => {
                self.pos += 1;
                Terminator::Colon
            }
            _ => Terminator::Semicolon,
        }
    }

    fn skip_empty_terminators(&mut self) {
        while matches!(self.peek(), Some(Token::Semicolon | Token::Colon)) {
            self.pos += 1;
        }
    }

    fn parse_statements(&mut self) -> Result<Vec<Statement>> {
        let mut stmts = Vec::new();
        loop {
            self.skip_empty_terminators();
            if self.peek().is_none() {
                break;
            }
            let stmt = self.parse_statement()?;
            stmts.push(stmt);
        }
        Ok(stmts)
    }

    fn parse_statement(&mut self) -> Result<Statement> {
        if let Some(Token::Ident(name)) = self.peek()
            && name == "restart"
            && matches!(self.peek_n(1), None | Some(Token::Semicolon | Token::Colon))
        {
            self.pos += 1;
            let terminator = self.consume_terminator();
            return Ok(Statement::Restart { terminator });
        }

        if let (Some(Token::Ident(name)), Some(Token::Assign)) =
            (self.peek().cloned(), self.peek_n(1))
        {
            self.pos += 2;
            let rhs = self.parse_expr()?;
            let terminator = self.consume_terminator();
            if let Expr::Call(tag, args) = &rhs
                && tag == "__arrow__"
                && args.len() == 2
                && let Expr::List(param_exprs) = &args[0]
            {
                let mut params = Vec::with_capacity(param_exprs.len());
                for p in param_exprs {
                    match p {
                        Expr::Symbol(s) => params.push(s.clone()),
                        _ => bail!("invalid function parameter `{p}`"),
                    }
                }
                return Ok(Statement::FunctionDef {
                    name,
                    params,
                    body: args[1].clone(),
                    terminator,
                });
            }
            return Ok(Statement::Assignment {
                name,
                value: rhs,
                terminator,
            });
        }

        let expr = self.parse_expr()?;
        let terminator = self.consume_terminator();
        Ok(Statement::EvalExpr { expr, terminator })
    }

    fn parse_expr(&mut self) -> Result<Expr> {
        self.parse_arrow()
    }

    fn parse_arrow(&mut self) -> Result<Expr> {
        // Check `ident -> body` or `(a, b, ...) -> body`.
        if let (Some(Token::Ident(param)), Some(Token::Arrow)) =
            (self.peek().cloned(), self.peek_n(1))
        {
            self.pos += 2;
            let body = self.parse_arrow()?;
            return Ok(Expr::Call(
                "__arrow__".to_string(),
                vec![Expr::List(vec![Expr::Symbol(param)]), body],
            ));
        }

        if matches!(self.peek(), Some(Token::LParen)) {
            let saved = self.pos;
            if let Ok(params) = self.try_parse_arrow_params() {
                let body = self.parse_arrow()?;
                let list = Expr::List(params.into_iter().map(Expr::Symbol).collect());
                return Ok(Expr::Call("__arrow__".to_string(), vec![list, body]));
            }
            self.pos = saved;
        }

        self.parse_equation()
    }

    fn try_parse_arrow_params(&mut self) -> Result<Vec<String>> {
        if !matches!(self.next(), Some(Token::LParen)) {
            bail!("expected `(`");
        }
        let mut params = Vec::new();
        if !matches!(self.peek(), Some(Token::RParen)) {
            loop {
                match self.next() {
                    Some(Token::Ident(p)) => params.push(p),
                    _ => bail!("not a parameter list"),
                }
                match self.peek() {
                    Some(Token::Comma) => {
                        self.pos += 1;
                    }
                    Some(Token::RParen) => break,
                    _ => bail!("not a parameter list"),
                }
            }
        }
        if !matches!(self.next(), Some(Token::RParen)) {
            bail!("expected `)`");
        }
        if !matches!(self.next(), Some(Token::Arrow)) {
            bail!("expected `->`");
        }
        Ok(params)
    }

    fn peek_rel_op(&self) -> Option<RelOp> {
        match self.peek() {
            Some(Token::Less) => Some(RelOp::Lt),
            Some(Token::LessEq) => Some(RelOp::Le),
            Some(Token::Greater) => Some(RelOp::Gt),
            Some(Token::GreaterEq) => Some(RelOp::Ge),
            _ => None,
        }
    }

    fn parse_equation(&mut self) -> Result<Expr> {
        let lhs = self.parse_range()?;
        if matches!(self.peek(), Some(Token::Equals)) {
            self.pos += 1;
            let rhs = self.parse_range()?;
            return Ok(Expr::Equation(Box::new(lhs), Box::new(rhs)));
        }
        if let Some(op1) = self.peek_rel_op() {
            self.pos += 1;
            let mid = self.parse_range()?;
            if let Some(op2) = self.peek_rel_op() {
                self.pos += 1;
                let rhs = self.parse_range()?;
                return Ok(Expr::Call(
                    "And".to_string(),
                    vec![
                        Expr::Inequality(op1, Box::new(lhs), Box::new(mid.clone())),
                        Expr::Inequality(op2, Box::new(mid), Box::new(rhs)),
                    ],
                ));
            }
            return Ok(Expr::Inequality(op1, Box::new(lhs), Box::new(mid)));
        }
        Ok(lhs)
    }

    fn parse_range(&mut self) -> Result<Expr> {
        let lhs = self.parse_additive()?;
        if matches!(self.peek(), Some(Token::Range)) {
            self.pos += 1;
            let rhs = self.parse_additive()?;
            Ok(Expr::Range(Box::new(lhs), Box::new(rhs)))
        } else {
            Ok(lhs)
        }
    }

    fn parse_additive(&mut self) -> Result<Expr> {
        let mut expr = self.parse_multiplicative()?;
        loop {
            match self.peek() {
                Some(Token::Plus) => {
                    self.pos += 1;
                    let rhs = self.parse_multiplicative()?;
                    expr = Expr::Add(Box::new(expr), Box::new(rhs));
                }
                Some(Token::Minus) => {
                    self.pos += 1;
                    let rhs = self.parse_multiplicative()?;
                    expr = Expr::Sub(Box::new(expr), Box::new(rhs));
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr> {
        let mut expr = self.parse_unary()?;
        loop {
            match self.peek() {
                Some(Token::Star) => {
                    self.pos += 1;
                    let rhs = self.parse_unary()?;
                    expr = Expr::Mul(Box::new(expr), Box::new(rhs));
                }
                Some(Token::Slash) => {
                    self.pos += 1;
                    let rhs = self.parse_unary()?;
                    expr = Expr::Div(Box::new(expr), Box::new(rhs));
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_unary(&mut self) -> Result<Expr> {
        match self.peek() {
            Some(Token::Plus) => {
                self.pos += 1;
                self.parse_unary()
            }
            Some(Token::Minus) => {
                self.pos += 1;
                let inner = self.parse_unary()?;
                match inner {
                    Expr::Rational(r) => Ok(Expr::Rational(-r)),
                    Expr::Float(v) => Ok(Expr::Float(-v)),
                    other => Ok(Expr::UnaryNeg(Box::new(other))),
                }
            }
            _ => self.parse_power(),
        }
    }

    fn parse_power(&mut self) -> Result<Expr> {
        let base = self.parse_dollar()?;
        if matches!(self.peek(), Some(Token::Caret)) {
            self.pos += 1;
            let exp = self.parse_unary()?;
            Ok(Expr::Pow(Box::new(base), Box::new(exp)))
        } else {
            Ok(base)
        }
    }

    fn parse_dollar(&mut self) -> Result<Expr> {
        let lhs = self.parse_postfix()?;
        if matches!(self.peek(), Some(Token::Dollar)) {
            self.pos += 1;
            let rhs = self.parse_postfix()?;
            Ok(Expr::Call("$".to_string(), vec![lhs, rhs]))
        } else {
            Ok(lhs)
        }
    }

    fn flatten_indexed_callee(expr: &Expr) -> Option<String> {
        match expr {
            Expr::Symbol(s) => Some(s.clone()),
            Expr::Call(name, args) if name == "__index__" && args.len() >= 2 => {
                let mut head = Self::flatten_indexed_callee(&args[0])?;
                let mut idxs = Vec::with_capacity(args.len() - 1);
                for a in &args[1..] {
                    match a {
                        Expr::Symbol(s) => idxs.push(s.clone()),
                        _ => return None,
                    }
                }
                head.push('[');
                head.push_str(&idxs.join(", "));
                head.push(']');
                Some(head)
            }
            _ => None,
        }
    }

    fn parse_postfix(&mut self) -> Result<Expr> {
        let mut expr = self.parse_atom()?;
        loop {
            match self.peek() {
                Some(Token::LParen) => {
                    let name = Self::flatten_indexed_callee(&expr).ok_or_else(|| {
                        anyhow::anyhow!("cannot call non-symbol expression `{expr}`")
                    })?;
                    self.pos += 1;
                    let args = self.parse_comma_list(Token::RParen)?;
                    expr = Expr::Call(name, args);
                }
                Some(Token::LBracket) => {
                    self.pos += 1;
                    let mut indices = vec![expr];
                    indices.extend(self.parse_comma_list(Token::RBracket)?);
                    expr = Expr::Call("__index__".to_string(), indices);
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_comma_list(&mut self, closing: Token) -> Result<Vec<Expr>> {
        let mut items = Vec::new();
        if self.peek() == Some(&closing) {
            self.pos += 1;
            return Ok(items);
        }
        loop {
            items.push(self.parse_expr()?);
            if matches!(self.peek(), Some(Token::Comma)) {
                self.pos += 1;
            } else if self.peek() == Some(&closing) {
                self.pos += 1;
                break;
            } else {
                bail!("expected `,` or `{closing:?}`, found {:?}", self.peek());
            }
        }
        Ok(items)
    }

    fn parse_atom(&mut self) -> Result<Expr> {
        match self.next() {
            Some(Token::Int(n)) => Ok(Expr::Rational(Rational::from_int(n))),
            Some(Token::Float(v)) => Ok(Expr::Float(v)),
            Some(Token::Ident(name)) => Ok(Expr::Symbol(name)),
            Some(Token::Percent) => Ok(Expr::Symbol("%".to_string())),
            Some(Token::DoublePercent) => Ok(Expr::Symbol("%%".to_string())),
            Some(Token::LParen) => {
                let inner = self.parse_expr()?;
                if !matches!(self.next(), Some(Token::RParen)) {
                    bail!("expected closing `)`");
                }
                Ok(inner)
            }
            Some(Token::LBracket) => {
                let items = self.parse_comma_list(Token::RBracket)?;
                Ok(Expr::List(items))
            }
            other => bail!("unexpected token `{other:?}` while parsing expression"),
        }
    }
}

/// Parse one or more semicolon/colon-separated Maple statements.
pub fn parse_statements(input: &str) -> Result<Vec<Statement>> {
    let tokens = tokenize(input)?;
    let mut parser = Parser::new(tokens);
    parser.parse_statements()
}

/// Parse a single Maple expression, failing if trailing tokens remain.
pub fn parse_expr(input: &str) -> Result<Expr> {
    let tokens = tokenize(input)?;
    let mut parser = Parser::new(tokens);
    let expr = parser.parse_expr()?;
    if let Some(tok) = parser.peek() {
        bail!("unexpected trailing token `{tok:?}`");
    }
    Ok(expr)
}
