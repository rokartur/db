use super::parser::{Ledger, Table, Value};
use crate::driver::Grid;
use anyhow::{Result, anyhow, bail};
use regex::Regex;
use rust_decimal::Decimal;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::str::FromStr;

pub fn run(ledger: &Ledger, text: &str) -> Result<Grid> {
    match Parser::new(text)?.statement()? {
        Statement::Select(select) => select.execute(ledger),
        Statement::Journal(pattern) => journal(ledger, &pattern),
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Num(Decimal),
    Str(String),
    Date(String),
    Ident(String),
    Symbol(&'static str),
    End,
}

#[derive(Debug, Clone)]
struct Spanned {
    token: Token,
    start: usize,
    end: usize,
}

const SYMBOLS: [&str; 14] = ["!=", "<=", ">=", "=", "<", ">", "~", "+", "-", "*", "/", "(", ")", ","];

fn tokenize(text: &str) -> Result<Vec<Spanned>> {
    let mut tokens = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < text.len() {
        let c = text[i..].chars().next().unwrap_or(' ');
        let start = i;
        if c.is_whitespace() {
            i += c.len_utf8();
            continue;
        }
        let token = if c.is_ascii_digit() {
            let date_like = text.get(i..i + 10).is_some_and(|s| s.as_bytes()[4] == b'-' && s.as_bytes()[7] == b'-');
            if date_like {
                i += 10;
                Token::Date(text[start..i].to_string())
            } else {
                while i < text.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                    i += 1;
                }
                Token::Num(Decimal::from_str(&text[start..i]).map_err(|_| anyhow!("syntax error at {start}: bad number"))?)
            }
        } else if c == '\'' || c == '"' {
            let end = text[i + 1..].find(c).ok_or_else(|| anyhow!("syntax error at {start}: unclosed string"))?;
            i += end + 2;
            Token::Str(text[start + 1..i - 1].to_string())
        } else if c.is_alphabetic() || c == '_' {
            while i < text.len() && (text[i..].starts_with(|c: char| c.is_alphanumeric() || c == '_' || c == '.')) {
                i += text[i..].chars().next().map_or(1, char::len_utf8);
            }
            Token::Ident(text[start..i].to_string())
        } else {
            let symbol = SYMBOLS.iter().find(|s| text[i..].starts_with(**s)).ok_or_else(|| anyhow!("syntax error at {start}: unexpected `{c}`"))?;
            i += symbol.len();
            Token::Symbol(symbol)
        };
        tokens.push(Spanned { token, start, end: i });
    }
    tokens.push(Spanned { token: Token::End, start: text.len(), end: text.len() });
    Ok(tokens)
}

#[derive(Debug, Clone)]
enum Expr {
    Lit(Value),
    Col(String),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Binary(&'static str, Box<Expr>, Box<Expr>),
    Match(Box<Expr>, Regex),
    In(Box<Expr>, Vec<Expr>),
    Call(String, Vec<Expr>),
    /// `GROUP BY 1` / `ORDER BY 2`: a 1-based target position.
    Position(usize),
}

struct Target {
    expr: Expr,
    name: String,
}

struct Select {
    distinct: bool,
    targets: Option<Vec<Target>>,
    from: String,
    filter: Option<Expr>,
    group_by: Vec<Expr>,
    order_by: Vec<(Expr, String, bool)>,
    limit: Option<usize>,
}

enum Statement {
    Select(Select),
    Journal(Regex),
}

struct Parser<'a> {
    text: &'a str,
    tokens: Vec<Spanned>,
    i: usize,
}

impl<'a> Parser<'a> {
    fn new(text: &'a str) -> Result<Self> {
        Ok(Parser { text, tokens: tokenize(text)?, i: 0 })
    }

    /// Past the end every read sees the final `End` token, so `next` followed by `i -= 1` always rewinds.
    fn current(&self) -> &Spanned {
        &self.tokens[self.i.min(self.tokens.len() - 1)]
    }

    fn peek(&self) -> &Token {
        &self.current().token
    }

    fn next(&mut self) -> Token {
        let token = self.peek().clone();
        self.i += 1;
        token
    }

    fn error(&self, expected: &str) -> anyhow::Error {
        let at = self.current();
        let found = if at.token == Token::End { "end of query".to_string() } else { format!("`{}`", &self.text[at.start..at.end]) };
        anyhow!("syntax error at {}: expected {expected}, found {found}", at.start)
    }

    fn keyword(&mut self, word: &str) -> bool {
        let matched = matches!(self.peek(), Token::Ident(ident) if ident.eq_ignore_ascii_case(word));
        if matched {
            self.i += 1;
        }
        matched
    }

    fn expect_keyword(&mut self, word: &str) -> Result<()> {
        if self.keyword(word) { Ok(()) } else { Err(self.error(word)) }
    }

    fn symbol(&mut self, symbol: &str) -> bool {
        let matched = matches!(self.peek(), Token::Symbol(s) if *s == symbol);
        if matched {
            self.i += 1;
        }
        matched
    }

    fn expect_symbol(&mut self, symbol: &str) -> Result<()> {
        if self.symbol(symbol) { Ok(()) } else { Err(self.error(&format!("`{symbol}`"))) }
    }

    fn statement(&mut self) -> Result<Statement> {
        let statement = if self.keyword("select") {
            Statement::Select(self.select()?)
        } else if self.keyword("balances") {
            let filter = if self.keyword("where") { Some(self.expr(0)?) } else { None };
            let account = || Expr::Col("account".into());
            Statement::Select(Select {
                distinct: false,
                targets: Some(vec![
                    Target { expr: account(), name: "account".into() },
                    Target { expr: Expr::Call("sum".into(), vec![Expr::Col("position".into())]), name: "balance".into() },
                ]),
                from: "postings".into(),
                filter,
                group_by: vec![account()],
                order_by: vec![(account(), "account".into(), false)],
                limit: None,
            })
        } else if self.keyword("journal") {
            let Token::Str(pattern) = self.next() else {
                self.i -= 1;
                return Err(self.error("an account regex in quotes"));
            };
            Statement::Journal(Regex::new(&format!("(?i){pattern}"))?)
        } else {
            return Err(self.error("SELECT, BALANCES or JOURNAL"));
        };
        if *self.peek() != Token::End {
            return Err(self.error("end of query"));
        }
        Ok(statement)
    }

    fn select(&mut self) -> Result<Select> {
        let distinct = self.keyword("distinct");
        let targets = if self.symbol("*") { None } else { Some(self.targets()?) };
        let from = if self.keyword("from") {
            match self.next() {
                Token::Ident(name) => name,
                _ => {
                    self.i -= 1;
                    return Err(self.error("a table name"));
                }
            }
        } else {
            "postings".into()
        };
        let filter = if self.keyword("where") { Some(self.expr(0)?) } else { None };
        let mut group_by = Vec::new();
        if self.keyword("group") {
            self.expect_keyword("by")?;
            loop {
                group_by.push(self.expr(0)?);
                if !self.symbol(",") {
                    break;
                }
            }
        }
        let mut order_by = Vec::new();
        if self.keyword("order") {
            self.expect_keyword("by")?;
            loop {
                let (expr, source) = self.sourced_expr()?;
                let descending = self.keyword("desc");
                if !descending {
                    self.keyword("asc");
                }
                order_by.push((expr, source, descending));
                if !self.symbol(",") {
                    break;
                }
            }
        }
        let limit = if self.keyword("limit") {
            match self.next() {
                Token::Num(n) => Some(n.try_into().map_err(|_| self.error("a row count"))?),
                _ => {
                    self.i -= 1;
                    return Err(self.error("a row count"));
                }
            }
        } else {
            None
        };
        Ok(Select { distinct, targets, from, filter, group_by: group_by.into_iter().map(positional).collect(), order_by, limit })
    }

    fn targets(&mut self) -> Result<Vec<Target>> {
        let mut targets = Vec::new();
        loop {
            let (expr, source) = self.sourced_expr()?;
            let name = if self.keyword("as") {
                match self.next() {
                    Token::Ident(name) => name,
                    _ => {
                        self.i -= 1;
                        return Err(self.error("a column name"));
                    }
                }
            } else {
                source
            };
            targets.push(Target { expr, name });
            if !self.symbol(",") {
                return Ok(targets);
            }
        }
    }

    fn sourced_expr(&mut self) -> Result<(Expr, String)> {
        let start = self.current().start;
        let expr = self.expr(0)?;
        let end = self.tokens[self.i - 1].end;
        Ok((positional(expr), self.text[start..end].to_string()))
    }

    /// Pratt parser; binding powers: OR 1, AND 2, NOT 3, comparisons 4, + - 5, * / 6.
    fn expr(&mut self, min_power: u8) -> Result<Expr> {
        let mut left = self.prefix()?;
        loop {
            let (operator, power) = match self.peek() {
                Token::Ident(word) if word.eq_ignore_ascii_case("or") => ("or", 1),
                Token::Ident(word) if word.eq_ignore_ascii_case("and") => ("and", 2),
                Token::Ident(word) if word.eq_ignore_ascii_case("in") => ("in", 4),
                Token::Symbol(s @ ("=" | "!=" | "<" | "<=" | ">" | ">=" | "~")) => (*s, 4),
                Token::Symbol(s @ ("+" | "-")) => (*s, 5),
                Token::Symbol(s @ ("*" | "/")) => (*s, 6),
                _ => return Ok(left),
            };
            if power <= min_power {
                return Ok(left);
            }
            self.i += 1;
            left = match operator {
                "~" => match self.next() {
                    Token::Str(pattern) => Expr::Match(Box::new(left), Regex::new(&format!("(?i){pattern}"))?),
                    _ => {
                        self.i -= 1;
                        return Err(self.error("a regex in quotes after ~"));
                    }
                },
                "in" => {
                    self.expect_symbol("(")?;
                    let mut items = vec![self.expr(0)?];
                    while self.symbol(",") {
                        items.push(self.expr(0)?);
                    }
                    self.expect_symbol(")")?;
                    Expr::In(Box::new(left), items)
                }
                _ => Expr::Binary(operator, Box::new(left), Box::new(self.expr(power)?)),
            };
        }
    }

    fn prefix(&mut self) -> Result<Expr> {
        match self.next() {
            Token::Num(n) => Ok(Expr::Lit(Value::Num(n))),
            Token::Str(s) => Ok(Expr::Lit(Value::Str(s))),
            Token::Date(d) => Ok(Expr::Lit(Value::Date(d))),
            Token::Symbol("-") => Ok(Expr::Neg(Box::new(self.expr(6)?))),
            Token::Symbol("(") => {
                let inner = self.expr(0)?;
                self.expect_symbol(")")?;
                Ok(inner)
            }
            Token::Ident(word) => match word.to_ascii_lowercase().as_str() {
                "not" => Ok(Expr::Not(Box::new(self.expr(3)?))),
                "true" => Ok(Expr::Lit(Value::Bool(true))),
                "false" => Ok(Expr::Lit(Value::Bool(false))),
                "null" => Ok(Expr::Lit(Value::Null)),
                _ if self.symbol("(") => self.call(word.to_ascii_lowercase()),
                _ => Ok(Expr::Col(word.to_ascii_lowercase())),
            },
            _ => {
                self.i -= 1;
                Err(self.error("an expression"))
            }
        }
    }

    fn call(&mut self, name: String) -> Result<Expr> {
        let mut args = Vec::new();
        if name == "count" && self.symbol("*") {
            args.push(Expr::Lit(Value::Bool(true)));
        } else if *self.peek() != Token::Symbol(")") {
            args.push(self.expr(0)?);
            while self.symbol(",") {
                args.push(self.expr(0)?);
            }
        }
        self.expect_symbol(")")?;
        Ok(Expr::Call(name, args))
    }
}

/// A bare integer in GROUP BY / ORDER BY names a target position.
fn positional(expr: Expr) -> Expr {
    match expr {
        Expr::Lit(Value::Num(n)) if n.is_integer() => match usize::try_from(n) {
            Ok(position) if position > 0 => Expr::Position(position),
            _ => Expr::Lit(Value::Num(n)),
        },
        other => other,
    }
}

const AGGREGATES: [&str; 6] = ["sum", "count", "first", "last", "min", "max"];

fn has_aggregate(expr: &Expr) -> bool {
    match expr {
        Expr::Call(name, args) => AGGREGATES.contains(&name.as_str()) || args.iter().any(has_aggregate),
        Expr::Neg(inner) | Expr::Not(inner) | Expr::Match(inner, _) => has_aggregate(inner),
        Expr::Binary(_, a, b) => has_aggregate(a) || has_aggregate(b),
        Expr::In(a, items) => has_aggregate(a) || items.iter().any(has_aggregate),
        Expr::Lit(_) | Expr::Col(_) | Expr::Position(_) => false,
    }
}

fn check_columns(expr: &Expr, table: &Table) -> Result<()> {
    match expr {
        Expr::Col(name) if !table.columns.contains(&name.as_str()) => {
            bail!("unknown column `{name}`; columns: {}", table.columns.join(", "))
        }
        Expr::Call(_, args) => args.iter().try_for_each(|a| check_columns(a, table)),
        Expr::Neg(inner) | Expr::Not(inner) | Expr::Match(inner, _) => check_columns(inner, table),
        Expr::Binary(_, a, b) => check_columns(a, table).and(check_columns(b, table)),
        Expr::In(a, items) => check_columns(a, table).and(items.iter().try_for_each(|i| check_columns(i, table))),
        _ => Ok(()),
    }
}

/// Rows an expression sees: one row, or every row of a group (for aggregates).
#[derive(Clone, Copy)]
enum Scope<'a> {
    Row(&'a [Value]),
    Group(&'a [&'a Vec<Value>]),
}

