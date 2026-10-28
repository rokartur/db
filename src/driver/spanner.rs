use super::http::{client, json_cell, send};
use super::{Driver, Grid, browse_sql, param, parse_url, path_segments};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use reqwest::RequestBuilder;
use serde_json::{Value, json};
use std::time::Duration;

/// Cloud Spanner REST: `spanner://project/instance/database?token=...&endpoint=http://localhost:9020`.
pub struct Spanner {
    base: String,
    database: String,
    session: String,
    token: Option<String>,
}

impl Spanner {
    pub async fn connect(url: &str) -> Result<Self> {
        let parsed = parse_url(url)?;
        let project = parsed.host_str().context("spanner URL: spanner://project/instance/database")?;
        let [instance, database] = path_segments(&parsed).try_into().ok().context("spanner URL: spanner://project/instance/database")?;
        let endpoint = param(&parsed, "endpoint").unwrap_or_else(|| "https://spanner.googleapis.com".into());
        let mut spanner = Spanner {
            base: format!("{}/v1", endpoint.trim_end_matches('/')),
            database: format!("projects/{project}/instances/{instance}/databases/{database}"),
            session: String::new(),
            token: param(&parsed, "token").or_else(|| std::env::var("GOOGLE_OAUTH_TOKEN").ok()),
        };
        let session = spanner.post(&format!("{}/sessions", spanner.database), json!({})).await?;
        spanner.session = json_cell(&session["name"]);
        Ok(spanner)
    }

    fn authorized(&self, request: RequestBuilder) -> RequestBuilder {
        match &self.token {
            Some(token) => request.bearer_auth(token),
            None => request,
        }
    }

    async fn post(&self, path: &str, body: Value) -> Result<Value> {
        send(self.authorized(client().post(format!("{}/{path}", self.base)).json(&body))).await
    }

    async fn update_ddl(&self, text: &str) -> Result<Grid> {
        let request = client().patch(format!("{}/{}/ddl", self.base, self.database)).json(&json!({"statements": [text]}));
        let mut operation = send(self.authorized(request)).await?;
        while operation["done"] != true {
            tokio::time::sleep(Duration::from_millis(300)).await;
            let name = json_cell(&operation["name"]);
            operation = send(self.authorized(client().get(format!("{}/{name}", self.base)))).await?;
        }
        if let Some(message) = operation["error"]["message"].as_str() {
            bail!("{message}");
        }
        Ok(Grid::message("OK"))
    }

    async fn dml(&self, text: &str) -> Result<Grid> {
        let body = json!({"sql": text, "seqno": "1", "transaction": {"begin": {"readWrite": {}}}});
        let response = self.post(&format!("{}:executeSql", self.session), body).await?;
        let transaction = json_cell(&response["metadata"]["transaction"]["id"]);
        self.post(&format!("{}:commit", self.session), json!({"transactionId": transaction})).await?;
        Ok(Grid::message(format!("{} rows affected", json_cell(&response["stats"]["rowCountExact"]))))
    }
}

#[async_trait]
impl Driver for Spanner {
    async fn query(&self, _group: &str, text: &str) -> Result<Grid> {
        let keyword = text.split_whitespace().next().unwrap_or_default().to_ascii_uppercase();
        match keyword.as_str() {
            "CREATE" | "ALTER" | "DROP" => self.update_ddl(text).await,
            "INSERT" | "UPDATE" | "DELETE" => self.dml(text).await,
            _ => Ok(parse_result_set(&self.post(&format!("{}:executeSql", self.session), json!({"sql": text})).await?)),
        }
    }

    async fn browse(&self, group: &str, item: &str, page: usize) -> Result<Grid> {
        self.query(group, &browse_sql('`', group, item, page)).await
    }
}

fn parse_result_set(response: &Value) -> Grid {
    let fields = response["metadata"]["rowType"]["fields"].as_array().into_iter().flatten();
    let columns = fields.map(|f| json_cell(&f["name"])).collect();
    let rows = response["rows"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|row| row.as_array().into_iter().flatten().map(json_cell).collect())
        .collect();
    Grid { columns, rows }
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_result_set() {
        let response = serde_json::json!({"metadata": {"rowType": {"fields": [
            {"name": "id", "type": {"code": "INT64"}}, {"name": "tags", "type": {"code": "ARRAY"}}]}},
            "rows": [["1", ["a", "b"]], ["2", null]]});
        let grid = super::parse_result_set(&response);
        assert_eq!(grid.columns, ["id", "tags"]);
        assert_eq!(grid.rows, [["1", "[\"a\",\"b\"]"], ["2", "NULL"]]);
    }
}
