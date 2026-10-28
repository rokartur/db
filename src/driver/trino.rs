use super::http::{client, json_cell, send};
use super::{Driver, Grid, PAGE, param, parse_url, password, path_segments, qualified, user};
use anyhow::{Result, bail};
use async_trait::async_trait;
use serde_json::Value;

/// Trino REST protocol: `trino://user@host:8080/catalog[/schema]`, `?tls=1` for https.
pub struct Trino {
    base: String,
    user: String,
    password: Option<String>,
    catalog: Option<String>,
    schema: Option<String>,
}

impl Trino {
    pub fn new(url: &str) -> Result<Self> {
        let parsed = parse_url(url)?;
        let scheme = if param(&parsed, "tls").as_deref() == Some("1") { "https" } else { "http" };
        let host = parsed.host_str().unwrap_or("localhost");
        let port = parsed.port().unwrap_or(8080);
        let mut path = path_segments(&parsed).into_iter();
        Ok(Trino {
            base: format!("{scheme}://{host}:{port}"),
            user: user(&parsed).unwrap_or_else(|| "dbtui".into()),
            password: password(&parsed),
            catalog: path.next(),
            schema: path.next(),
        })
    }
}

#[async_trait]
impl Driver for Trino {
    async fn query(&self, group: &str, text: &str) -> Result<Grid> {
        let mut request = client().post(format!("{}/v1/statement", self.base)).header("X-Trino-User", &self.user).body(text.to_string());
        if let Some(catalog) = &self.catalog {
            request = request.header("X-Trino-Catalog", catalog);
        }
        let schema = if group.is_empty() { self.schema.as_deref() } else { Some(group) };
        if let Some(schema) = schema {
            request = request.header("X-Trino-Schema", schema);
        }
        if let Some(password) = &self.password {
            request = request.basic_auth(&self.user, Some(password));
        }
        let mut grid = None;
        let mut response = send(request).await?;
        loop {
            absorb(&mut grid, &response)?;
            let Some(next) = response["nextUri"].as_str() else {
                return Ok(grid.unwrap_or_else(|| Grid::message("OK")));
            };
            let mut request = client().get(next).header("X-Trino-User", &self.user);
            if let Some(password) = &self.password {
                request = request.basic_auth(&self.user, Some(password));
            }
            response = send(request).await?;
        }
    }

    async fn browse(&self, group: &str, item: &str, page: usize) -> Result<Grid> {
        self.query(group, &browse_sql(group, item, page)).await
    }
}

/// Trino wants OFFSET before LIMIT.
fn browse_sql(group: &str, item: &str, page: usize) -> String {
    format!("SELECT * FROM {} OFFSET {} LIMIT {PAGE}", qualified(group, item, '"'), page * PAGE)
}

/// Adds one page of the protocol to the grid: columns arrive once, data in any later page.
fn absorb(grid: &mut Option<Grid>, page: &Value) -> Result<()> {
    if let Some(message) = page["error"]["message"].as_str() {
        bail!("{message}");
    }
    if let (None, Some(columns)) = (&grid, page["columns"].as_array()) {
        *grid = Some(Grid { columns: columns.iter().map(|c| json_cell(&c["name"])).collect(), rows: Vec::new() });
    }
    if let Some(grid) = grid {
        for row in page["data"].as_array().into_iter().flatten() {
            grid.rows.push(row.as_array().into_iter().flatten().map(json_cell).collect());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn collects_columns_and_data_across_pages() {
        let mut grid = None;
        super::absorb(&mut grid, &json!({"id": "q1", "nextUri": "http://x/1"})).unwrap();
        super::absorb(&mut grid, &json!({"columns": [{"name": "a"}], "data": [[1]], "nextUri": "http://x/2"})).unwrap();
        super::absorb(&mut grid, &json!({"data": [[null]]})).unwrap();
        assert_eq!(grid.unwrap().rows, [["1"], ["NULL"]]);
        let failed = super::absorb(&mut None, &json!({"error": {"message": "line 1:1: mismatched input"}}));
        assert_eq!(failed.unwrap_err().to_string(), "line 1:1: mismatched input");
    }
}
