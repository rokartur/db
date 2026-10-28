//! Each case: connect, set up one record, find it in the tree, browse it, and check that a broken
//! query is an error. URLs come from env (see scripts/live-env.sh); a missing one is reported as
//! skipped, and DBTUI_LIVE_STRICT=1 turns skips into failures.

use dbtui::driver::{Driver, connect};
use std::time::{Duration, Instant};

struct Case {
    env: &'static str,
    /// Setup steps run in `group`; a leading `?` lets the step fail (drop-if-exists on engines without it).
    setup: &'static [&'static str],
    group: &'static str,
    item: &'static str,
    /// An extra query that must return a cell equal to `EXPECTED`.
    check: Option<&'static str>,
    bad: &'static str,
}

const EXPECTED: &str = "live-ok";
const READY_TIMEOUT: Duration = Duration::from_secs(240);

const SQL_SETUP: &[&str] = &[
    "?DROP TABLE dbtui_t",
    "CREATE TABLE dbtui_t (id INT, name VARCHAR(20))",
    "INSERT INTO dbtui_t VALUES (1, 'live-ok')",
];

fn sql(env: &'static str, group: &'static str) -> Case {
    Case { env, setup: SQL_SETUP, group, item: "dbtui_t", check: None, bad: "SELEC 1" }
}

async fn run(case: Case) {
    let Ok(url) = std::env::var(case.env) else {
        if std::env::var("DBTUI_LIVE_STRICT").is_ok_and(|v| v == "1") {
            panic!("{} is not set", case.env);
        }
        eprintln!("SKIPPED: {} is not set", case.env);
        return;
    };
    run_url(&url, case).await;
}

async fn run_url(url: &str, case: Case) {
    let db = ready(url, &case).await;

    let groups = db.groups().await.expect("groups");
    let group = groups.iter().find(|g| g.name == case.group);
    let items = group.map(|g| g.items.clone()).unwrap_or_default();
    assert!(items.iter().any(|i| i == case.item), "{}: `{}` not in group `{}`: {groups:?}", case.env, case.item, case.group);

    let grid = db.browse(case.group, case.item, 0).await.expect("browse");
    assert!(contains(&grid.rows, EXPECTED), "{}: browse did not return {EXPECTED}: {grid:?}", case.env);

    if let Some(check) = case.check {
        let grid = db.query(case.group, check).await.expect("check query");
        assert!(contains(&grid.rows, EXPECTED), "{}: `{check}` did not return {EXPECTED}: {grid:?}", case.env);
    }

    assert!(db.query(case.group, case.bad).await.is_err(), "{}: `{}` should fail", case.env, case.bad);
}

