use super::Grid;
use anyhow::{Result, bail};
use reqwest::{Client, RequestBuilder};
use serde_json::{Map, Value};
use std::sync::LazyLock;
use std::time::Duration;

static CLIENT: LazyLock<Client> =
    LazyLock::new(|| Client::builder().timeout(Duration::from_secs(120)).build().expect("TLS backend"));

pub fn client() -> &'static Client {
    &CLIENT
}

/// Sends the request and returns the JSON body; a non-2xx status becomes an error carrying the body.
pub async fn send(request: RequestBuilder) -> Result<Value> {
    let response = request.send().await.map_err(|e| e.without_url())?;
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        bail!("HTTP {status}: {}", body.chars().take(2000).collect::<String>());
    }
    if body.trim().is_empty() {
        return Ok(Value::Null);
    }
    Ok(serde_json::from_str(&body)?)
}

pub fn json_cell(value: &Value) -> String {
    match value {
        Value::Null => "NULL".into(),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// Turns any JSON answer into a table: the first array of objects found becomes rows,
/// an array of arrays gets numbered columns, anything else becomes one flattened row.
pub fn json_to_grid(value: &Value) -> Grid {
    if let Some(rows) = first_array_of_objects(value) {
        return objects_to_grid(rows);
    }
    match value {
        Value::Array(items) if items.iter().all(Value::is_array) && !items.is_empty() => {
            let width = items.iter().filter_map(Value::as_array).map(Vec::len).max().unwrap_or(0);
            let rows = items
                .iter()
                .filter_map(Value::as_array)
                .map(|row| (0..width).map(|i| row.get(i).map(json_cell).unwrap_or_default()).collect())
                .collect();
            Grid { columns: (0..width).map(|i| i.to_string()).collect(), rows }
        }
        Value::Array(items) => {
            Grid { columns: vec!["value".into()], rows: items.iter().map(|item| vec![json_cell(item)]).collect() }
        }
        Value::Object(_) => objects_to_grid(std::slice::from_ref(value)),
        scalar => Grid { columns: vec!["value".into()], rows: vec![vec![json_cell(scalar)]] },
    }
}

fn first_array_of_objects(value: &Value) -> Option<&[Value]> {
    match value {
        Value::Array(items) if !items.is_empty() && items.iter().all(Value::is_object) => Some(items),
        Value::Object(map) => map.values().find_map(first_array_of_objects),
        _ => None,
    }
}

pub fn objects_to_grid(objects: &[Value]) -> Grid {
    let mut columns: Vec<String> = Vec::new();
    let mut flat_rows = Vec::new();
    for object in objects {
        let mut flat = Map::new();
        flatten("", object, &mut flat);
        for key in flat.keys() {
            if !columns.contains(key) {
                columns.push(key.clone());
            }
        }
        flat_rows.push(flat);
    }
    let rows = flat_rows
        .iter()
        .map(|flat| columns.iter().map(|c| flat.get(c).map(json_cell).unwrap_or_default()).collect())
        .collect();
    Grid { columns, rows }
}

fn flatten(prefix: &str, value: &Value, out: &mut Map<String, Value>) {
    match value {
        Value::Object(map) if !map.is_empty() => {
            for (key, child) in map {
                let name = if prefix.is_empty() { key.clone() } else { format!("{prefix}.{key}") };
                flatten(&name, child, out);
            }
        }
        other => {
            let name = if prefix.is_empty() { "value".to_string() } else { prefix.to_string() };
            out.insert(name, other.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn objects_become_flattened_columns() {
        let grid = json_to_grid(&json!({"hits": [{"a": 1, "b": {"c": "x"}}, {"a": 2, "d": null}]}));
        assert_eq!(grid.columns, ["a", "b.c", "d"]);
        assert_eq!(grid.rows, [["1", "x", ""], ["2", "", "NULL"]]);
    }

    #[test]
    fn arrays_of_arrays_get_numbered_columns() {
        let grid = json_to_grid(&json!([[1, "a"], [2]]));
        assert_eq!(grid.columns, ["0", "1"]);
        assert_eq!(grid.rows, [["1", "a"], ["2", ""]]);
    }

    #[test]
    fn scalars_and_plain_objects_make_one_row() {
        assert_eq!(json_to_grid(&json!("ok")).rows, [["ok"]]);
        let grid = json_to_grid(&json!({"count": 3, "tags": [1, 2]}));
        assert_eq!(grid.columns, ["count", "tags"]);
        assert_eq!(grid.rows, [["3", "[1,2]"]]);
    }
}
