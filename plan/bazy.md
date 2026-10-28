# Mapa baz

"Rodzina" = plik w `src/driver/`. "Test": docker = kontener w `docker-compose.yml` + `tests/live.rs`;
fixture = test jednostkowy parsera na odpowiedzi z dokumentacji (brak emulatora).

## Relacyjne

| Baza | Rodzina | URL | Test |
|---|---|---|---|
| PostgreSQL | postgres (tokio-postgres, simple_query) | `postgres://u:p@h:5432/db` | docker `postgres:17` |
| MySQL | mysql (mysql_async) | `mysql://u:p@h:3306/db` | docker `mysql:8.4` |
| Microsoft SQL Server | mssql (tiberius) | `sqlserver://u:p@h:1433/db` | docker `mcr.microsoft.com/mssql/server:2022-latest` (amd64, Rosetta) |
| MariaDB | mysql | `mariadb://u:p@h:3306/db` | docker `mariadb:11` |
| TiDB | mysql | `tidb://root@h:4000/test` | docker `pingcap/tidb` |
| OceanBase | mysql | `oceanbase://root@h:2881/test` | docker `oceanbase/oceanbase-ce` (ciężki, opcjonalny) |
| CockroachDB | postgres | `cockroachdb://root@h:26257/defaultdb` | docker `cockroachdb/cockroach` single-node insecure |
| PGlite | postgres (przez pglite-socket) | `pglite://h:5432/postgres` | `npx @electric-sql/pglite-socket` |
| Oracle Database | odbc | `odbc:Driver=...;Dbq=...` | opcjonalnie `gvenzl/oracle-free` + Instant Client ODBC |
| Google Cloud Spanner | spanner (REST executeSql) | `spanner://proj/inst/db?token=&endpoint=` | docker emulator `gcr.io/cloud-spanner-emulator/emulator` |
| Dameng DM8 | odbc | `odbc:Driver=DM8 ODBC DRIVER;...` | brak obrazu ARM; ścieżka ODBC przez sqliteodbc |
| SAP HANA | odbc | `odbc:Driver=HDBODBC;...` | brak (HANA express: amd64, 16 GB RAM); ścieżka ODBC |

## Analityczne i hurtownie

| Baza | Rodzina | URL | Test |
|---|---|---|---|
| Databend | mysql (handler MySQL :3307) | `databend://u:p@h:3307/default` | docker `datafuselabs/databend` |
| Amazon Redshift | postgres + TLS | `redshift://u:p@cluster:5439/dev?sslmode=require` | brak emulatora; ta sama ścieżka co Postgres |
| ClickHouse | clickhouse (HTTP, `FORMAT JSONCompact`) | `clickhouse://u:p@h:8123/db` | docker `clickhouse/clickhouse-server` |
| Google BigQuery | bigquery (REST jobs.query) | `bigquery://project?token=&endpoint=` | docker `ghcr.io/goccy/bigquery-emulator` |
| Snowflake | snowflake (SQL API v2, PAT/OAuth/JWT bearer) | `snowflake://account?token=&warehouse=&database=` | fixture |
| Teradata | odbc | `odbc:Driver=Teradata;...` | brak; ścieżka ODBC |
| Trino | trino (`/v1/statement` + `nextUri`) | `trino://user@h:8080/catalog` | docker `trinodb/trino` |

## Chmurowe i serverless

| Baza | Rodzina | URL | Test |
|---|---|---|---|
| Cloudflare D1 | d1 (REST `/raw`: columns + rows) | `d1://account/database?token=` | fixture |
| Turso | libsql (Hrana HTTP `/v2/pipeline`) | `libsql://db-org.turso.io?authToken=` | docker sqld (ta sama ścieżka) |
| libSQL | libsql | `libsql://h:8080?tls=0` | docker `ghcr.io/tursodatabase/libsql-server` |
| Cloudflare R2 SQL | r2sql (Basin SQL `POST /api/v1/accounts/{a}/basin-sql/query/{bucket}`) | `r2sql://account/bucket?token=` | fixture; format odpowiedzi nieudokumentowany, ogólne JSON na Grid |

## Dokumentowe i wielomodelowe

| Baza | Rodzina | URL | Test |
|---|---|---|---|
| MongoDB | mongodb (zapytanie = komenda JSON przez runCommand) | `mongodb://u:p@h:27017` | docker `mongo:8` |
| SurrealDB | surreal (HTTP `POST /sql`) | `surrealdb://u:p@h:8000/ns/db` | docker `surrealdb/surrealdb` |

## Klucz-wartość

| Baza | Rodzina | URL | Test |
|---|---|---|---|
| Redis | redis (zapytanie = surowa komenda) | `redis://h:6379/0` | docker `redis:8` |
| Amazon DynamoDB | dynamodb (PartiQL ExecuteStatement, SigV4 ręcznie) | `dynamodb://key:secret@region?endpoint=` | docker `amazon/dynamodb-local` |

## Wide-column

| Baza | Rodzina | URL | Test |
|---|---|---|---|
| Apache Cassandra | cql (scylla crate) | `cassandra://u:p@h:9042` (`?single_node=1` za dockerem/port-forwardem) | docker `cassandra:5` |
| ScyllaDB | cql | `scylla://h:9042` | docker `scylladb/scylla --smp 1` |

## Wyszukiwanie i wektory

| Baza | Rodzina | URL | Test |
|---|---|---|---|
| Elasticsearch | search (SQL przez `/_sql` albo surowe `GET /path` + JSON) | `elasticsearch://h:9200` | docker `elasticsearch:8` (security off) |
| Typesense | search (surowe REST) | `typesense://apikey@h:8108` | docker `typesense/typesense` |
| Weaviate | search (surowe REST/GraphQL) | `weaviate://h:8080` | docker `semitechnologies/weaviate` |

## Streaming i koordynacja

| Baza | Rodzina | URL | Test |
|---|---|---|---|
| Apache Kafka | kafka (rskafka, czysty Rust) | `kafka://h:9092` | docker `apache/kafka` (KRaft) |
| etcd | etcd (gateway JSON `/v3/kv/*`, bez gRPC) | `etcd://h:2379` | docker `quay.io/coreos/etcd` |

## Pliki

| Baza | Rodzina | URL | Test |
|---|---|---|---|
| SQLite | sqlite (rusqlite bundled) | `sqlite://plik.db` albo `plik.db` | plik tymczasowy |
| DuckDB | duckdb (bundled) | `duckdb://plik.duckdb` albo `plik.duckdb` | plik tymczasowy |
| Beancount | beancount (własny parser + BQL) | `beancount://ledger.beancount` albo `*.beancount` | fixture ledger |

Wynik: 40 baz w 20 plikach driverów.
