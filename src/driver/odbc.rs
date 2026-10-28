use super::{Driver, Grid, Group, PAGE, qualified};
use anyhow::Result;
use async_trait::async_trait;
use odbc_api::buffers::TextRowSet;
use odbc_api::{Connection, ConnectionOptions, Cursor, Environment, ResultSetMetadata};
use std::sync::{LazyLock, Mutex};

static ENVIRONMENT: LazyLock<Environment> = LazyLock::new(|| Environment::new().expect("ODBC environment"));
const MAX_TEXT: usize = 4096;

/// Any ODBC driver: `odbc:Driver={...};...` or `odbc:DSN=name;UID=..;PWD=..`.
/// Used for Oracle, SAP HANA, Teradata and Dameng DM8.
pub struct Odbc {
    conn: Mutex<Connection<'static>>,
}

impl Odbc {
    pub fn connect(url: &str) -> Result<Self> {
        let connection_string = url.strip_prefix("odbc:").unwrap_or(url);
        let conn = ENVIRONMENT.connect_with_connection_string(connection_string, ConnectionOptions::default())?;
        Ok(Odbc { conn: Mutex::new(conn) })
    }

    /// Dialects differ in paging syntax, so the page is cut out of the cursor instead.
    fn read(&self, text: &str, skip: usize, limit: Option<usize>) -> Result<Grid> {
        let conn = self.conn.lock().unwrap();
        let Some(mut cursor) = conn.execute(text, (), None)? else {
            return Ok(Grid::message("OK"));
        };
        let columns: Vec<String> = cursor.column_names()?.collect::<Result<_, _>>()?;
        let buffer = TextRowSet::for_cursor(PAGE, &mut cursor, Some(MAX_TEXT))?;
        let mut block_cursor = cursor.bind_buffer(buffer)?;
        let mut rows = Vec::new();
        let mut seen = 0;
        while let Some(batch) = block_cursor.fetch()? {
            for row in 0..batch.num_rows() {
                seen += 1;
                if seen <= skip {
                    continue;
                }
                if limit.is_some_and(|limit| rows.len() == limit) {
                    return Ok(Grid { columns, rows });
                }
                let cells = (0..batch.num_cols()).map(|col| {
                    batch.at_as_str(col, row).map(|text| text.unwrap_or("NULL").to_string())
                });
                rows.push(cells.collect::<Result<_, _>>()?);
            }
        }
        Ok(Grid { columns, rows })
    }
}

#[async_trait]
impl Driver for Odbc {
    async fn query(&self, _group: &str, text: &str) -> Result<Grid> {
        self.read(text, 0, None)
    }

    async fn groups(&self) -> Result<Vec<Group>> {
        let conn = self.conn.lock().unwrap();
        let mut groups: Vec<Group> = Vec::new();
        for row in conn.tables("", "", "", "TABLE,VIEW")? {
            let row = row?;
            let schema = row.schema.as_str()?.unwrap_or("").to_string();
            let table = row.table.as_str()?.unwrap_or("").to_string();
            match groups.iter_mut().find(|g| g.name == schema) {
                Some(group) => group.items.push(table),
                None => groups.push(Group { name: schema, items: vec![table] }),
            }
        }
        Ok(groups)
    }

    async fn browse(&self, group: &str, item: &str, page: usize) -> Result<Grid> {
        self.read(&format!("SELECT * FROM {}", qualified(group, item, '"')), page * PAGE, Some(PAGE))
    }
}
