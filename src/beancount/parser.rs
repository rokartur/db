use anyhow::{Context, Result, bail};
use rust_decimal::Decimal;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;

const TOLERANCE: Decimal = Decimal::from_parts(5, 0, 0, false, 3);

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Num(Decimal),
    Str(String),
    Date(String),
    Amount(Decimal, String),
    Inventory(BTreeMap<String, Decimal>),
}

#[derive(Debug, Default)]
pub struct Table {
    pub columns: Vec<&'static str>,
    pub rows: Vec<Vec<Value>>,
}

#[derive(Debug, Default)]
pub struct Ledger {
    pub postings: Table,
    pub transactions: Table,
    pub accounts: Table,
    pub balances: Table,
    pub prices: Table,
}

impl Ledger {
    pub const TABLES: [&'static str; 5] = ["postings", "transactions", "accounts", "balances", "prices"];

    pub fn table(&self, name: &str) -> Option<&Table> {
        match name.to_ascii_lowercase().as_str() {
            "postings" => Some(&self.postings),
            "transactions" => Some(&self.transactions),
            "accounts" => Some(&self.accounts),
            "balances" => Some(&self.balances),
            "prices" => Some(&self.prices),
            _ => None,
        }
    }
}

type Amount = (Decimal, String);

#[derive(Debug, Clone)]
struct Posting {
    flag: Option<String>,
    account: String,
    units: Option<Amount>,
    cost: Option<Amount>,
    price: Option<Amount>,
}

#[derive(Debug, Clone)]
struct Transaction {
    date: String,
    flag: String,
    payee: Option<String>,
    narration: String,
    tags: Vec<String>,
    links: Vec<String>,
    postings: Vec<Posting>,
    file: String,
    line: usize,
}

struct Balance {
    date: String,
    account: String,
    amount: Amount,
}

struct Pad {
    date: String,
    account: String,
    source: String,
    file: String,
    line: usize,
}

#[derive(Default)]
struct Directives {
    transactions: Vec<Transaction>,
    opens: BTreeMap<String, (String, Vec<String>)>,
    closes: BTreeMap<String, String>,
    balances: Vec<Balance>,
    prices: Vec<(String, String, Amount)>,
    pads: Vec<Pad>,
    tags: Vec<String>,
}

pub fn load(path: &Path) -> Result<Ledger> {
    let mut directives = Directives::default();
    read_file(path, &mut directives)?;
    finish(directives)
}

pub fn parse_str(text: &str) -> Result<Ledger> {
    let mut directives = Directives::default();
    parse_text(text, Path::new("<memory>"), &mut directives)?;
    finish(directives)
}

fn finish(mut directives: Directives) -> Result<Ledger> {
    directives.transactions.sort_by(|a, b| a.date.cmp(&b.date));
    for transaction in &mut directives.transactions {
        complete(transaction)?;
    }
    apply_pads(&mut directives);
    Ok(build_tables(directives))
}

fn read_file(path: &Path, directives: &mut Directives) -> Result<()> {
    let text = std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    parse_text(&text, path, directives)
}

fn parse_text(text: &str, path: &Path, directives: &mut Directives) -> Result<()> {
    let file = path.display().to_string();
    let mut current: Option<Transaction> = None;
    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let at = |error: anyhow::Error| error.context(format!("{file}:{line}"));
        let content = strip_comment(raw);
        if content.trim().is_empty() {
            continue;
        }
        if raw.starts_with([' ', '\t']) {
            let Some(transaction) = &mut current else { continue };
            if let Some(posting) = parse_posting(content.trim()).map_err(at)? {
                transaction.postings.push(posting);
            }
            continue;
        }
        if let Some(done) = current.take() {
            directives.transactions.push(done);
        }
        if raw.starts_with('*') {
            continue;
        }
        let words = split_words(content).map_err(at)?;
        current = parse_directive(&words, path, &file, line, directives).map_err(at)?;
    }
    if let Some(done) = current {
        directives.transactions.push(done);
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
enum Word {
    Quoted(String),
    Bare(String),
}

impl Word {
    fn bare(&self) -> Option<&str> {
        match self {
            Word::Bare(text) => Some(text),
            Word::Quoted(_) => None,
        }
    }
}

fn strip_comment(line: &str) -> &str {
    let mut quoted = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => quoted = !quoted,
            ';' if !quoted => return &line[..i],
            _ => {}
        }
    }
    line
}

