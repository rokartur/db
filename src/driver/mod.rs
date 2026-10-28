mod bigquery;
mod clickhouse;
#[cfg(feature = "cql")]
mod cql;
mod d1;
#[cfg(feature = "duckdb")]
mod duckdb;
mod dynamodb;
mod etcd;
pub mod http;
#[cfg(feature = "kafka")]
mod kafka;
mod libsql;
#[cfg(feature = "mongodb")]
mod mongo;
mod mssql;
mod mysql;
#[cfg(feature = "odbc")]
mod odbc;
mod postgres;
mod r2sql;
mod redis;
mod search;
mod snowflake;
mod spanner;
mod sqlite;
mod surreal;
mod trino;

use anyhow::{Result, bail};
use async_trait::async_trait;
use percent_encoding::percent_decode_str;
use url::Url;

pub const PAGE: usize = 200;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Grid {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl Grid {
    pub fn message(text: impl Into<String>) -> Grid {
        Grid { columns: vec!["result".into()], rows: vec![vec![text.into()]] }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub name: String,
    pub items: Vec<String>,
}

const SYSTEM_SCHEMAS: &str = "'information_schema', 'INFORMATION_SCHEMA', 'pg_catalog', 'crdb_internal', \
     'pg_extension', 'mysql', 'sys', 'performance_schema', 'metrics_schema', 'oceanbase', 'system', 'SPANNER_SYS'";

#[async_trait]
pub trait Driver: Send + Sync {
    /// `group` is the context selected in the tree: a schema, a Mongo database, a Redis db number.
    async fn query(&self, group: &str, text: &str) -> Result<Grid>;

    async fn groups(&self) -> Result<Vec<Group>> {
        let sql = format!(
            "SELECT table_schema, table_name FROM information_schema.tables \
             WHERE table_schema NOT IN ({SYSTEM_SCHEMAS}) ORDER BY 1, 2"
        );
        Ok(group_rows(&self.query("", &sql).await?))
    }

    async fn browse(&self, group: &str, item: &str, page: usize) -> Result<Grid> {
        self.query(group, &browse_sql('"', group, item, page)).await
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Family {
    Postgres,
    Mysql,
    Mssql,
    Odbc,
    Spanner,
    Clickhouse,
    Trino,
    Bigquery,
    Snowflake,
    D1,
    Libsql,
    R2sql,
    Mongo,
    Surreal,
    Redis,
    Dynamo,
    Cql,
    Search,
    Kafka,
    Etcd,
    Sqlite,
    Duckdb,
    Beancount,
}

pub fn family(url: &str) -> Result<Family> {
    let Some(scheme) = scheme(url) else {
        return family_by_extension(url);
    };
    Ok(match scheme.to_ascii_lowercase().as_str() {
        "postgres" | "postgresql" | "cockroachdb" | "cockroach" | "redshift" | "pglite" => Family::Postgres,
        "mysql" | "mariadb" | "tidb" | "oceanbase" | "databend" => Family::Mysql,
        "sqlserver" | "mssql" => Family::Mssql,
        "odbc" => Family::Odbc,
        "spanner" => Family::Spanner,
        "clickhouse" => Family::Clickhouse,
        "trino" => Family::Trino,
        "bigquery" => Family::Bigquery,
        "snowflake" => Family::Snowflake,
        "d1" => Family::D1,
        "libsql" | "turso" => Family::Libsql,
        "r2sql" => Family::R2sql,
        "mongodb" | "mongodb+srv" => Family::Mongo,
        "surrealdb" | "surreal" => Family::Surreal,
        "redis" | "rediss" => Family::Redis,
        "dynamodb" => Family::Dynamo,
        "cassandra" | "scylla" | "cql" => Family::Cql,
        "elasticsearch" | "typesense" | "weaviate" => Family::Search,
        "kafka" => Family::Kafka,
        "etcd" => Family::Etcd,
        "sqlite" => Family::Sqlite,
        "duckdb" => Family::Duckdb,
        "beancount" => Family::Beancount,
        other => bail!("unknown database scheme `{other}://`"),
    })
}

fn scheme(url: &str) -> Option<&str> {
    let (scheme, _) = url.split_once(':')?;
    let valid = !scheme.is_empty() && scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c));
    valid.then_some(scheme)
}

fn family_by_extension(path: &str) -> Result<Family> {
    let extension = path.rsplit_once('.').map(|(_, ext)| ext.to_ascii_lowercase());
    Ok(match extension.as_deref() {
        Some("db" | "sqlite" | "sqlite3") => Family::Sqlite,
        Some("duckdb") => Family::Duckdb,
        Some("beancount" | "bean") => Family::Beancount,
        _ => bail!("`{path}` is neither a URL nor a .db/.sqlite/.duckdb/.beancount file"),
    })
}

pub async fn connect(url: &str) -> Result<Box<dyn Driver>> {
    Ok(match family(url)? {
        Family::Postgres => Box::new(postgres::Postgres::connect(url).await?),
        Family::Mysql => Box::new(mysql::Mysql::connect(url).await?),
        Family::Mssql => Box::new(mssql::Mssql::connect(url).await?),
        #[cfg(feature = "odbc")]
        Family::Odbc => Box::new(odbc::Odbc::connect(url)?),
        Family::Spanner => Box::new(spanner::Spanner::connect(url).await?),
        Family::Clickhouse => Box::new(clickhouse::Clickhouse::new(url)?),
        Family::Trino => Box::new(trino::Trino::new(url)?),
        Family::Bigquery => Box::new(bigquery::Bigquery::new(url)?),
        Family::Snowflake => Box::new(snowflake::Snowflake::new(url)?),
        Family::D1 => Box::new(d1::D1::new(url)?),
        Family::Libsql => Box::new(libsql::Libsql::new(url)?),
        Family::R2sql => Box::new(r2sql::R2sql::new(url)?),
        #[cfg(feature = "mongodb")]
        Family::Mongo => Box::new(mongo::Mongo::connect(url).await?),
        Family::Surreal => Box::new(surreal::Surreal::new(url)?),
        Family::Redis => Box::new(redis::Redis::connect(url).await?),
        Family::Dynamo => Box::new(dynamodb::Dynamo::new(url)?),
        #[cfg(feature = "cql")]
        Family::Cql => Box::new(cql::Cql::connect(url).await?),
        Family::Search => Box::new(search::Search::new(url)?),
        #[cfg(feature = "kafka")]
        Family::Kafka => Box::new(kafka::Kafka::connect(url).await?),
        Family::Etcd => Box::new(etcd::Etcd::new(url)?),
        Family::Sqlite => Box::new(sqlite::Sqlite::open(file_path(url))?),
        #[cfg(feature = "duckdb")]
        Family::Duckdb => Box::new(duckdb::Duckdb::open(file_path(url))?),
        Family::Beancount => Box::new(crate::beancount::Beancount::open(file_path(url))?),
        #[allow(unreachable_patterns)]
        other => bail!("dbtui built without feature {}", feature_of(other)),
    })
}

fn feature_of(family: Family) -> &'static str {
    match family {
        Family::Odbc => "odbc",
        Family::Mongo => "mongodb",
        Family::Cql => "cql",
        Family::Kafka => "kafka",
        Family::Duckdb => "duckdb",
        _ => "?",
    }
}

/// `sqlite://a.db`, `sqlite:///abs/a.db` and plain `a.db` all name a file.
pub fn file_path(url: &str) -> &str {
    match url.split_once("://") {
        Some((_, path)) => path,
        None => url,
    }
}

pub fn parse_url(url: &str) -> Result<Url> {
    Ok(Url::parse(url)?)
}

pub fn decoded(text: &str) -> String {
    percent_decode_str(text).decode_utf8_lossy().into_owned()
}

pub fn user(url: &Url) -> Option<String> {
    (!url.username().is_empty()).then(|| decoded(url.username()))
}

pub fn password(url: &Url) -> Option<String> {
    url.password().map(decoded)
}

pub fn param(url: &Url, key: &str) -> Option<String> {
    url.query_pairs().find(|(k, _)| k == key).map(|(_, v)| v.into_owned())
}

pub fn path_segments(url: &Url) -> Vec<String> {
    url.path().split('/').filter(|s| !s.is_empty()).map(decoded).collect()
}

/// Values in a connection URL that must never reach the screen: passwords, bare-username API keys, tokens.
pub fn secrets_in(url: &str) -> Vec<String> {
    let mut secrets = Vec::new();
    if let Some(odbc) = url.strip_prefix("odbc:") {
        for part in odbc.split(';') {
            if let Some((key, value)) = part.split_once('=') {
                let key = key.trim().to_ascii_lowercase();
                if key == "pwd" || key == "password" {
                    secrets.push(value.trim().to_string());
                }
            }
        }
        return secrets;
    }
    let Ok(parsed) = Url::parse(url) else { return secrets };
    match password(&parsed) {
        Some(password) => secrets.push(password),
        None => secrets.extend(user(&parsed)),
    }
    for (key, value) in parsed.query_pairs() {
        let key = key.to_ascii_lowercase();
        if ["token", "key", "secret", "password", "auth"].iter().any(|word| key.contains(word)) {
            secrets.push(value.into_owned());
        }
    }
    secrets.retain(|secret| !secret.is_empty());
    secrets
}

pub fn redact(text: &str, secrets: &[String]) -> String {
    let mut text = text.to_string();
    for secret in secrets {
        text = text.replace(secret.as_str(), "***");
        let encoded = percent_encoding::utf8_percent_encode(secret, percent_encoding::NON_ALPHANUMERIC).to_string();
        text = text.replace(&encoded, "***");
    }
    text
}

pub fn quote_ident(name: &str, open: char) -> String {
    let close = if open == '[' { ']' } else { open };
    let escaped = name.replace(close, &format!("{close}{close}"));
    format!("{open}{escaped}{close}")
}

pub fn qualified(group: &str, item: &str, quote: char) -> String {
    if group.is_empty() {
        quote_ident(item, quote)
    } else {
        format!("{}.{}", quote_ident(group, quote), quote_ident(item, quote))
    }
}

pub fn browse_sql(quote: char, group: &str, item: &str, page: usize) -> String {
    format!("SELECT * FROM {} LIMIT {PAGE} OFFSET {}", qualified(group, item, quote), page * PAGE)
}

/// The text after the first `n` words, inner spacing kept: the value of `put key some value`.
pub fn after_words(text: &str, n: usize) -> &str {
    let mut rest = text.trim_start();
    for _ in 0..n {
        rest = rest.trim_start_matches(|c: char| !c.is_whitespace()).trim_start();
    }
    rest
}

/// Folds `(group, item)` rows, sorted by group, into the tree.
pub fn group_rows(grid: &Grid) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    for row in &grid.rows {
        let (name, item) = (&row[0], &row[1]);
        match groups.last_mut() {
            Some(group) if &group.name == name => group.items.push(item.clone()),
            _ => groups.push(Group { name: name.clone(), items: vec![item.clone()] }),
        }
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_map_to_families() {
        let cases = [
            ("postgres://u@h/db", Family::Postgres),
            ("cockroachdb://root@h:26257/defaultdb", Family::Postgres),
            ("redshift://u:p@c:5439/dev?sslmode=require", Family::Postgres),
            ("pglite://h:5432/postgres", Family::Postgres),
            ("mariadb://u:p@h/db", Family::Mysql),
            ("tidb://root@h:4000/test", Family::Mysql),
            ("oceanbase://root@h:2881/test", Family::Mysql),
            ("databend://u:p@h:3307/default", Family::Mysql),
            ("sqlserver://sa:p@h:1433/master", Family::Mssql),
            ("odbc:Driver=HDBODBC;ServerNode=h:30015", Family::Odbc),
            ("mongodb+srv://u:p@cluster", Family::Mongo),
            ("scylla://h:9042", Family::Cql),
            ("typesense://key@h:8108", Family::Search),
            ("libsql://db-org.turso.io?authToken=x", Family::Libsql),
            ("ledger.beancount", Family::Beancount),
            ("./data/app.db", Family::Sqlite),
            ("/tmp/x.duckdb", Family::Duckdb),
        ];
        for (url, expected) in cases {
            assert_eq!(family(url).unwrap(), expected, "{url}");
        }
    }

    #[tokio::test]
    async fn unknown_scheme_is_a_readable_error() {
        let error = connect("x://").await.err().unwrap().to_string();
        assert_eq!(error, "unknown database scheme `x://`");
        assert!(family("notes.txt").is_err());
    }

    #[test]
    fn group_rows_folds_consecutive_schemas() {
        let grid = Grid {
            columns: vec!["schema".into(), "table".into()],
            rows: vec![
                vec!["public".into(), "orders".into()],
                vec!["public".into(), "users".into()],
                vec!["stats".into(), "daily".into()],
            ],
        };
        assert_eq!(
            group_rows(&grid),
            vec![
                Group { name: "public".into(), items: vec!["orders".into(), "users".into()] },
                Group { name: "stats".into(), items: vec!["daily".into()] },
            ]
        );
    }

    #[test]
    fn identifiers_are_quoted_and_escaped() {
        assert_eq!(quote_ident("a\"b", '"'), "\"a\"\"b\"");
        assert_eq!(quote_ident("x]y", '['), "[x]]y]");
        assert_eq!(browse_sql('`', "", "t", 2), "SELECT * FROM `t` LIMIT 200 OFFSET 400");
    }

    #[test]
    fn after_words_keeps_inner_spacing() {
        assert_eq!(after_words("  put  key  a  b ", 2), "a  b ");
        assert_eq!(after_words("get", 2), "");
    }

    #[test]
    fn secrets_are_redacted() {
        let secrets = secrets_in("snowflake://acct?token=s3cr3t&warehouse=wh");
        assert_eq!(secrets, vec!["s3cr3t"]);
        assert_eq!(redact("bad token s3cr3t", &secrets), "bad token ***");
        assert_eq!(secrets_in("postgres://u:p%40ss@h/db"), vec!["p@ss"]);
        assert_eq!(secrets_in("typesense://apikey@h:8108"), vec!["apikey"]);
        assert_eq!(secrets_in("odbc:Driver=X;UID=a;PWD=hunter2"), vec!["hunter2"]);
    }
}
