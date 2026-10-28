# Etap 3: pliki i rodzina SQLite

## sqlite.rs (SQLite)
- `rusqlite` z `bundled`; `Mutex<Connection>`.
- `groups`: jedna grupa `main` z `sqlite_master` (tabele i widoki).

## duckdb.rs (DuckDB)
- `duckdb` z `bundled` (najcięższa kompilacja, build na Studio).
- `groups`: domyślny z information_schema.

## libsql.rs (libSQL, Turso)
- Hrana przez HTTP: `POST /v2/pipeline` z `execute` + `close`; `cols[].name`, `rows[][]` jako `{type, value}`.
- `libsql://host` -> `https://host`; `?tls=0` -> `http://`; `authToken` jako Bearer.
- `groups` z `sqlite_master`.

## d1.rs (Cloudflare D1)
- `POST https://api.cloudflare.com/client/v4/accounts/{a}/d1/database/{db}/raw` z `{sql}`; `result[last].results.columns/rows`.
- Błędy z `errors[].message` do paska statusu.

## Wspólne
`http.rs`: jeden `reqwest::Client` (rustls), `json_cell(Value) -> String` (null -> `NULL`, napis bez cudzysłowów, reszta jako JSON).

## Gotowe, gdy
- live: sqlite (plik tymczasowy), duckdb (plik tymczasowy), libsql (sqld w dockerze).
- fixture: odpowiedź D1 z dokumentacji parsuje się do Grid.