fn split_words(line: &str) -> Result<Vec<Word>> {
    let mut words = Vec::new();
    let mut rest = line.trim();
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('"') {
            let end = after.find('"').context("unclosed string")?;
            words.push(Word::Quoted(after[..end].to_string()));
            rest = after[end + 1..].trim_start();
        } else {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            words.push(Word::Bare(rest[..end].to_string()));
            rest = rest[end..].trim_start();
        }
    }
    Ok(words)
}

fn parse_directive(words: &[Word], path: &Path, file: &str, line: usize, directives: &mut Directives) -> Result<Option<Transaction>> {
    let bare = |i: usize| words.get(i).and_then(Word::bare).with_context(|| format!("missing word {}", i + 1));
    let first = bare(0)?;
    match first {
        "option" | "plugin" => return Ok(None),
        "include" => {
            let Some(Word::Quoted(name)) = words.get(1) else { bail!("include needs a quoted path") };
            let target: PathBuf = path.parent().unwrap_or(Path::new(".")).join(name);
            read_file(&target, directives)?;
            return Ok(None);
        }
        "pushtag" => {
            directives.tags.push(bare(1)?.trim_start_matches('#').to_string());
            return Ok(None);
        }
        "poptag" => {
            let tag = bare(1)?.trim_start_matches('#');
            directives.tags.retain(|t| t != tag);
            return Ok(None);
        }
        _ => {}
    }
    if !is_date(first) {
        bail!("expected a date or a directive, found `{first}`");
    }
    let date = first.to_string();
    let kind = match words.get(1) {
        Some(Word::Bare(kind)) => kind.as_str(),
        _ => bail!("expected a directive after the date"),
    };
    match kind {
        "open" => {
            let account = bare(2)?.to_string();
            let currencies = words.get(3).and_then(Word::bare).map(|c| c.split(',').map(String::from).collect()).unwrap_or_default();
            directives.opens.insert(account, (date, currencies));
        }
        "close" => {
            directives.closes.insert(bare(2)?.to_string(), date);
        }
        "balance" => {
            let account = bare(2)?.to_string();
            let amount = (number(bare(3)?)?, bare(4)?.to_string());
            directives.balances.push(Balance { date, account, amount });
        }
        "price" => {
            let currency = bare(2)?.to_string();
            directives.prices.push((date, currency, (number(bare(3)?)?, bare(4)?.to_string())));
        }
        "pad" => {
            let (account, source) = (bare(2)?.to_string(), bare(3)?.to_string());
            directives.pads.push(Pad { date, account, source, file: file.to_string(), line });
        }
        "commodity" | "note" | "event" | "document" | "query" | "custom" => {}
        flag if flag == "txn" || flag.chars().count() == 1 => {
            let mut strings = Vec::new();
            let mut tags = directives.tags.clone();
            let mut links = Vec::new();
            for word in &words[2..] {
                match word {
                    Word::Quoted(text) => strings.push(text.clone()),
                    Word::Bare(tag) if tag.starts_with('#') => tags.push(tag[1..].to_string()),
                    Word::Bare(link) if link.starts_with('^') => links.push(link[1..].to_string()),
                    Word::Bare(other) => bail!("unexpected `{other}` in transaction header"),
                }
            }
            let (payee, narration) = match strings.len() {
                0 => (None, String::new()),
                1 => (None, strings.remove(0)),
                2 => (Some(strings.remove(0)), strings.remove(0)),
                _ => bail!("a transaction has at most a payee and a narration"),
            };
            let flag = if flag == "txn" { "*".to_string() } else { flag.to_string() };
            return Ok(Some(Transaction { date, flag, payee, narration, tags, links, postings: Vec::new(), file: file.to_string(), line }));
        }
        other => bail!("unknown directive `{other}`"),
    }
    Ok(None)
}

