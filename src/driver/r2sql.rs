use super::http::{client, json_cell, json_to_grid, send};
use super::{Driver, Grid, Group, param, parse_url, path_segments};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde_json::{Value, json};

/// Cloudflare R2 SQL (Basin SQL): `r2sql://account_id/bucket?token=...`.
pub struct R2sql {
    endpoint: String,
    token: String,
}

impl R2sql {
    pub fn new(url: &str) -> Result<Self> {
        let parsed = parse_url(url)?;
        let account = parsed.host_str().context("r2sql URL needs an account id: r2sql://account/bucket")?;
        let bucket = path_segments(&parsed).into_iter().next().context("r2sql URL needs a bucket")?;
        let token = param(&parsed, "token")
            .or_else(|| std::env::var("CLOUDFLARE_API_TOKEN").ok())
            .context("r2sql needs ?token= or CLOUDFLARE_API_TOKEN")?;
        let endpoint = format!("https://api.sql.cloudflarestorage.com/api/v1/accounts/{account}/basin-sql/query/{bucket}");
        Ok(R2sql { endpoint, token })
    }
}

#[async_trait]
impl Driver for R2sql {
    async fn query(&self, _group: &str, text: &str) -> Result<Grid> {
        let request = client().post(&self.endpoint).bearer_auth(&self.token).json(&json!({"query": text}));
        parse_response(&send(request).await?)
    }

    async fn groups(&self) -> Result<Vec<Group>> {
        let mut groups = Vec::new();
        for namespace in first_column(&self.query("", "SHOW DATABASES").await?) {
            let items = first_column(&self.query("", &format!("SHOW TABLES IN {namespace}")).await?);
            groups.push(Group { name: namespace, items });
        }
        Ok(groups)
    }
}

fn first_column(grid: &Grid) -> Vec<String> {
    grid.rows.iter().filter_map(|row| row.first().cloned()).collect()
}

/// The response shape is undocumented: Cloudflare's `{success, errors, result}` envelope is assumed,
/// and whatever `result` holds goes through the generic JSON-to-table mapping.
fn parse_response(response: &Value) -> Result<Grid> {
    if response["success"] == false {
        let messages: Vec<String> = response["errors"].as_array().into_iter().flatten().map(|e| json_cell(&e["message"])).collect();
        bail!("{}", messages.join("; "));
    }
    let result = if response["result"].is_null() { response } else { &response["result"] };
    Ok(json_to_grid(result.get("rows").unwrap_or(result)))
}

#[cfg(test)]
mod tests {
    #[test]
    fn maps_result_rows_generically() {
        let response = serde_json::json!({"success": true, "errors": [], "result": {
            "schema": [{"name": "id"}, {"name": "city"}],
            "rows": [{"id": 1, "city": "Kraków"}, {"id": 2, "city": null}]}});
        let grid = super::parse_response(&response).unwrap();
        assert_eq!(grid.columns, ["id", "city"]);
        assert_eq!(grid.rows, [["1", "Kraków"], ["2", "NULL"]]);
        let failed = serde_json::json!({"success": false, "errors": [{"code": 40003, "message": "table not found"}]});
        assert_eq!(super::parse_response(&failed).unwrap_err().to_string(), "table not found");
    }
}
