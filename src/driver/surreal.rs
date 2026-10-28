use super::http::{client, json_to_grid, send};
use super::{Driver, Grid, Group, PAGE, param, parse_url, password, path_segments, user};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde_json::Value;

/// SurrealDB over HTTP: `surrealdb://user:pass@host:8000/namespace/database`, `?tls=1` for https.
pub struct Surreal {
    endpoint: String,
    user: Option<String>,
    password: Option<String>,
    namespace: String,
    database: String,
}

impl Surreal {
    pub fn new(url: &str) -> Result<Self> {
        let parsed = parse_url(url)?;
        let scheme = if param(&parsed, "tls").as_deref() == Some("1") { "https" } else { "http" };
        let host = parsed.host_str().unwrap_or("localhost");
        let port = parsed.port().unwrap_or(8000);
        let [namespace, database] = path_segments(&parsed).try_into().ok().context("surrealdb URL: surrealdb://user:pass@host:8000/ns/db")?;
        Ok(Surreal { endpoint: format!("{scheme}://{host}:{port}/sql"), user: user(&parsed), password: password(&parsed), namespace, database })
    }
}

#[async_trait]
impl Driver for Surreal {
    async fn query(&self, _group: &str, text: &str) -> Result<Grid> {
        let mut request = client()
            .post(&self.endpoint)
            .header("Accept", "application/json")
            .header("Surreal-NS", &self.namespace)
            .header("Surreal-DB", &self.database)
            .body(text.to_string());
        if let Some(user) = &self.user {
            request = request.basic_auth(user, self.password.as_ref());
        }
        Ok(json_to_grid(last_result(&send(request).await?)?))
    }

    async fn groups(&self) -> Result<Vec<Group>> {
        let info = self.query("", "INFO FOR DB").await?;
        let tables = info.columns.iter().filter_map(|c| c.strip_prefix("tables.")).map(String::from).collect();
        Ok(vec![Group { name: self.database.clone(), items: tables }])
    }

    async fn browse(&self, _group: &str, item: &str, page: usize) -> Result<Grid> {
        // A JSON string is a valid SurrealQL string literal, so the table name cannot break out of it.
        let table = serde_json::to_string(item)?;
        self.query("", &format!("SELECT * FROM type::table({table}) LIMIT {PAGE} START {}", page * PAGE)).await
    }
}

/// The reply has one `{status, result}` per statement; any `ERR` fails the query.
fn last_result(reply: &Value) -> Result<&Value> {
    let statements = reply.as_array().context("unexpected SurrealDB reply")?;
    for statement in statements {
        if statement["status"] == "ERR" {
            bail!("{}", super::http::json_cell(&statement["result"]));
        }
    }
    Ok(statements.last().map_or(&Value::Null, |s| &s["result"]))
}

#[cfg(test)]
mod tests {
    #[test]
    fn takes_last_statement_and_reports_errors() {
        let reply = serde_json::json!([{"status": "OK", "result": []}, {"status": "OK", "result": [{"id": "t:1", "n": 1}]}]);
        assert_eq!(super::last_result(&reply).unwrap()[0]["n"], 1);
        let failed = serde_json::json!([{"status": "ERR", "result": "Parse error"}]);
        assert_eq!(super::last_result(&failed).unwrap_err().to_string(), "Parse error");
    }
}