/// `[flag] Account [number CUR] [{cost}|{{total cost}}] [@ price | @@ total price]`; metadata lines give `None`.
fn parse_posting(text: &str) -> Result<Option<Posting>> {
    let (text, cost_text) = match (text.find('{'), text.rfind('}')) {
        (Some(open), Some(close)) if open < close => (format!("{} {}", &text[..open], &text[close + 1..]), Some(&text[open..=close])),
        _ => (text.to_string(), None),
    };
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut i = 0;
    let mut flag = None;
    if words[0].chars().count() == 1 && !words[0].starts_with(char::is_alphanumeric) {
        flag = Some(words[0].to_string());
        i = 1;
    }
    let account = words.get(i).context("posting without an account")?;
    if !account.starts_with(char::is_uppercase) || !account.contains(':') {
        if account.ends_with(':') {
            return Ok(None);
        }
        bail!("`{account}` is not an account");
    }
    i += 1;
    let mut units = None;
    if let (Some(n), Some(currency)) = (words.get(i), words.get(i + 1))
        && !n.starts_with('@')
    {
        units = Some((number(n)?, currency.to_string()));
        i += 2;
    }
    let mut price = None;
    if let Some(&at) = words.get(i) {
        let amount = (number(words.get(i + 1).context("price without a number")?)?, words.get(i + 2).context("price without a currency")?.to_string());
        price = match (at, &units) {
            ("@", _) => Some(amount),
            ("@@", Some((n, _))) if !n.is_zero() => Some((amount.0 / n.abs(), amount.1)),
            ("@@", _) => bail!("a total price needs units"),
            (other, _) => bail!("unexpected `{other}` in posting"),
        };
    }
    let cost = match cost_text {
        Some(cost_text) => parse_cost(cost_text, units.as_ref())?,
        None => None,
    };
    Ok(Some(Posting { flag, account: account.to_string(), units, cost, price }))
}

/// `{10 USD}`, `{10 USD, 2024-01-01, "lot"}`, `{{100 USD}}` (total) or `{}`.
fn parse_cost(text: &str, units: Option<&Amount>) -> Result<Option<Amount>> {
    let total = text.starts_with("{{");
    let inner = text.trim_start_matches('{').trim_end_matches('}');
    let first = inner.split(',').next().unwrap_or_default();
    let words: Vec<&str> = first.split_whitespace().collect();
    let [n, currency] = words.as_slice() else { return Ok(None) };
    let mut per_unit = number(n)?;
    if total {
        let units = units.context("a total cost needs units")?;
        per_unit /= units.0.abs();
    }
    Ok(Some((per_unit, currency.to_string())))
}

fn number(text: &str) -> Result<Decimal> {
    Decimal::from_str(&text.replace(',', "")).with_context(|| format!("`{text}` is not a number"))
}

fn is_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10 && bytes[4] == b'-' && bytes[7] == b'-' && text.chars().filter(char::is_ascii_digit).count() == 8
}

/// The amount a posting contributes to the transaction's balance.
fn weight(posting: &Posting) -> Option<Amount> {
    let (n, currency) = posting.units.clone()?;
    Some(match (&posting.cost, &posting.price) {
        (Some((cost, cost_currency)), _) => (n * cost, cost_currency.clone()),
        (None, Some((price, price_currency))) => (n * price, price_currency.clone()),
        _ => (n, currency),
    })
}

/// Fills the one posting without an amount and checks the transaction balances per currency.
fn complete(transaction: &mut Transaction) -> Result<()> {
    let mut residual: BTreeMap<String, Decimal> = BTreeMap::new();
    for (n, currency) in transaction.postings.iter().filter_map(weight) {
        *residual.entry(currency).or_default() += n;
    }
    let open: Vec<usize> = (0..transaction.postings.len()).filter(|&i| transaction.postings[i].units.is_none()).collect();
    let at = format!("{}:{}", transaction.file, transaction.line);
    match open.as_slice() {
        [] => {
            for (currency, n) in &residual {
                if n.abs() > TOLERANCE {
                    bail!("{at}: transaction does not balance: {n} {currency}");
                }
            }
        }
        [index] => {
            let template = transaction.postings.remove(*index);
            let mut fills = Vec::new();
            for (currency, n) in residual.into_iter().filter(|(_, n)| !n.is_zero()) {
                fills.push(Posting { units: Some((-n, currency)), ..template.clone() });
            }
            transaction.postings.splice(*index..*index, fills);
        }
        _ => bail!("{at}: more than one posting without an amount"),
    }
    Ok(())
}