struct Output<'a> {
    values: Vec<Value>,
    rows: Vec<&'a Vec<Value>>,
}

impl Select {
    fn execute(self, ledger: &Ledger) -> Result<Grid> {
        let table = ledger.table(&self.from).ok_or_else(|| anyhow!("unknown table `{}`; tables: {}", self.from, Ledger::TABLES.join(", ")))?;
        let targets = self.targets.unwrap_or_else(|| {
            table.columns.iter().map(|c| Target { expr: Expr::Col(c.to_string()), name: c.to_string() }).collect()
        });
        let resolve = |expr: &Expr| -> Result<Expr> {
            match expr {
                Expr::Position(n) => targets.get(n - 1).map(|t| t.expr.clone()).ok_or_else(|| anyhow!("no target at position {n}")),
                other => Ok(other.clone()),
            }
        };
        let group_by: Vec<Expr> = self.group_by.iter().map(resolve).collect::<Result<_>>()?;
        for expr in targets.iter().map(|t| &t.expr).chain(&self.filter).chain(&group_by) {
            check_columns(expr, table)?;
        }

        let mut rows = Vec::new();
        for row in &table.rows {
            let keep = match &self.filter {
                Some(filter) => eval(filter, &table.columns, Scope::Row(row))? == Value::Bool(true),
                None => true,
            };
            if keep {
                rows.push(row);
            }
        }

        let grouped = !group_by.is_empty() || targets.iter().any(|t| has_aggregate(&t.expr));
        let mut outputs = Vec::new();
        if grouped {
            for group in group_rows(&group_by, &table.columns, rows)? {
                let values = targets.iter().map(|t| eval(&t.expr, &table.columns, Scope::Group(&group))).collect::<Result<_>>()?;
                outputs.push(Output { values, rows: group });
            }
        } else {
            for row in rows {
                let values = targets.iter().map(|t| eval(&t.expr, &table.columns, Scope::Row(row))).collect::<Result<_>>()?;
                outputs.push(Output { values, rows: vec![row] });
            }
        }

        if self.distinct {
            let mut seen = HashSet::new();
            outputs.retain(|o| seen.insert(format!("{:?}", o.values)));
        }

        if !self.order_by.is_empty() {
            let mut keyed = Vec::new();
            for output in outputs {
                let mut key = Vec::new();
                for (expr, source, _) in &self.order_by {
                    let target = match expr {
                        Expr::Position(n) => Some(n - 1),
                        _ => targets.iter().position(|t| t.name.eq_ignore_ascii_case(source)),
                    };
                    key.push(match target {
                        Some(index) => output.values.get(index).cloned().ok_or_else(|| anyhow!("no target at position {}", index + 1))?,
                        None => {
                            check_columns(expr, table)?;
                            let scope = if grouped { Scope::Group(&output.rows) } else { Scope::Row(output.rows[0]) };
                            eval(expr, &table.columns, scope)?
                        }
                    });
                }
                keyed.push((key, output));
            }
            keyed.sort_by(|(a, _), (b, _)| {
                for (i, (_, _, descending)) in self.order_by.iter().enumerate() {
                    let order = compare(&a[i], &b[i]);
                    let order = if *descending { order.reverse() } else { order };
                    if order != Ordering::Equal {
                        return order;
                    }
                }
                Ordering::Equal
            });
            outputs = keyed.into_iter().map(|(_, output)| output).collect();
        }

        if let Some(limit) = self.limit {
            outputs.truncate(limit);
        }
        Ok(Grid {
            columns: targets.into_iter().map(|t| t.name).collect(),
            rows: outputs.iter().map(|o| o.values.iter().map(display).collect()).collect(),
        })
    }
}

