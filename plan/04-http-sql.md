# Etap 4: hurtownie przez HTTP

Każdy driver: `query` = jedno żądanie HTTP (plus polling, gdy API go wymaga) -> Grid.

| Plik | API | Drzewo | Uwagi |
|---|---|---|---|
| clickhouse.rs | `POST /?database=g` treść + `FORMAT JSONCompact` -> `meta[].name`, `data` | `system.tables` | Basic auth z URL |
| trino.rs | `POST /v1/statement`, potem `GET nextUri` aż zniknie; `columns`, `data` | `information_schema.tables` per katalog | nagłówki `X-Trino-User`, `X-Trino-Catalog`, `X-Trino-Schema` |
| bigquery.rs | `POST /bigquery/v2/projects/{p}/queries`, `useLegacySql:false`; `schema.fields`, `rows[].f[].v` | `datasets` + `tables` (REST list) | token z URL albo `GOOGLE_OAUTH_TOKEN`; `endpoint` dla emulatora |
| snowflake.rs | `POST https://{account}.snowflakecomputing.com/api/v2/statements`; `resultSetMetaData.rowType`, `data` | `information_schema.tables` | Bearer + `X-Snowflake-Authorization-Token-Type`; tylko pierwsza partycja (shortcut) |
| spanner.rs | `POST .../sessions` raz, potem `sessions/{s}:executeSql`; `metadata.rowType.fields`, `rows` | `information_schema.tables` | `endpoint` dla emulatora (REST :9020); DML przez `executeSql` z transakcją `begin` |
| r2sql.rs | `POST https://api.sql.cloudflarestorage.com/api/v1/accounts/{a}/basin-sql/query/{bucket}` z `{query}` | `SHOW DATABASES`, `SHOW TABLES IN ns` | format odpowiedzi nieudokumentowany: ogólne JSON na Grid |

`http.rs` dostaje `json_to_grid(Value) -> Grid`: tablica obiektów -> kolumny z kluczy (zagnieżdżone spłaszczone kropką), tablica tablic -> kolumny `0..n`, skalar -> jedna komórka.
Używany przez R2 SQL, wyszukiwarki, Mongo, SurrealDB.

## Gotowe, gdy
- live: clickhouse, trino, bigquery-emulator, spanner-emulator.
- fixture: snowflake (przykład z dokumentacji SQL API), r2sql (odpowiedź w stylu `result.rows`).