/// Each `pad` gets a padding transaction that makes the next `balance` of that account hold.
fn apply_pads(directives: &mut Directives) {
    let mut pads = std::mem::take(&mut directives.pads);
    pads.sort_by(|a, b| a.date.cmp(&b.date));
    for pad in pads {
        let targets: Vec<&Balance> = directives.balances.iter().filter(|b| b.account == pad.account && b.date > pad.date).collect();
        let mut seen = Vec::new();
        let mut postings = Vec::new();
        for target in targets {
            let currency = &target.amount.1;
            if seen.contains(currency) {
                continue;
            }
            seen.push(currency.clone());
            let held: Decimal = directives
                .transactions
                .iter()
                .filter(|t| t.date < target.date)
                .flat_map(|t| &t.postings)
                .filter(|p| p.account == pad.account)
                .filter_map(|p| p.units.as_ref().filter(|(_, c)| c == currency).map(|(n, _)| *n))
                .sum();
            let difference = target.amount.0 - held;
            if difference.is_zero() {
                continue;
            }
            let posting = |account: &str, n: Decimal| Posting { flag: None, account: account.to_string(), units: Some((n, currency.clone())), cost: None, price: None };
            postings.push(posting(&pad.account, difference));
            postings.push(posting(&pad.source, -difference));
        }
        if postings.is_empty() {
            continue;
        }
        let transaction = Transaction {
            date: pad.date.clone(),
            flag: "P".into(),
            payee: None,
            narration: format!("(Padding inserted for Balance of {})", pad.account),
            tags: Vec::new(),
            links: Vec::new(),
            postings,
            file: pad.file.clone(),
            line: pad.line,
        };
        let at = directives.transactions.partition_point(|t| t.date <= pad.date);
        directives.transactions.insert(at, transaction);
    }
}

fn text_or_null(text: Option<&String>) -> Value {
    text.map_or(Value::Null, |t| Value::Str(t.clone()))
}

fn build_tables(directives: Directives) -> Ledger {
    let mut ledger = Ledger {
        postings: Table {
            columns: vec![
                "date", "flag", "payee", "narration", "account", "number", "currency", "position", "cost_number",
                "cost_currency", "price", "tags", "links", "filename", "lineno",
            ],
            rows: Vec::new(),
        },
        transactions: Table { columns: vec!["date", "flag", "payee", "narration", "tags", "links"], rows: Vec::new() },
        accounts: Table { columns: vec!["account", "open", "close", "currencies"], rows: Vec::new() },
        balances: Table { columns: vec!["date", "account", "number", "currency"], rows: Vec::new() },
        prices: Table { columns: vec!["date", "currency", "number", "quote"], rows: Vec::new() },
    };
    for t in &directives.transactions {
        let (tags, links) = (Value::Str(t.tags.join(",")), Value::Str(t.links.join(",")));
        ledger.transactions.rows.push(vec![
            Value::Date(t.date.clone()),
            Value::Str(t.flag.clone()),
            text_or_null(t.payee.as_ref()),
            Value::Str(t.narration.clone()),
            tags.clone(),
            links.clone(),
        ]);
        for p in &t.postings {
            let (n, currency) = p.units.clone().unwrap_or_default();
            ledger.postings.rows.push(vec![
                Value::Date(t.date.clone()),
                Value::Str(p.flag.clone().unwrap_or_else(|| t.flag.clone())),
                text_or_null(t.payee.as_ref()),
                Value::Str(t.narration.clone()),
                Value::Str(p.account.clone()),
                Value::Num(n),
                Value::Str(currency.clone()),
                Value::Amount(n, currency),
                p.cost.as_ref().map_or(Value::Null, |c| Value::Num(c.0)),
                p.cost.as_ref().map_or(Value::Null, |c| Value::Str(c.1.clone())),
                p.price.as_ref().map_or(Value::Null, |c| Value::Amount(c.0, c.1.clone())),
                tags.clone(),
                links.clone(),
                Value::Str(t.file.clone()),
                Value::Num(Decimal::from(t.line)),
            ]);
        }
    }
    for (account, (open, currencies)) in &directives.opens {
        ledger.accounts.rows.push(vec![
            Value::Str(account.clone()),
            Value::Date(open.clone()),
            directives.closes.get(account).map_or(Value::Null, |d| Value::Date(d.clone())),
            Value::Str(currencies.join(",")),
        ]);
    }
    for b in &directives.balances {
        ledger.balances.rows.push(vec![Value::Date(b.date.clone()), Value::Str(b.account.clone()), Value::Num(b.amount.0), Value::Str(b.amount.1.clone())]);
    }
    for (date, currency, (n, quote)) in &directives.prices {
        ledger.prices.rows.push(vec![Value::Date(date.clone()), Value::Str(currency.clone()), Value::Num(*n), Value::Str(quote.clone())]);
    }
    ledger
}