/// Groups in first-seen order; no GROUP BY means one group with every row.
fn group_rows<'a>(group_by: &[Expr], columns: &[&str], rows: Vec<&'a Vec<Value>>) -> Result<Vec<Vec<&'a Vec<Value>>>> {
    if group_by.is_empty() {
        return Ok(vec![rows]);
    }
    let mut groups: Vec<Vec<&Vec<Value>>> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for row in rows {
        let key: Vec<Value> = group_by.iter().map(|e| eval(e, columns, Scope::Row(row))).collect::<Result<_>>()?;
        let slot = *index.entry(format!("{key:?}")).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[slot].push(row);
    }
    Ok(groups)
}

fn eval(expr: &Expr, columns: &[&str], scope: Scope) -> Result<Value> {
    let sub = |e: &Expr| eval(e, columns, scope);
    Ok(match expr {
        Expr::Lit(value) => value.clone(),
        Expr::Col(name) => {
            let index = columns.iter().position(|c| c == name).ok_or_else(|| anyhow!("unknown column `{name}`"))?;
            match scope {
                Scope::Row(row) => row[index].clone(),
                Scope::Group(rows) => rows.first().map_or(Value::Null, |row| row[index].clone()),
            }
        }
        Expr::Neg(inner) => match sub(inner)? {
            Value::Num(n) => Value::Num(-n),
            Value::Amount(n, c) => Value::Amount(-n, c),
            Value::Null => Value::Null,
            other => bail!("cannot negate {}", display(&other)),
        },
        Expr::Not(inner) => Value::Bool(sub(inner)? != Value::Bool(true)),
        Expr::Match(inner, regex) => match sub(inner)? {
            Value::Null => Value::Bool(false),
            value => Value::Bool(regex.is_match(&display(&value))),
        },
        Expr::In(inner, items) => {
            let value = sub(inner)?;
            let mut found = false;
            for item in items {
                found |= compare(&value, &sub(item)?) == Ordering::Equal;
            }
            Value::Bool(found)
        }
        Expr::Binary(operator, a, b) => binary(operator, sub(a)?, sub(b)?)?,
        Expr::Call(name, args) if AGGREGATES.contains(&name.as_str()) => {
            let Scope::Group(rows) = scope else { bail!("{name}() needs GROUP BY or a whole-table query") };
            let arg = args.first().ok_or_else(|| anyhow!("{name}() takes one argument"))?;
            let mut values = Vec::new();
            for row in rows {
                values.push(eval(arg, columns, Scope::Row(row))?);
            }
            aggregate(name, values)?
        }
        Expr::Call(name, args) => {
            let values: Vec<Value> = args.iter().map(sub).collect::<Result<_>>()?;
            function(name, &values)?
        }
        Expr::Position(n) => bail!("position {n} is only valid in GROUP BY and ORDER BY"),
    })
}

