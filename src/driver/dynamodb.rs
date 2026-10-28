use super::http::{client, json_cell, json_to_grid, objects_to_grid, send};
use super::{Driver, Grid, Group, PAGE, param, parse_url, password, user};
use anyhow::{Context, Result};
use async_trait::async_trait;
use hmac::{Hmac, KeyInit, Mac};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

/// DynamoDB JSON API with a hand-written SigV4: `dynamodb://key:secret@region?endpoint=http://localhost:8000`.
pub struct Dynamo {
    endpoint: String,
    host: String,
    region: String,
    key: String,
    secret: String,
}

impl Dynamo {
    pub fn new(url: &str) -> Result<Self> {
        let parsed = parse_url(url)?;
        let region = parsed.host_str().context("dynamodb URL: dynamodb://key:secret@region")?.to_string();
        let endpoint = param(&parsed, "endpoint").unwrap_or_else(|| format!("https://dynamodb.{region}.amazonaws.com"));
        let endpoint_url = parse_url(&endpoint)?;
        let host = match endpoint_url.port() {
            Some(port) => format!("{}:{port}", endpoint_url.host_str().unwrap_or_default()),
            None => endpoint_url.host_str().unwrap_or_default().to_string(),
        };
        let key = user(&parsed).or_else(|| std::env::var("AWS_ACCESS_KEY_ID").ok()).context("dynamodb needs an access key")?;
        let secret = password(&parsed).or_else(|| std::env::var("AWS_SECRET_ACCESS_KEY").ok()).context("dynamodb needs a secret key")?;
        Ok(Dynamo { endpoint, host, region, key, secret })
    }

    async fn call(&self, action: &str, body: Value) -> Result<Value> {
        let payload = body.to_string();
        let target = format!("DynamoDB_20120810.{action}");
        let now = chrono::Utc::now();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let headers = [
            ("content-type", "application/x-amz-json-1.0"),
            ("host", self.host.as_str()),
            ("x-amz-date", amz_date.as_str()),
            ("x-amz-target", target.as_str()),
        ];
        let scope = format!("{}/{}/dynamodb/aws4_request", &amz_date[..8], self.region);
        let canonical = canonical_request("POST", "/", "", &headers, &payload);
        let authorization = authorization(&self.key, &self.secret, &scope, &amz_date, &signed_headers(&headers), &canonical);
        let request = client()
            .post(&self.endpoint)
            .header("content-type", headers[0].1)
            .header("x-amz-date", &amz_date)
            .header("x-amz-target", &target)
            .header("authorization", authorization)
            .body(payload);
        send(request).await
    }
}

#[async_trait]
impl Driver for Dynamo {
    /// PartiQL (`SELECT * FROM "users" WHERE id = 'a'`) or a raw API call (`CreateTable {...}`).
    async fn query(&self, _group: &str, text: &str) -> Result<Grid> {
        if let Some((action, body)) = api_call(text) {
            return Ok(json_to_grid(&self.call(action, body).await?));
        }
        let response = self.call("ExecuteStatement", json!({"Statement": text})).await?;
        Ok(items_to_grid(&response["Items"]))
    }

    async fn groups(&self) -> Result<Vec<Group>> {
        let mut tables = Vec::new();
        let mut body = json!({});
        loop {
            let response = self.call("ListTables", body.clone()).await?;
            tables.extend(response["TableNames"].as_array().into_iter().flatten().map(json_cell));
            match response.get("LastEvaluatedTableName") {
                Some(last) => body = json!({"ExclusiveStartTableName": last}),
                None => break,
            }
        }
        Ok(vec![Group { name: "tables".into(), items: tables }])
    }

    /// Scan pages are walked from the start, since DynamoDB has no offset.
    async fn browse(&self, _group: &str, item: &str, page: usize) -> Result<Grid> {
        let mut body = json!({"TableName": item, "Limit": PAGE});
        for _ in 0..page {
            let response = self.call("Scan", body.clone()).await?;
            let Some(last) = response.get("LastEvaluatedKey") else { return Ok(Grid::default()) };
            body["ExclusiveStartKey"] = last.clone();
        }
        Ok(items_to_grid(&self.call("Scan", body).await?["Items"]))
    }
}

/// `Action {json}`: an action name followed by a JSON object.
fn api_call(text: &str) -> Option<(&str, Value)> {
    let (action, body) = text.trim().split_once(char::is_whitespace)?;
    if !action.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let body: Value = serde_json::from_str(body).ok()?;
    body.is_object().then_some((action, body))
}