/// Containers accept connections before they accept work, so connect and setup are retried together.
async fn ready(url: &str, case: &Case) -> Box<dyn Driver> {
    let started = Instant::now();
    loop {
        let attempt = async {
            let db = connect(url).await?;
            for step in case.setup {
                match step.strip_prefix('?') {
                    Some(optional) => drop(db.query(case.group, optional).await),
                    None => {
                        db.query(case.group, step).await?;
                    }
                }
            }
            anyhow::Ok(db)
        };
        match attempt.await {
            Ok(db) => return db,
            Err(error) if started.elapsed() < READY_TIMEOUT => {
                eprintln!("{}: not ready: {error:#}", case.env);
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
            Err(error) => panic!("{}: {error:#}", case.env),
        }
    }
}

fn contains(rows: &[Vec<String>], value: &str) -> bool {
    rows.iter().flatten().any(|cell| cell == value)
}

#[tokio::test]
async fn postgres() {
    run(sql("DBTUI_POSTGRES", "public")).await;
}

#[tokio::test]
async fn cockroach() {
    run(sql("DBTUI_COCKROACH", "public")).await;
}

#[tokio::test]
async fn pglite() {
    run(sql("DBTUI_PGLITE", "public")).await;
}

#[tokio::test]
async fn redshift() {
    run(sql("DBTUI_REDSHIFT", "public")).await;
}

#[tokio::test]
async fn mysql() {
    run(sql("DBTUI_MYSQL", "test")).await;
}

#[tokio::test]
async fn mariadb() {
    run(sql("DBTUI_MARIADB", "test")).await;
}

#[tokio::test]
async fn tidb() {
    run(sql("DBTUI_TIDB", "test")).await;
}

#[tokio::test]
async fn oceanbase() {
    run(sql("DBTUI_OCEANBASE", "test")).await;
}

#[tokio::test]
async fn databend() {
    run(sql("DBTUI_DATABEND", "default")).await;
}

#[tokio::test]
async fn mssql() {
    run(sql("DBTUI_MSSQL", "dbo")).await;
}

#[tokio::test]
async fn libsql() {
    run(sql("DBTUI_LIBSQL", "main")).await;
}

#[tokio::test]
async fn turso() {
    run(sql("DBTUI_TURSO", "main")).await;
}

#[tokio::test]
async fn d1() {
    run(sql("DBTUI_D1", "main")).await;
}

#[tokio::test]
async fn snowflake() {
    run(Case { item: "DBTUI_T", ..sql("DBTUI_SNOWFLAKE", "PUBLIC") }).await;
}

#[tokio::test]
async fn odbc() {
    run(sql("DBTUI_ODBC", "")).await;
}

#[tokio::test]
async fn clickhouse() {
    run(Case {
        // The name `dbtui\"t` breaks quoting that only doubles quotes, since ClickHouse also reads `\"`.
        setup: &[
            r#"DROP TABLE IF EXISTS "dbtui\\\"t""#,
            r#"CREATE TABLE "dbtui\\\"t" (id Int32, name String) ENGINE = MergeTree ORDER BY id"#,
            r#"INSERT INTO "dbtui\\\"t" VALUES (1, 'live-ok')"#,
        ],
        item: r#"dbtui\"t"#,
        ..sql("DBTUI_CLICKHOUSE", "default")
    })
    .await;
}

#[tokio::test]
async fn trino() {
    run(sql("DBTUI_TRINO", "default")).await;
}

#[tokio::test]
async fn bigquery() {
    run(Case {
        setup: &["?DROP TABLE ds.dbtui_t", "CREATE TABLE ds.dbtui_t (id INT64, name STRING)", "INSERT INTO ds.dbtui_t VALUES (1, 'live-ok')"],
        ..sql("DBTUI_BIGQUERY", "ds")
    })
    .await;
}

#[tokio::test]
async fn spanner() {
    if let Ok(url) = std::env::var("DBTUI_SPANNER") {
        create_spanner_database(&url).await;
    }
    run(Case {
        setup: &[
            "?DROP TABLE dbtui_t",
            "CREATE TABLE dbtui_t (id INT64, name STRING(20)) PRIMARY KEY (id)",
            "INSERT INTO dbtui_t (id, name) VALUES (1, 'live-ok')",
        ],
        ..sql("DBTUI_SPANNER", "")
    })
    .await;
}

/// The emulator starts empty: instance `inst` and database `db` are created through its REST API.
async fn create_spanner_database(url: &str) {
    let endpoint = url.split("endpoint=").nth(1).expect("spanner URL with endpoint=");
    let client = reqwest::Client::new();
    let instance = serde_json::json!({"instanceId": "inst", "instance": {"config": "emulator-config", "displayName": "dbtui", "nodeCount": 1}});
    let database = serde_json::json!({"createStatement": "CREATE DATABASE `db`"});
    let started = Instant::now();
    loop {
        let created = client.post(format!("{endpoint}/v1/projects/test/instances")).json(&instance).send().await;
        if created.is_ok() || started.elapsed() > READY_TIMEOUT {
            break;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    let _ = client.post(format!("{endpoint}/v1/projects/test/instances/inst/databases")).json(&database).send().await;
}

#[tokio::test]
async fn mongodb() {
    run(Case {
        env: "DBTUI_MONGODB",
        setup: &["?{\"drop\": \"dbtui_t\"}", "{\"insert\": \"dbtui_t\", \"documents\": [{\"name\": \"live-ok\"}]}"],
        group: "dbtui",
        item: "dbtui_t",
        check: Some("{\"find\": \"dbtui_t\", \"filter\": {\"name\": \"live-ok\"}}"),
        bad: "not json",
    })
    .await;
}

#[tokio::test]
async fn surrealdb() {
    run(Case {
        env: "DBTUI_SURREALDB",
        setup: &[
            "DEFINE NAMESPACE IF NOT EXISTS test",
            "DEFINE DATABASE IF NOT EXISTS test",
            "?DELETE dbtui_t",
            "CREATE dbtui_t SET name = 'live-ok'",
        ],
        group: "test",
        item: "dbtui_t",
        check: Some("SELECT name FROM dbtui_t"),
        bad: "SELEC FROM",
    })
    .await;
}

#[tokio::test]
async fn redis() {
    run(Case { env: "DBTUI_REDIS", setup: &["SET dbtui_k live-ok"], group: "db0", item: "dbtui_k", check: Some("GET dbtui_k"), bad: "NOSUCHCOMMAND" }).await;
}

#[tokio::test]
async fn dynamodb() {
    run(Case {
        env: "DBTUI_DYNAMODB",
        setup: &[
            "?DeleteTable {\"TableName\": \"dbtui_t\"}",
            "CreateTable {\"TableName\": \"dbtui_t\", \"AttributeDefinitions\": [{\"AttributeName\": \"id\", \"AttributeType\": \"S\"}], \
             \"KeySchema\": [{\"AttributeName\": \"id\", \"KeyType\": \"HASH\"}], \"BillingMode\": \"PAY_PER_REQUEST\"}",
            "INSERT INTO \"dbtui_t\" VALUE {'id': 'a', 'name': 'live-ok'}",
        ],
        group: "tables",
        item: "dbtui_t",
        check: Some("SELECT * FROM \"dbtui_t\" WHERE id = 'a'"),
        bad: "SELEC * FROM",
    })
    .await;
}

const CQL_SETUP: &[&str] = &[
    "CREATE KEYSPACE IF NOT EXISTS dbtui WITH replication = {'class': 'NetworkTopologyStrategy', 'replication_factor': 1}",
    "CREATE TABLE IF NOT EXISTS dbtui.dbtui_t (id int PRIMARY KEY, name text)",
    "INSERT INTO dbtui.dbtui_t (id, name) VALUES (1, 'live-ok')",
];

#[tokio::test]
async fn cassandra() {
    run(Case { env: "DBTUI_CASSANDRA", setup: CQL_SETUP, group: "dbtui", item: "dbtui_t", check: None, bad: "SELEC 1" }).await;
}

#[tokio::test]
async fn scylla() {
    run(Case { env: "DBTUI_SCYLLA", setup: CQL_SETUP, group: "dbtui", item: "dbtui_t", check: None, bad: "SELEC 1" }).await;
}

#[tokio::test]
async fn elasticsearch() {
    run(Case {
        env: "DBTUI_ELASTICSEARCH",
        setup: &["PUT /dbtui_t/_doc/1?refresh=true\n{\"name\": \"live-ok\"}"],
        group: "indices",
        item: "dbtui_t",
        check: Some("SELECT name FROM dbtui_t"),
        bad: "SELEC name FROM",
    })
    .await;
}

#[tokio::test]
async fn typesense() {
    run(Case {
        env: "DBTUI_TYPESENSE",
        setup: &[
            "?DELETE /collections/dbtui_t",
            "POST /collections\n{\"name\": \"dbtui_t\", \"fields\": [{\"name\": \"name\", \"type\": \"string\"}]}",
            "POST /collections/dbtui_t/documents\n{\"id\": \"1\", \"name\": \"live-ok\"}",
        ],
        group: "collections",
        item: "dbtui_t",
        check: Some("GET /collections/dbtui_t/documents/1"),
        bad: "GET /no_such_path",
    })
    .await;
}

#[tokio::test]
async fn weaviate() {
    run(Case {
        env: "DBTUI_WEAVIATE",
        setup: &[
            "?DELETE /v1/schema/Dbtui",
            "POST /v1/schema\n{\"class\": \"Dbtui\", \"vectorizer\": \"none\", \"properties\": [{\"name\": \"name\", \"dataType\": [\"text\"]}]}",
            "POST /v1/objects\n{\"class\": \"Dbtui\", \"properties\": {\"name\": \"live-ok\"}}",
        ],
        group: "classes",
        item: "Dbtui",
        check: Some("POST /v1/graphql\n{\"query\": \"{ Get { Dbtui { name } } }\"}"),
        bad: "GET /v1/no_such_path",
    })
    .await;
}

#[tokio::test]
async fn kafka() {
    run(Case {
        env: "DBTUI_KAFKA",
        setup: &["?create dbtui_t", "produce dbtui_t live-ok"],
        group: "topics",
        item: "dbtui_t",
        check: Some("consume dbtui_t"),
        bad: "nonsense",
    })
    .await;
}

#[tokio::test]
async fn etcd() {
    run(Case { env: "DBTUI_ETCD", setup: &["put dbtui_k live-ok"], group: "keys", item: "dbtui_k", check: Some("get dbtui_k"), bad: "nonsense" }).await;
    if let Ok(url) = std::env::var("DBTUI_ETCD") {
        let db = connect(&url).await.unwrap();
        assert_eq!(db.query("", "del dbtui_k").await.unwrap().rows, [["1 deleted"]]);
        assert!(db.query("", "get dbtui_k").await.unwrap().rows.is_empty());
    }
}

#[tokio::test]
async fn sqlite_file() {
    let path = std::env::temp_dir().join(format!("dbtui-live-{}.db", std::process::id()));
    run_url(&format!("sqlite://{}", path.display()), sql("sqlite file", "main")).await;
    std::fs::remove_file(path).ok();
}

#[tokio::test]
async fn duckdb_file() {
    let path = std::env::temp_dir().join(format!("dbtui-live-{}.duckdb", std::process::id()));
    run_url(&format!("duckdb://{}", path.display()), sql("duckdb file", "main")).await;
    std::fs::remove_file(path).ok();
}

#[tokio::test]
async fn beancount_fixture() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/ledger.beancount");
    let db = connect(path).await.unwrap();
    assert!(db.groups().await.unwrap()[0].items.contains(&"postings".to_string()));
    let grid = db.browse("ledger", "accounts", 0).await.unwrap();
    assert!(contains(&grid.rows, "Assets:Bank"));
    assert!(db.query("ledger", "SELECT FROM").await.is_err());
}