fn binary(operator: &str, a: Value, b: Value) -> Result<Value> {
    use Value::*;
    let truthy = |v: &Value| *v == Bool(true);
    Ok(match operator {
        "and" => Bool(truthy(&a) && truthy(&b)),
        "or" => Bool(truthy(&a) || truthy(&b)),
        _ if a == Null || b == Null => {
            if ["=", "!=", "<", "<=", ">", ">="].contains(&operator) { Bool(false) } else { Null }
        }
        "=" => Bool(compare(&a, &b) == Ordering::Equal),
        "!=" => Bool(compare(&a, &b) != Ordering::Equal),
        "<" => Bool(compare(&a, &b) == Ordering::Less),
        "<=" => Bool(compare(&a, &b) != Ordering::Greater),
        ">" => Bool(compare(&a, &b) == Ordering::Greater),
        ">=" => Bool(compare(&a, &b) != Ordering::Less),
        _ => match (operator, a, b) {
            ("+", Num(x), Num(y)) => Num(x + y),
            ("-", Num(x), Num(y)) => Num(x - y),
            ("*", Num(x), Num(y)) => Num(x * y),
            ("/", Num(_), Num(y)) if y.is_zero() => Null,
            ("/", Num(x), Num(y)) => Num(x / y),
            ("+", Amount(x, c), Amount(y, d)) if c == d => Amount(x + y, c),
            ("-", Amount(x, c), Amount(y, d)) if c == d => Amount(x - y, c),
            ("*", Amount(x, c), Num(y)) | ("*", Num(y), Amount(x, c)) => Amount(x * y, c),
            ("/", Amount(x, c), Num(y)) if !y.is_zero() => Amount(x / y, c),
            (operator, a, b) => bail!("cannot compute {} {operator} {}", display(&a), display(&b)),
        },
    })
}