#[cfg(test)]
mod tests {
    use super::*;

    fn amounts(ledger: &Ledger, account: &str) -> Vec<Value> {
        ledger.postings.rows.iter().filter(|r| r[4] == Value::Str(account.into())).map(|r| r[7].clone()).collect()
    }

    #[test]
    fn fills_the_posting_without_amount() {
        let ledger = parse_str(
            "2024-01-01 open Assets:Bank\n2024-01-05 * \"Shop\" \"Food\" #trip\n  Expenses:Food  12.50 EUR ; lunch\n  Assets:Bank\n",
        )
        .unwrap();
        assert_eq!(amounts(&ledger, "Assets:Bank"), [Value::Amount(Decimal::new(-1250, 2), "EUR".into())]);
        assert_eq!(ledger.transactions.rows[0][2], Value::Str("Shop".into()));
        assert_eq!(ledger.transactions.rows[0][4], Value::Str("trip".into()));
    }

    #[test]
    fn unbalanced_transaction_names_its_line() {
        let error = parse_str("\n2024-01-05 * \"x\"\n  Expenses:Food  10 EUR\n  Assets:Bank  -9 EUR\n").unwrap_err();
        assert_eq!(error.to_string(), "<memory>:2: transaction does not balance: 1 EUR");
    }

    #[test]
    fn costs_and_prices_weigh_in_their_currency() {
        let ledger = parse_str(
            "2024-02-01 * \"Buy\"\n  Assets:Broker  10 VT {100.00 USD}\n  Assets:Cash  -1000.00 USD\n\
             2024-02-02 * \"FX\"\n  Assets:Cash  -110 USD @@ 100 EUR\n  Assets:Euro  100 EUR\n",
        )
        .unwrap();
        assert_eq!(ledger.postings.rows[0][8], Value::Num(Decimal::new(10000, 2)));
        assert_eq!(ledger.postings.rows.len(), 4);
    }

    #[test]
    fn pad_inserts_the_difference_before_balance() {
        let ledger = parse_str(
            "2024-01-01 pad Assets:Bank Equity:Opening\n2024-01-02 balance Assets:Bank 100 EUR\n\
             2024-01-01 * \"x\"\n  Assets:Bank  30 EUR\n  Income:Job\n",
        )
        .unwrap();
        let bank: Decimal = amounts(&ledger, "Assets:Bank")
            .iter()
            .map(|v| match v {
                Value::Amount(n, _) => *n,
                _ => Decimal::ZERO,
            })
            .sum();
        assert_eq!(bank, Decimal::from(100));
        assert_eq!(amounts(&ledger, "Equity:Opening"), [Value::Amount(Decimal::from(-70), "EUR".into())]);
    }

    #[test]
    fn errors_point_at_the_line() {
        let error = parse_str("2024-01-01 open Assets:Bank\n2024-13 nonsense\n").unwrap_err();
        assert!(format!("{error:#}").starts_with("<memory>:2:"), "{error:#}");
    }
}
