use super::{Driver, Grid, Group, group_rows};
use anyhow::Result;
use async_trait::async_trait;
use rusqlite::fallible_iterator::FallibleIterator;
use rusqlite::types::ValueRef;
use rusqlite::{Batch, Connection};
use std::sync::Mutex;

pub struct Sqlite {
    conn: Mutex<Connection>,
}

impl Sqlite {
    pub fn open(path: &str) -> Result<Self> {
        Ok(Sqlite { conn: Mutex::new(Connection::open(path)?) })
    }
}

#[async_trait]
impl Driver for Sqlite {
    async fn query(&self, _group: &str, text: &str) -> Result<Grid> {
        run(&self.conn.lock().unwrap(), text)
    }

    async fn groups(&self) -> Result<Vec<Group>> {
        let sql = "SELECT 'main', name FROM sqlite_master \
                   WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%' ORDER BY name";
        Ok(group_rows(&self.query("", sql).await?))
    }
}

/// Runs every statement in `text`; the last one that returns rows wins, otherwise the affected-row count.
fn run(conn: &Connection, text: &str) -> Result<Grid> {
    let mut batch = Batch::new(conn, text);
    let mut result = Grid::message("OK");
    while let Some(mut statement) = batch.next()? {
        if statement.column_count() == 0 {
            let affected = statement.raw_execute()?;
            result = Grid::message(format!("{affected} rows affected"));
            continue;
        }
        let columns: Vec<String> = statement.column_names().map(String::from).collect();
        let width = columns.len();
        let mut rows = Vec::new();
        let mut cursor = statement.raw_query();
        while let Some(row) = cursor.next()? {
            rows.push((0..width).map(|i| row.get_ref(i).map(cell)).collect::<Result<_, _>>()?);
        }
        result = Grid { columns, rows };
    }
    Ok(result)
}

pub fn cell(value: ValueRef) -> String {
    match value {
        ValueRef::Null => "NULL".into(),
        ValueRef::Integer(n) => n.to_string(),
        ValueRef::Real(x) => x.to_string(),
        ValueRef::Text(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        ValueRef::Blob(bytes) => format!("0x{}", hex::encode(bytes)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn runs_statements_and_lists_tables() {
        let db = Sqlite::open(":memory:").unwrap();
        let grid = db.query("", "create table t(a, b); insert into t values (1, 'x'), (2, null); select * from t").await;
        assert_eq!(grid.unwrap().rows, [["1", "x"], ["2", "NULL"]]);
        assert_eq!(db.groups().await.unwrap(), [Group { name: "main".into(), items: vec!["t".into()] }]);
        assert_eq!(db.browse("main", "t", 1).await.unwrap().rows.len(), 0);
        assert!(db.query("", "selec 1").await.is_err());
    }
}