fn function(name: &str, args: &[Value]) -> Result<Value> {
    use Value::*;
    let arg = |i: usize| args.get(i).ok_or_else(|| anyhow!("{name}() is missing argument {}", i + 1));
    if args.first() == Some(&Null) {
        return Ok(Null);
    }
    let text = |v: &Value| match v {
        Str(s) | Date(s) => Ok(s.clone()),
        other => Err(anyhow!("{name}() expects text, got {}", display(other))),
    };
    let date_part = |range: std::ops::Range<usize>| -> Result<Value> {
        let date = text(arg(0)?)?;
        let part = date.get(range).ok_or_else(|| anyhow!("{name}() expects a date"))?;
        Ok(Num(Decimal::from_str(part)?))
    };
    Ok(match name {
        "year" => date_part(0..4)?,
        "month" => date_part(5..7)?,
        "day" => date_part(8..10)?,
        "lower" => Str(text(arg(0)?)?.to_lowercase()),
        "upper" => Str(text(arg(0)?)?.to_uppercase()),
        "length" => Num(Decimal::from(text(arg(0)?)?.chars().count())),
        "parent" => {
            let account = text(arg(0)?)?;
            account.rsplit_once(':').map_or(Null, |(parent, _)| Str(parent.to_string()))
        }
        "leaf" => Str(text(arg(0)?)?.rsplit(':').next().unwrap_or_default().to_string()),
        "root" => {
            let depth = match args.get(1) {
                Some(Num(n)) => usize::try_from(*n).map_err(|_| anyhow!("root() depth must be a positive integer"))?,
                None => 1,
                Some(other) => bail!("root() depth must be a number, got {}", display(other)),
            };
            Str(text(arg(0)?)?.split(':').take(depth).collect::<Vec<_>>().join(":"))
        }
        "units" => arg(0)?.clone(),
        "number" => match arg(0)? {
            Amount(n, _) | Num(n) => Num(*n),
            other => bail!("number() expects an amount, got {}", display(other)),
        },
        "currency" => match arg(0)? {
            Amount(_, c) => Str(c.clone()),
            other => bail!("currency() expects an amount, got {}", display(other)),
        },
        "abs" => match arg(0)? {
            Num(n) => Num(n.abs()),
            Amount(n, c) => Amount(n.abs(), c.clone()),
            other => bail!("abs() expects a number, got {}", display(other)),
        },
        "str" => Str(display(arg(0)?)),
        other => bail!("unknown function {other}()"),
    })
}

