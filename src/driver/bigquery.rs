use super::http::{client, json_cell, send};
use super::{Driver, Grid, Group, browse_sql, param, parse_url};
use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::RequestBuilder;
use serde_json::{Value, json};

/// BigQuery REST: `bigquery://project?token=...&endpoint=http://localhost:9050` (endpoint for the emulator).
pub struct Bigquery {
    base: String,
    token: Option<String>,
}

impl Bigquery {
    pub fn new(url: &str) -> Result<Self> {
        let parsed = parse_url(url)?;
        let project = parsed.host_str().context("bigquery URL needs a project: bigquery://project")?;
        let endpoint = param(&parsed, "endpoint").unwrap_or_else(|| "https://bigquery.googleapis.com".into());
        let token = param(&parsed, "token").or_else(|| std::env::var("GOOGLE_OAUTH_TOKEN").ok());
        Ok(Bigquery { base: format!("{}/bigquery/v2/projects/{project}", endpoint.trim_end_matches('/')), token })
    }

    fn authorized(&self, request: RequestBuilder) -> RequestBuilder {
        match &self.token {
            Some(token) => request.bearer_auth(token),
            None => request,
        }
    }

    async fn get(&self, path: &str) -> Result<Value> {
        send(self.authorized(client().get(format!("{}{path}", self.base)))).await
    }
}

#[async_trait]
impl Driver for Bigquery {
    async fn query(&self, group: &str, text: &str) -> Result<Grid> {
        let mut body = json!({"query": text, "useLegacySql": false, "timeoutMs": 30000});
        if !group.is_empty() {
            let project = self.base.rsplit('/').next().unwrap_or_default();
            body["defaultDataset"] = json!({"projectId": project, "datasetId": group});
        }
        let mut response = send(self.authorized(client().post(format!("{}/queries", self.base)).json(&body))).await?;
        while response["jobComplete"] == false {
            let job = json_cell(&response["jobReference"]["jobId"]);
            response = self.get(&format!("/queries/{job}?timeoutMs=30000")).await?;
        }
        Ok(parse_query_response(&response))
    }

    async fn groups(&self) -> Result<Vec<Group>> {
        let datasets = self.get("/datasets").await?;
        let mut groups = Vec::new();
        for dataset in datasets["datasets"].as_array().into_iter().flatten() {
            let name = json_cell(&dataset["datasetReference"]["datasetId"]);
            let tables = self.get(&format!("/datasets/{name}/tables")).await?;
            let items = tables["tables"].as_array().into_iter().flatten().map(|t| json_cell(&t["tableReference"]["tableId"])).collect();
            groups.push(Group { name, items });
        }
        Ok(groups)
    }

    async fn browse(&self, group: &str, item: &str, page: usize) -> Result<Grid> {
        self.query(group, &browse_sql('`', group, item, page)).await
    }
}

/// shortcut: only the first page of rows (`pageToken` is not followed); browse pages by OFFSET anyway.
fn parse_query_response(response: &Value) -> Grid {
    let Some(fields) = response["schema"]["fields"].as_array() else {
        let affected = response["numDmlAffectedRows"].as_str().unwrap_or("0");
        return Grid::message(format!("{affected} rows affected"));
    };
    let columns = fields.iter().map(|f| json_cell(&f["name"])).collect();
    let rows = response["rows"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|row| row["f"].as_array().into_iter().flatten().map(|cell| json_cell(&cell["v"])).collect())
        .collect();
    Grid { columns, rows }
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_rows_of_f_v_cells() {
        let response = serde_json::json!({"jobComplete": true,
            "schema": {"fields": [{"name": "id", "type": "INTEGER"}, {"name": "name", "type": "STRING"}]},
            "rows": [{"f": [{"v": "1"}, {"v": "Ala"}]}, {"f": [{"v": "2"}, {"v": null}]}]});
        let grid = super::parse_query_response(&response);
        assert_eq!(grid.columns, ["id", "name"]);
        assert_eq!(grid.rows, [["1", "Ala"], ["2", "NULL"]]);
    }
}
