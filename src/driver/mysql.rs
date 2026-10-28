use super::{Driver, Grid, browse_sql, quote_ident};
use anyhow::Result;
use async_trait::async_trait;
use mysql_async::prelude::Queryable;
use mysql_async::{Conn, Opts, Row, Value};
use tokio::sync::Mutex;

/// MySQL wire: MySQL, MariaDB, TiDB, OceanBase, Databend.
pub struct Mysql {
    conn: Mutex<Conn>,
}

impl Mysql {
    pub async fn connect(url: &str) -> Result<Self> {
        let (_, rest) = url.split_once("://").unwrap_or(("", url));
        let conn = Conn::new(Opts::from_url(&format!("mysql://{rest}"))?).await?;
        Ok(Mysql { conn: Mutex::new(conn) })
    }
}

#[async_trait]
impl Driver for Mysql {
    async fn query(&self, group: &str, text: &str) -> Result<Grid> {
        let mut conn = self.conn.lock().await;
        if !group.is_empty() {
            conn.query_drop(format!("USE {}", quote_ident(group, '`'))).await?;
        }
        let mut result = conn.query_iter(text).await?;
        loop {
            let columns = result.columns().filter(|columns| !columns.is_empty());
            let affected = result.affected_rows();
            let rows: Vec<Row> = result.collect().await?;
            let grid = match columns {
                Some(columns) => Grid {
                    columns: columns.iter().map(|c| c.name_str().into_owned()).collect(),
                    rows: rows.into_iter().map(|row| row.unwrap().iter().map(cell).collect()).collect(),
                },
                None => Grid::message(format!("{affected} rows affected")),
            };
            if result.is_empty() {
                return Ok(grid);
            }
        }
    }

    async fn browse(&self, group: &str, item: &str, page: usize) -> Result<Grid> {
        self.query(group, &browse_sql('`', group, item, page)).await
    }
}

fn cell(value: &Value) -> String {
    match value {
        Value::NULL => "NULL".into(),
        Value::Bytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        other => other.as_sql(true),
    }
}