fn aggregate(name: &str, values: Vec<Value>) -> Result<Value> {
    let present = values.into_iter().filter(|v| *v != Value::Null);
    Ok(match name {
        "count" => Value::Num(Decimal::from(present.count())),
        "first" => present.into_iter().next().unwrap_or(Value::Null),
        "last" => present.into_iter().next_back().unwrap_or(Value::Null),
        "min" => present.min_by(compare).unwrap_or(Value::Null),
        "max" => present.max_by(compare).unwrap_or(Value::Null),
        _ => {
            let mut total = Value::Null;
            for value in present {
                total = add(total, value)?;
            }
            total
        }
    })
}

/// `sum` on numbers gives a number; on amounts it builds a multi-currency inventory.
fn add(total: Value, value: Value) -> Result<Value> {
    use Value::*;
    Ok(match (total, value) {
        (Null, Num(n)) => Num(n),
        (Num(a), Num(b)) => Num(a + b),
        (Null, Amount(n, c)) => Inventory(BTreeMap::from([(c, n)])),
        (Inventory(mut held), Amount(n, c)) => {
            *held.entry(c).or_default() += n;
            Inventory(held)
        }
        (Null, Inventory(other)) => Inventory(other),
        (Inventory(mut held), Inventory(other)) => {
            for (c, n) in other {
                *held.entry(c).or_default() += n;
            }
            Inventory(held)
        }
        (total, value) => bail!("cannot sum {} and {}", display(&total), display(&value)),
    })
}

