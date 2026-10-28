use super::http::{client, send};
use super::{Driver, Grid, Group, param, parse_url, password, user};
use anyhow::{Result, bail};
use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};

const MAX_KEYS: usize = 1000;
const HELP: &str = "commands: get <key> | get --prefix <prefix> | put <key> <value> | del <key>";

/// etcd v3 through its JSON gateway (no gRPC): `etcd://host:2379`, `?tls=1` for https.
pub struct Etcd {
    base: String,
    auth: Option<(String, String)>,
}

impl Etcd {
    pub fn new(url: &str) -> Result<Self> {
        let parsed = parse_url(url)?;
        let scheme = if param(&parsed, "tls").as_deref() == Some("1") { "https" } else { "http" };
        let host = parsed.host_str().unwrap_or("localhost");
        let port = parsed.port().unwrap_or(2379);
        let auth = user(&parsed).map(|u| (u, password(&parsed).unwrap_or_default()));
        Ok(Etcd { base: format!("{scheme}://{host}:{port}/v3"), auth })
    }

    async fn call(&self, path: &str, body: Value) -> Result<Value> {
        let mut request = client().post(format!("{}{path}", self.base)).json(&body);
        if let Some((name, password)) = &self.auth {
            let token = send(client().post(format!("{}/auth/authenticate", self.base)).json(&json!({"name": name, "password": password}))).await?;
            request = request.header("Authorization", token["token"].as_str().unwrap_or_default());
        }
        send(request).await
    }

    async fn range(&self, key: &str, prefix: bool) -> Result<Grid> {
        let mut body = json!({"key": STANDARD.encode(key)});
        if prefix {
            body["range_end"] = json!(STANDARD.encode(prefix_end(key.as_bytes())));
        }
        let response = self.call("/kv/range", body).await?;
        let columns = ["key", "value", "create_revision", "mod_revision", "version"].map(String::from).to_vec();
        let rows = response["kvs"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|kv| {
                vec![
                    decode(&kv["key"]),
                    decode(&kv["value"]),
                    revision(&kv["create_revision"]),
                    revision(&kv["mod_revision"]),
                    revision(&kv["version"]),
                ]
            })
            .collect();
        Ok(Grid { columns, rows })
    }
}

#[async_trait]
impl Driver for Etcd {
    async fn query(&self, _group: &str, text: &str) -> Result<Grid> {
        let words: Vec<&str> = text.split_whitespace().collect();
        match words.as_slice() {
            ["get", "--prefix", prefix] => self.range(prefix, true).await,
            ["get", key] => self.range(key, false).await,
            ["put", key, _, ..] => {
                let value = super::after_words(text, 2);
                self.call("/kv/put", json!({"key": STANDARD.encode(key), "value": STANDARD.encode(value)})).await?;
                Ok(Grid::message("OK"))
            }
            ["del", key] => {
                let response = self.call("/kv/deleterange", json!({"key": STANDARD.encode(key)})).await?;
                Ok(Grid::message(format!("{} deleted", revision(&response["deleted"]))))
            }
            _ => bail!("{HELP}"),
        }
    }

    /// shortcut: the first 1000 keys; `get --prefix` in Query reaches the rest.
    async fn groups(&self) -> Result<Vec<Group>> {
        let body = json!({"key": STANDARD.encode([0u8]), "range_end": STANDARD.encode([0u8]), "keys_only": true, "limit": MAX_KEYS, "sort_order": "ASCEND"});
        let response = self.call("/kv/range", body).await?;
        let keys = response["kvs"].as_array().into_iter().flatten().map(|kv| decode(&kv["key"])).collect();
        Ok(vec![Group { name: "keys".into(), items: keys }])
    }

    async fn browse(&self, _group: &str, key: &str, page: usize) -> Result<Grid> {
        if page > 0 {
            return Ok(Grid::default());
        }
        self.range(key, false).await
    }
}

/// The smallest key greater than every key with this prefix, as etcd's `--prefix` computes it.
fn prefix_end(prefix: &[u8]) -> Vec<u8> {
    let mut end = prefix.to_vec();
    while let Some(last) = end.pop() {
        if last < 0xff {
            end.push(last + 1);
            return end;
        }
    }
    vec![0]
}

fn decode(value: &Value) -> String {
    let encoded = value.as_str().unwrap_or_default();
    STANDARD.decode(encoded).map_or_else(|_| encoded.to_string(), |bytes| String::from_utf8_lossy(&bytes).into_owned())
}

/// The gateway encodes int64 as JSON strings and omits zero values.
fn revision(value: &Value) -> String {
    match value {
        Value::Null => "0".into(),
        other => super::http::json_cell(other),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn prefix_end_increments_last_byte() {
        assert_eq!(super::prefix_end(b"app/"), b"app0");
        assert_eq!(super::prefix_end(&[b'a', 0xff]), b"b");
        assert_eq!(super::prefix_end(&[0xff]), [0]);
    }
}
