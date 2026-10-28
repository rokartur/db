use super::{Driver, Grid, Group, PAGE};
use anyhow::{Result, bail};
use async_trait::async_trait;
use redis::aio::MultiplexedConnection;
use redis::{Value, cmd};

const MAX_KEYS: usize = 1000;

/// Redis: `redis://host:6379/0`; the tree shows the keys of the database in the URL.
pub struct Redis {
    conn: MultiplexedConnection,
    group: String,
}

impl Redis {
    pub async fn connect(url: &str) -> Result<Self> {
        let client = redis::Client::open(url)?;
        let group = format!("db{}", client.get_connection_info().redis_settings().db());
        Ok(Redis { conn: client.get_multiplexed_async_connection().await?, group })
    }
}

#[async_trait]
impl Driver for Redis {
    async fn query(&self, _group: &str, text: &str) -> Result<Grid> {
        let words = split_command(text)?;
        let Some((name, args)) = words.split_first() else { bail!("empty command") };
        let value: Value = cmd(name).arg(args).query_async(&mut self.conn.clone()).await?;
        Ok(value_to_grid(&value))
    }

    /// shortcut: shows the first 1000 keys of SCAN; use `SCAN` / `KEYS` in Query for more.
    async fn groups(&self) -> Result<Vec<Group>> {
        let mut conn = self.conn.clone();
        let mut keys = Vec::new();
        let mut cursor = 0u64;
        loop {
            let (next, batch): (u64, Vec<String>) = cmd("SCAN").arg(cursor).arg("COUNT").arg(MAX_KEYS).query_async(&mut conn).await?;
            keys.extend(batch);
            cursor = next;
            if cursor == 0 || keys.len() >= MAX_KEYS {
                break;
            }
        }
        keys.truncate(MAX_KEYS);
        keys.sort();
        Ok(vec![Group { name: self.group.clone(), items: keys }])
    }

    async fn browse(&self, _group: &str, key: &str, page: usize) -> Result<Grid> {
        let mut conn = self.conn.clone();
        let (start, stop) = (page * PAGE, page * PAGE + PAGE - 1);
        let kind: String = cmd("TYPE").arg(key).query_async(&mut conn).await?;
        let grid = match kind.as_str() {
            "string" => {
                let value: Value = cmd("GET").arg(key).query_async(&mut conn).await?;
                Grid { columns: vec!["value".into()], rows: vec![vec![text(&value)]] }
            }
            "hash" => {
                let pairs: Vec<(String, String)> = cmd("HGETALL").arg(key).query_async(&mut conn).await?;
                let rows = pairs.into_iter().skip(start).take(PAGE).map(|(f, v)| vec![f, v]).collect();
                Grid { columns: vec!["field".into(), "value".into()], rows }
            }
            "list" => {
                let items: Vec<String> = cmd("LRANGE").arg(key).arg(start).arg(stop).query_async(&mut conn).await?;
                let rows = items.into_iter().enumerate().map(|(i, v)| vec![(start + i).to_string(), v]).collect();
                Grid { columns: vec!["index".into(), "value".into()], rows }
            }
            "set" => {
                // shortcut: SMEMBERS loads the whole set to page it; SSCAN cursors cannot jump to a page.
                let mut members: Vec<String> = cmd("SMEMBERS").arg(key).query_async(&mut conn).await?;
                members.sort();
                Grid { columns: vec!["member".into()], rows: members.into_iter().skip(start).take(PAGE).map(|m| vec![m]).collect() }
            }
            "zset" => {
                let items: Vec<(String, f64)> =
                    cmd("ZRANGE").arg(key).arg(start).arg(stop).arg("WITHSCORES").query_async(&mut conn).await?;
                Grid { columns: vec!["member".into(), "score".into()], rows: items.into_iter().map(|(m, s)| vec![m, s.to_string()]).collect() }
            }
            "stream" => {
                let entries: Value = cmd("XRANGE").arg(key).arg("-").arg("+").arg("COUNT").arg(stop + 1).query_async(&mut conn).await?;
                let Value::Array(entries) = entries else { bail!("unexpected XRANGE reply") };
                let rows = entries.iter().skip(start).map(stream_entry).collect();
                Grid { columns: vec!["id".into(), "fields".into()], rows }
            }
            "none" => bail!("key `{key}` does not exist"),
            other => Grid::message(format!("{other} key: inspect it with a command in Query")),
        };
        Ok(grid)
    }
}

fn stream_entry(entry: &Value) -> Vec<String> {
    match entry {
        Value::Array(parts) if parts.len() == 2 => vec![text(&parts[0]), text(&parts[1])],
        other => vec![text(other), String::new()],
    }
}

fn value_to_grid(value: &Value) -> Grid {
    match value {
        Value::Array(items) | Value::Set(items) | Value::Push { data: items, .. } => Grid {
            columns: vec!["#".into(), "value".into()],
            rows: items.iter().enumerate().map(|(i, v)| vec![(i + 1).to_string(), text(v)]).collect(),
        },
        Value::Map(pairs) => {
            Grid { columns: vec!["key".into(), "value".into()], rows: pairs.iter().map(|(k, v)| vec![text(k), text(v)]).collect() }
        }
        Value::Attribute { data, .. } => value_to_grid(data),
        scalar => Grid { columns: vec!["value".into()], rows: vec![vec![text(scalar)]] },
    }
}

fn text(value: &Value) -> String {
    match value {
        Value::Nil => "NULL".into(),
        Value::Int(n) => n.to_string(),
        Value::BulkString(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        Value::SimpleString(s) => s.clone(),
        Value::Okay => "OK".into(),
        Value::Double(x) => x.to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::VerbatimString { text, .. } => text.clone(),
        Value::Array(items) | Value::Set(items) | Value::Push { data: items, .. } => {
            format!("[{}]", items.iter().map(text).collect::<Vec<_>>().join(", "))
        }
        Value::Map(pairs) => format!("{{{}}}", pairs.iter().map(|(k, v)| format!("{}: {}", text(k), text(v))).collect::<Vec<_>>().join(", ")),
        Value::Attribute { data, .. } => text(data),
        other => format!("{other:?}"),
    }
}

/// Splits `SET "a key" "x \"y\""` into words; double quotes group, backslash escapes inside them.
fn split_command(line: &str) -> Result<Vec<String>> {
    let mut words = Vec::new();
    let mut chars = line.trim().chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }
        let mut word = String::new();
        if c == '"' {
            chars.next();
            loop {
                match chars.next() {
                    Some('"') => break,
                    Some('\\') => word.extend(chars.next()),
                    Some(other) => word.push(other),
                    None => bail!("unclosed quote"),
                }
            }
        } else {
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() {
                    break;
                }
                word.push(c);
                chars.next();
            }
        }
        words.push(word);
    }
    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_quoted_words() {
        assert_eq!(split_command(r#"SET "a key"  "x \"y\"" 3"#).unwrap(), ["SET", "a key", "x \"y\"", "3"]);
        assert!(split_command("GET \"open").is_err());
    }

    #[test]
    fn nested_replies_become_rows() {
        let reply = Value::Array(vec![Value::BulkString(b"a".to_vec()), Value::Array(vec![Value::Int(1), Value::Nil])]);
        assert_eq!(value_to_grid(&reply).rows, [["1", "a"], ["2", "[1, NULL]"]]);
    }
}
