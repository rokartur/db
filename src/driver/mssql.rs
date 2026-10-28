use super::{Driver, Grid, PAGE, param, parse_url, password, path_segments, qualified, user};
use anyhow::Result;
use async_trait::async_trait;
use futures::TryStreamExt;
use std::fmt::Display;
use tiberius::{AuthMethod, Client, ColumnData, Config, FromSql, QueryItem};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};

/// `sqlserver://user:pass@host:1433/db?trust_cert=1`
pub struct Mssql {
    client: Mutex<Client<Compat<TcpStream>>>,
}

impl Mssql {
    pub async fn connect(url: &str) -> Result<Self> {
        let url = parse_url(url)?;
        let mut config = Config::new();
        config.host(url.host_str().unwrap_or("localhost"));
        config.port(url.port().unwrap_or(1433));
        config.authentication(AuthMethod::sql_server(user(&url).unwrap_or_default(), password(&url).unwrap_or_default()));
        if let Some(database) = path_segments(&url).first() {
            config.database(database);
        }
        if param(&url, "trust_cert").is_some_and(|v| v == "1" || v == "true") {
            config.trust_cert();
        }
        let tcp = TcpStream::connect(config.get_addr()).await?;
        tcp.set_nodelay(true)?;
        let client = Client::connect(config, tcp.compat_write()).await?;
        Ok(Mssql { client: Mutex::new(client) })
    }
}

#[async_trait]
impl Driver for Mssql {
    async fn query(&self, _group: &str, text: &str) -> Result<Grid> {
        let mut client = self.client.lock().await;
        let mut stream = client.simple_query(text).await?;
        let mut grid = Grid::message("OK");
        while let Some(item) = stream.try_next().await? {
            match item {
                QueryItem::Metadata(meta) => {
                    grid = Grid { columns: meta.columns().iter().map(|c| c.name().to_string()).collect(), rows: Vec::new() };
                }
                QueryItem::Row(row) => grid.rows.push(row.into_iter().map(cell).collect()),
            }
        }
        Ok(grid)
    }

    async fn browse(&self, group: &str, item: &str, page: usize) -> Result<Grid> {
        let sql = format!(
            "SELECT * FROM {} ORDER BY (SELECT NULL) OFFSET {} ROWS FETCH NEXT {PAGE} ROWS ONLY",
            qualified(group, item, '['),
            page * PAGE
        );
        self.query(group, &sql).await
    }
}

fn cell(data: ColumnData<'static>) -> String {
    fn text<T: Display>(value: Option<T>) -> String {
        value.map_or("NULL".into(), |v| v.to_string())
    }
    fn via_chrono<'a, T: FromSql<'a> + Display>(data: &'a ColumnData<'static>) -> String {
        match T::from_sql(data) {
            Ok(value) => text(value),
            Err(error) => format!("<{error}>"),
        }
    }
    match &data {
        ColumnData::U8(v) => text(*v),
        ColumnData::I16(v) => text(*v),
        ColumnData::I32(v) => text(*v),
        ColumnData::I64(v) => text(*v),
        ColumnData::F32(v) => text(*v),
        ColumnData::F64(v) => text(*v),
        ColumnData::Bit(v) => text(*v),
        ColumnData::String(v) => text(v.as_deref()),
        ColumnData::Guid(v) => text(*v),
        ColumnData::Binary(v) => v.as_ref().map_or("NULL".into(), |b| format!("0x{}", hex::encode(b))),
        ColumnData::Numeric(v) => text(*v),
        ColumnData::Xml(v) => text(v.as_deref()),
        ColumnData::DateTime(_) | ColumnData::SmallDateTime(_) | ColumnData::DateTime2(_) => {
            via_chrono::<chrono::NaiveDateTime>(&data)
        }
        ColumnData::Date(_) => via_chrono::<chrono::NaiveDate>(&data),
        ColumnData::Time(_) => via_chrono::<chrono::NaiveTime>(&data),
        ColumnData::DateTimeOffset(_) => via_chrono::<chrono::DateTime<chrono::FixedOffset>>(&data),
    }
}