fn compare(a: &Value, b: &Value) -> Ordering {
    use Value::*;
    match (a, b) {
        (Null, Null) => Ordering::Equal,
        (Null, _) => Ordering::Less,
        (_, Null) => Ordering::Greater,
        (Num(x), Num(y)) => x.cmp(y),
        (Bool(x), Bool(y)) => x.cmp(y),
        (Str(x) | Date(x), Str(y) | Date(y)) => x.cmp(y),
        (Amount(x, c), Amount(y, d)) => c.cmp(d).then(x.cmp(y)),
        _ => display(a).cmp(&display(b)),
    }
}

pub fn display(value: &Value) -> String {
    match value {
        Value::Null => "NULL".into(),
        Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
        Value::Num(n) => n.to_string(),
        Value::Str(s) | Value::Date(s) => s.clone(),
        Value::Amount(n, c) => format!("{n} {c}"),
        Value::Inventory(held) => {
            let parts: Vec<String> = held.iter().filter(|(_, n)| !n.is_zero()).map(|(c, n)| format!("{n} {c}")).collect();
            parts.join(", ")
        }
    }
}

/// Postings of matching accounts with a running balance.
fn journal(ledger: &Ledger, pattern: &Regex) -> Result<Grid> {
    let table = &ledger.postings;
    let column = |name: &str| table.columns.iter().position(|c| *c == name).ok_or_else(|| anyhow!("missing column {name}"));
    let shown = ["date", "flag", "payee", "narration", "account", "position"].map(column);
    let shown: Vec<usize> = shown.into_iter().collect::<Result<_>>()?;
    let (account, position) = (column("account")?, column("position")?);
    let mut balance = Value::Null;
    let mut rows = Vec::new();
    for row in &table.rows {
        if !pattern.is_match(&display(&row[account])) {
            continue;
        }
        balance = add(balance, row[position].clone())?;
        let mut cells: Vec<String> = shown.iter().map(|&i| display(&row[i])).collect();
        cells.push(display(&balance));
        rows.push(cells);
    }
    let mut columns: Vec<String> = ["date", "flag", "payee", "narration", "account", "position"].map(String::from).to_vec();
    columns.push("balance".into());
    Ok(Grid { columns, rows })
}
