pub mod bql;
pub mod parser;

use crate::driver::{Driver, Grid, Group, PAGE};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use parser::Ledger;
use std::path::Path;

/// A Beancount ledger file as five read-only tables queried with BQL.
pub struct Beancount {
    ledger: Ledger,
}

impl Beancount {
    pub fn open(path: &str) -> Result<Self> {
        Ok(Beancount { ledger: parser::load(Path::new(path))? })
    }
}

#[async_trait]
impl Driver for Beancount {
    async fn query(&self, _group: &str, text: &str) -> Result<Grid> {
        bql::run(&self.ledger, text)
    }

    async fn groups(&self) -> Result<Vec<Group>> {
        Ok(vec![Group { name: "ledger".into(), items: Ledger::TABLES.map(String::from).to_vec() }])
    }

    async fn browse(&self, _group: &str, item: &str, page: usize) -> Result<Grid> {
        let table = self.ledger.table(item).ok_or_else(|| anyhow!("unknown table `{item}`"))?;
        let rows = table.rows.iter().skip(page * PAGE).take(PAGE);
        Ok(Grid {
            columns: table.columns.iter().map(|c| c.to_string()).collect(),
            rows: rows.map(|row| row.iter().map(bql::display).collect()).collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ledger() -> Ledger {
        parser::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ledger.beancount")).unwrap()
    }

    fn rows(text: &str) -> Vec<Vec<String>> {
        bql::run(&ledger(), text).unwrap().rows
    }

    #[test]
    fn balances_per_account() {
        assert_eq!(
            rows("BALANCES"),
            [
                ["Assets:Bank", "2224.50 EUR"],
                ["Assets:Cash", "-20 USD"],
                ["Equity:Opening", "-1000.00 EUR"],
                ["Expenses:Food", "75.50 EUR, 20 USD"],
                ["Expenses:Rent", "1200.00 EUR"],
                ["Income:Salary", "-2500.00 EUR"],
            ]
        );
        assert_eq!(rows("BALANCES WHERE account ~ '^assets'").len(), 2);
    }

    #[test]
    fn yearly_expenses() {
        let grid = bql::run(&ledger(), "SELECT year(date), sum(position) WHERE account ~ 'expenses' GROUP BY 1 ORDER BY 1").unwrap();
        assert_eq!(grid.columns, ["year(date)", "sum(position)"]);
        assert_eq!(grid.rows, [["2024", "1245.50 EUR, 20 USD"], ["2025", "30.00 EUR"]]);
    }

    #[test]
    fn journal_keeps_a_running_balance() {
        let balances: Vec<String> = rows("JOURNAL 'Assets:Bank'").into_iter().map(|r| r[6].clone()).collect();
        assert_eq!(balances, ["1000.00 EUR", "3500.00 EUR", "3454.50 EUR", "2254.50 EUR", "2224.50 EUR"]);
    }

    #[test]
    fn select_filters_orders_and_limits() {
        assert_eq!(
            rows("SELECT DISTINCT payee FROM transactions WHERE flag = '*' AND date >= 2024-01-15 ORDER BY payee DESC LIMIT 2"),
            [["Landlord"], ["Grocer"]]
        );
        assert_eq!(rows("SELECT count(*), max(number) FROM postings WHERE currency IN ('USD')"), [["2", "20"]]);
        assert_eq!(rows("SELECT root(account, 1) AS r, leaf(account), number * 2 FROM postings WHERE tags = 'work' LIMIT 1"), [["Assets", "Bank", "5000.00"]]);
        assert_eq!(rows("SELECT account, close FROM accounts WHERE NOT account ~ 'e' ORDER BY account"), Vec::<Vec<String>>::new());
        assert_eq!(rows("SELECT * FROM prices"), [["2024-02-28", "USD", "0.92", "EUR"]]);
    }

    #[test]
    fn syntax_errors_carry_the_position() {
        let error = bql::run(&ledger(), "SELECT account FROM postings WHERE").unwrap_err();
        assert_eq!(error.to_string(), "syntax error at 34: expected an expression, found end of query");
        let error = bql::run(&ledger(), "SELECT nope").unwrap_err();
        assert!(error.to_string().starts_with("unknown column `nope`"), "{error}");
    }

    #[tokio::test]
    async fn browse_pages_tables() {
        let driver = Beancount { ledger: ledger() };
        assert_eq!(driver.browse("ledger", "postings", 0).await.unwrap().rows.len(), 12);
        assert!(driver.browse("ledger", "postings", 1).await.unwrap().rows.is_empty());
    }
}