fn items_to_grid(items: &Value) -> Grid {
    let objects: Vec<Value> = items.as_array().into_iter().flatten().map(attribute_map).collect();
    if objects.is_empty() {
        return Grid::message("0 items");
    }
    objects_to_grid(&objects)
}

fn attribute_map(item: &Value) -> Value {
    let mut map = Map::new();
    for (name, attribute) in item.as_object().into_iter().flatten() {
        map.insert(name.clone(), attribute_value(attribute));
    }
    Value::Object(map)
}

/// `{"S": "x"}` -> "x", `{"N": "1"}` -> 1, `{"M": {...}}` -> object, and so on.
fn attribute_value(attribute: &Value) -> Value {
    let Some((kind, value)) = attribute.as_object().and_then(|o| o.iter().next()) else { return Value::Null };
    match kind.as_str() {
        "N" => value.as_str().and_then(|n| serde_json::from_str(n).ok()).unwrap_or_else(|| value.clone()),
        "NULL" => Value::Null,
        "M" => attribute_map(value),
        "L" => Value::Array(value.as_array().into_iter().flatten().map(attribute_value).collect()),
        _ => value.clone(),
    }
}

fn hex_sha256(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

fn hmac(key: &[u8], data: &str) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC takes any key length");
    mac.update(data.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

/// Headers must be lowercase and sorted by name.
fn canonical_request(method: &str, path: &str, query: &str, headers: &[(&str, &str)], payload: &str) -> String {
    let canonical_headers: String = headers.iter().map(|(name, value)| format!("{name}:{}\n", value.trim())).collect();
    let signed = signed_headers(headers);
    format!("{method}\n{path}\n{query}\n{canonical_headers}\n{signed}\n{}", hex_sha256(payload.as_bytes()))
}

fn signed_headers(headers: &[(&str, &str)]) -> String {
    headers.iter().map(|(name, _)| *name).collect::<Vec<_>>().join(";")
}

fn signing_key(secret: &str, scope: &str) -> Vec<u8> {
    let mut key = format!("AWS4{secret}").into_bytes();
    for part in scope.split('/') {
        key = hmac(&key, part);
    }
    key
}

fn authorization(access_key: &str, secret: &str, scope: &str, amz_date: &str, signed: &str, canonical: &str) -> String {
    let string_to_sign = format!("AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}", hex_sha256(canonical.as_bytes()));
    let signature = hex::encode(hmac(&signing_key(secret, scope), &string_to_sign));
    format!("AWS4-HMAC-SHA256 Credential={access_key}/{scope}, SignedHeaders={signed}, Signature={signature}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY";

    #[test]
    fn signing_key_matches_aws_example() {
        let key = signing_key(SECRET, "20150830/us-east-1/iam/aws4_request");
        assert_eq!(hex::encode(key), "c4afb1cc5771d871763a393e44b703571b55cc28424d1a5e86da6ed3c154a4b9");
    }

    #[test]
    fn get_vanilla_from_aws_test_suite() {
        let headers = [("host", "example.amazonaws.com"), ("x-amz-date", "20150830T123600Z")];
        let canonical = canonical_request("GET", "/", "", &headers, "");
        let header = authorization("AKIDEXAMPLE", SECRET, "20150830/us-east-1/service/aws4_request", "20150830T123600Z", &signed_headers(&headers), &canonical);
        assert_eq!(
            header,
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, \
             SignedHeaders=host;x-amz-date, Signature=5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31"
        );
    }

    #[test]
    fn recognizes_api_calls() {
        assert_eq!(api_call("ListTables {}").unwrap().0, "ListTables");
        assert!(api_call("SELECT * FROM \"t\"").is_none());
    }

    #[test]
    fn attribute_values_become_plain_json() {
        let items = json!([{"id": {"S": "a"}, "n": {"N": "1.5"}, "tags": {"L": [{"S": "x"}]}, "m": {"M": {"ok": {"BOOL": true}}}, "z": {"NULL": true}}]);
        let grid = items_to_grid(&items);
        assert_eq!(grid.columns, ["id", "n", "tags", "m.ok", "z"]);
        assert_eq!(grid.rows, [["a", "1.5", "[\"x\"]", "true", "NULL"]]);
    }
}
