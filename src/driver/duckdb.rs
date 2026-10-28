use super::{Driver, Grid};
use anyhow::Result;
use async_trait::async_trait;
use duckdb::Connection;
use duckdb::arrow::array::Array;
use duckdb::arrow::record_batch::RecordBatch;
use duckdb::arrow::util::display::array_value_to_string;
use std::sync::Mutex;

pub struct Duckdb {
    conn: Mutex<Connection>,
}

impl Duckdb {
    pub fn open(path: &str) -> Result<Self> {
        Ok(Duckdb { conn: Mutex::new(Connection::open(path)?) })
    }
}

#[async_trait]
impl Driver for Duckdb {
    async fn query(&self, _group: &str, text: &str) -> Result<Grid> {
        let conn = self.conn.lock().unwrap();
        let mut statement = conn.prepare(text)?;
        let batches: Vec<RecordBatch> = statement.query_arrow([])?.collect();
        let columns = statement.schema().fields().iter().map(|f| f.name().clone()).collect();
        let mut rows = Vec::new();
        for batch in &batches {
            for row in 0..batch.num_rows() {
                rows.push(batch.columns().iter().map(|column| cell(column.as_ref(), row)).collect::<Result<_>>()?);
            }
        }
        Ok(Grid { columns, rows })
    }
}

fn cell(column: &dyn Array, row: usize) -> Result<String> {
    if column.is_null(row) {
        return Ok("NULL".into());
    }
    Ok(array_value_to_string(column, row)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn queries_browses_and_lists_tables() {
        let db = Duckdb::open(":memory:").unwrap();
        db.query("", "create table t(a int, b varchar, d date)").await.unwrap();
        db.query("", "insert into t values (1, 'x', '2024-01-31'), (2, null, null)").await.unwrap();
        assert_eq!(db.browse("main", "t", 0).await.unwrap().rows, [["1", "x", "2024-01-31"], ["2", "NULL", "NULL"]]);
        assert_eq!(db.groups().await.unwrap()[0].items, ["t"]);
        assert!(db.query("", "selec 1").await.is_err());
    }
}
