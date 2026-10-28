use super::http::{client, json_cell, json_to_grid, objects_to_grid, send};
use super::{Driver, Grid, Group, PAGE, param, parse_url, password, user};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use reqwest::Method;
use serde_json::{Map, Value, json};

#[derive(Debug, Clone, Copy, PartialEq)]
enum Engine {
    Elasticsearch,
    Typesense,
    Weaviate,
}

/// Elasticsearch, Typesense and Weaviate over REST. A query is `METHOD /path` with an optional JSON body
/// on the next lines; on Elasticsearch plain text goes to `/_sql`.
pub struct Search {
    engine: Engine,
    base: String,
    user: Option<String>,
    password: Option<String>,
    token: Option<String>,
}

impl Search {
    pub fn new(url: &str) -> Result<Self> {
        let parsed = parse_url(url)?;
        let (engine, default_port) = match parsed.scheme() {
            "elasticsearch" => (Engine::Elasticsearch, 9200),
            "typesense" => (Engine::Typesense, 8108),
            _ => (Engine::Weaviate, 8080),
        };
        let scheme = if param(&parsed, "tls").as_deref() == Some("1") { "https" } else { "http" };
        let host = parsed.host_str().unwrap_or("localhost");
        let port = parsed.port().unwrap_or(default_port);
        Ok(Search {
            engine,
            base: format!("{scheme}://{host}:{port}"),
            user: user(&parsed),
            password: password(&parsed),
            token: param(&parsed, "token"),
        })
    }

    async fn call(&self, method: Method, path: &str, body: Option<Value>) -> Result<Value> {
        let mut request = client().request(method, format!("{}{path}", self.base));
        match self.engine {
            Engine::Typesense => request = request.header("X-TYPESENSE-API-KEY", self.user.as_deref().unwrap_or_default()),
            _ if self.user.is_some() => request = request.basic_auth(self.user.as_deref().unwrap_or_default(), self.password.as_ref()),
            _ => {}
        }
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        send(request).await
    }
}

#[async_trait]
impl Driver for Search {
    async fn query(&self, _group: &str, text: &str) -> Result<Grid> {
        if let Some((method, path, body)) = parse_raw_request(text)? {
            return Ok(json_to_grid(&self.call(method, &path, body).await?));
        }
        if self.engine != Engine::Elasticsearch {
            bail!("start the query with `METHOD /path`, e.g. `GET /collections`");
        }
        let response = self.call(Method::POST, "/_sql?format=json", Some(json!({"query": text}))).await?;
        let columns = response["columns"].as_array().into_iter().flatten().map(|c| json_cell(&c["name"])).collect();
        let rows = response["rows"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|row| row.as_array().into_iter().flatten().map(json_cell).collect())
            .collect();
        Ok(Grid { columns, rows })
    }

    async fn groups(&self) -> Result<Vec<Group>> {
        let (name, mut items): (&str, Vec<String>) = match self.engine {
            Engine::Elasticsearch => {
                let indices = self.call(Method::GET, "/_cat/indices?format=json", None).await?;
                let names = indices.as_array().into_iter().flatten().map(|i| json_cell(&i["index"]));
                ("indices", names.filter(|name| !name.starts_with('.')).collect())
            }
            Engine::Typesense => {
                let collections = self.call(Method::GET, "/collections", None).await?;
                ("collections", collections.as_array().into_iter().flatten().map(|c| json_cell(&c["name"])).collect())
            }
            Engine::Weaviate => {
                let schema = self.call(Method::GET, "/v1/schema", None).await?;
                ("classes", schema["classes"].as_array().into_iter().flatten().map(|c| json_cell(&c["class"])).collect())
            }
        };
        items.sort();
        Ok(vec![Group { name: name.into(), items }])
    }

    async fn browse(&self, _group: &str, item: &str, page: usize) -> Result<Grid> {
        let name = utf8_percent_encode(item, NON_ALPHANUMERIC);
        let documents: Vec<Value> = match self.engine {
            Engine::Elasticsearch => {
                let body = json!({"from": page * PAGE, "size": PAGE, "query": {"match_all": {}}});
                let response = self.call(Method::POST, &format!("/{name}/_search"), Some(body)).await?;
                let hits = response["hits"]["hits"].as_array().cloned().unwrap_or_default();
                hits.iter().map(|hit| with_id(&hit["_id"], &hit["_source"])).collect()
            }
            Engine::Typesense => {
                let path = format!("/collections/{name}/documents/search?q=*&page={}&per_page={PAGE}", page + 1);
                let response = self.call(Method::GET, &path, None).await?;
                response["hits"].as_array().into_iter().flatten().map(|hit| hit["document"].clone()).collect()
            }
            Engine::Weaviate => {
                let path = format!("/v1/objects?class={name}&limit={PAGE}&offset={}", page * PAGE);
                let response = self.call(Method::GET, &path, None).await?;
                let objects = response["objects"].as_array().cloned().unwrap_or_default();
                objects.iter().map(|object| with_id(&object["id"], &object["properties"])).collect()
            }
        };
        Ok(objects_to_grid(&documents))
    }
}

fn with_id(id: &Value, fields: &Value) -> Value {
    let mut object = Map::new();
    object.insert("_id".into(), id.clone());
    object.extend(fields.as_object().cloned().unwrap_or_default());
    Value::Object(object)
}

/// `GET /path` on the first line, optional JSON body on the following ones; `None` when the text is not a raw request.
fn parse_raw_request(text: &str) -> Result<Option<(Method, String, Option<Value>)>> {
    let text = text.trim();
    let (first_line, body) = text.split_once('\n').unwrap_or((text, ""));
    let mut words = first_line.split_whitespace();
    let (Some(method), Some(path), None) = (words.next(), words.next(), words.next()) else { return Ok(None) };
    let methods = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"];
    if !methods.contains(&method.to_ascii_uppercase().as_str()) || !path.starts_with('/') {
        return Ok(None);
    }
    let method = Method::from_bytes(method.to_ascii_uppercase().as_bytes())?;
    let body = if body.trim().is_empty() { None } else { Some(serde_json::from_str(body).context("request body is not JSON")?) };
    Ok(Some((method, path.to_string(), body)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_raw_requests() {
        let (method, path, body) = parse_raw_request("post /idx/_doc\n{\"a\": 1}").unwrap().unwrap();
        assert_eq!((method, path.as_str()), (Method::POST, "/idx/_doc"));
        assert_eq!(body.unwrap()["a"], 1);
        assert!(parse_raw_request("SELECT * FROM idx").unwrap().is_none());
        assert!(parse_raw_request("GET /x\nnot json").is_err());
    }
}
