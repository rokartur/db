use super::{Driver, Grid, Group, PAGE, group_rows, param, parse_url, password, qualified, user};
use anyhow::Result;
use async_trait::async_trait;
use scylla::client::session::Session;
use scylla::errors::TranslationError;
use scylla::policies::address_translator::{AddressTranslator, UntranslatedPeer};
use std::net::SocketAddr;
use std::sync::Arc;
use scylla::client::session_builder::SessionBuilder;
use scylla::response::query_result::IntoRowsResultError;
use scylla::value::{CqlValue, Row};

/// Cassandra and ScyllaDB over CQL: `cassandra://user:pass@host:9042`, `scylla://host:9042`.
/// `?single_node=1` sends everything to the URL's host, for nodes behind docker or a port-forward.
pub struct Cql {
    session: Session,
}

impl Cql {
    pub async fn connect(url: &str) -> Result<Self> {
        let parsed = parse_url(url)?;
        let node = format!("{}:{}", parsed.host_str().unwrap_or("localhost"), parsed.port().unwrap_or(9042));
        let mut builder = SessionBuilder::new().known_node(&node);
        if param(&parsed, "single_node").as_deref() == Some("1") {
            let address = tokio::net::lookup_host(&node).await?.next().ok_or_else(|| anyhow::anyhow!("cannot resolve {node}"))?;
            builder = builder.address_translator(Arc::new(OnlyNode(address)));
        }
        if let Some(user) = user(&parsed) {
            builder = builder.user(user, password(&parsed).unwrap_or_default());
        }
        Ok(Cql { session: builder.build().await? })
    }
}

#[async_trait]
impl Driver for Cql {
    async fn query(&self, _group: &str, text: &str) -> Result<Grid> {
        let result = self.session.query_unpaged(text, &[]).await?;
        let rows_result = match result.into_rows_result() {
            Ok(rows_result) => rows_result,
            Err(IntoRowsResultError::ResultNotRows(_)) => return Ok(Grid::message("OK")),
            Err(error) => return Err(error.into()),
        };
        let columns = rows_result.column_specs().iter().map(|spec| spec.name().to_string()).collect();
        let mut rows = Vec::new();
        for row in rows_result.rows::<Row>()? {
            rows.push(row?.columns.iter().map(|value| value.as_ref().map_or("NULL".into(), cell)).collect());
        }
        Ok(Grid { columns, rows })
    }

    async fn groups(&self) -> Result<Vec<Group>> {
        let mut grid = self.query("", "SELECT keyspace_name, table_name FROM system_schema.tables").await?;
        grid.rows.retain(|row| !row[0].starts_with("system"));
        grid.rows.sort();
        Ok(group_rows(&grid))
    }

    /// CQL has no OFFSET: read up to the end of the page and drop the rows before it.
    async fn browse(&self, group: &str, item: &str, page: usize) -> Result<Grid> {
        let sql = format!("SELECT * FROM {} LIMIT {}", qualified(group, item, '"'), (page + 1) * PAGE);
        let mut grid = self.query(group, &sql).await?;
        grid.rows.drain(..grid.rows.len().min(page * PAGE));
        Ok(grid)
    }
}

/// Peers announce addresses the client cannot reach (container IPs); route them all to one node.
struct OnlyNode(SocketAddr);

#[async_trait]
impl AddressTranslator for OnlyNode {
    async fn translate_address(&self, _peer: &UntranslatedPeer) -> Result<SocketAddr, TranslationError> {
        Ok(self.0)
    }
}

fn cell(value: &CqlValue) -> String {
    let list = |items: &[CqlValue]| format!("[{}]", items.iter().map(cell).collect::<Vec<_>>().join(", "));
    match value {
        CqlValue::Ascii(s) | CqlValue::Text(s) => s.clone(),
        CqlValue::Boolean(b) => b.to_string(),
        CqlValue::Blob(bytes) => format!("0x{}", hex::encode(bytes)),
        CqlValue::Counter(c) => c.0.to_string(),
        CqlValue::Double(x) => x.to_string(),
        CqlValue::Float(x) => x.to_string(),
        CqlValue::Int(n) => n.to_string(),
        CqlValue::BigInt(n) => n.to_string(),
        CqlValue::SmallInt(n) => n.to_string(),
        CqlValue::TinyInt(n) => n.to_string(),
        CqlValue::Uuid(u) => u.to_string(),
        CqlValue::Timeuuid(u) => u.to_string(),
        CqlValue::Inet(ip) => ip.to_string(),
        CqlValue::Date(d) => TryInto::<chrono::NaiveDate>::try_into(*d).map_or_else(|_| format!("{d:?}"), |d| d.to_string()),
        CqlValue::Timestamp(t) => TryInto::<chrono::DateTime<chrono::Utc>>::try_into(*t).map_or_else(|_| format!("{t:?}"), |t| t.to_rfc3339()),
        CqlValue::Time(t) => TryInto::<chrono::NaiveTime>::try_into(*t).map_or_else(|_| format!("{t:?}"), |t| t.to_string()),
        CqlValue::List(items) | CqlValue::Set(items) | CqlValue::Vector(items) => list(items),
        CqlValue::Map(pairs) => format!("{{{}}}", pairs.iter().map(|(k, v)| format!("{}: {}", cell(k), cell(v))).collect::<Vec<_>>().join(", ")),
        CqlValue::Tuple(items) => format!("({})", items.iter().map(|v| v.as_ref().map_or("NULL".into(), cell)).collect::<Vec<_>>().join(", ")),
        CqlValue::UserDefinedType { fields, .. } => format!(
            "{{{}}}",
            fields.iter().map(|(k, v)| format!("{k}: {}", v.as_ref().map_or("NULL".into(), cell))).collect::<Vec<_>>().join(", ")
        ),
        other => format!("{other:?}"),
    }
}
