# dbtui: plan

Cel: TUI w Rust (ratatui) w stylu gobang, która przegląda i odpytuje 40 baz z listy.

## Decyzje

| Pytanie | Wybór |
|---|---|
| Baza kodu | Nowa, mniejsza TUI w stylu gobang (nie fork) |
| Drivery | Rodziny protokołów + ODBC dla baz bez sensownego crate'a |
| Weryfikacja | Docker: kontener na bazę, testy integracyjne |
| Beancount | Własny parser + własny BQL |

## Architektura

```
Cargo.toml
src/main.rs          CLI: `dbtui <url>` albo `dbtui` (wybór z configu)
src/config.rs        ~/.config/dbtui/config.toml: [[connection]] name, url
src/app.rs           stan aplikacji + pętla zdarzeń
src/ui.rs            rysowanie (ratatui)
src/editor.rs        wieloliniowy edytor zapytań
src/driver/mod.rs    trait Driver, Grid, Group, connect(url) po schemacie URL
src/driver/*.rs      jeden plik na rodzinę protokołu
src/beancount/       parser.rs, bql.rs
tests/live.rs        testy na kontenerach (URL z env, brak env = test pominięty i wypisany)
docker-compose.yml   wszystkie bazy, które da się postawić lokalnie
```

Jeden kontrakt dla wszystkich baz. Wszystkie wartości wracają jako tekst:

```rust
pub struct Grid { pub columns: Vec<String>, pub rows: Vec<Vec<String>> }
pub struct Group { pub name: String, pub items: Vec<String> }

#[async_trait]
pub trait Driver: Send + Sync {
    /// `group` = kontekst: schemat, baza Mongo, numer db Redis, ...
    async fn query(&self, group: &str, text: &str) -> Result<Grid>;
    /// domyślnie: information_schema.tables bez schematów systemowych
    async fn groups(&self) -> Result<Vec<Group>>;
    /// domyślnie: SELECT * FROM "g"."t" LIMIT 200 OFFSET page*200
    async fn browse(&self, group: &str, item: &str, page: usize) -> Result<Grid>;
}
```

Drivery SQL implementują tylko `query` i nadpisują `groups`/`browse`, gdy dialekt jest inny.
Drivery nie-SQL implementują wszystkie trzy metody.

## Etapy

| # | Etap | Plik |
|---|---|---|
| 0 | Szkielet: projekt, trait, dispatch URL, config | [00-szkielet.md](00-szkielet.md) |
| 1 | UI w stylu gobang | [01-ui.md](01-ui.md) |
| 2 | SQL wire: Postgres, MySQL, TDS | [02-sql-wire.md](02-sql-wire.md) |
| 3 | Pliki i SQLite-podobne: SQLite, DuckDB, libSQL/Turso, D1 | [03-pliki-i-sqlite.md](03-pliki-i-sqlite.md) |
| 4 | Hurtownie przez HTTP: ClickHouse, Trino, BigQuery, Snowflake, Spanner, R2 SQL | [04-http-sql.md](04-http-sql.md) |
| 5 | ODBC: Oracle, SAP HANA, Teradata, DM8 | [05-odbc.md](05-odbc.md) |
| 6 | NoSQL: MongoDB, SurrealDB, Redis, DynamoDB, Cassandra/Scylla | [06-nosql.md](06-nosql.md) |
| 7 | Wyszukiwarki: Elasticsearch, Typesense, Weaviate | [07-search.md](07-search.md) |
| 8 | Streaming: Kafka, etcd | [08-streaming.md](08-streaming.md) |
| 9 | Beancount: parser + BQL | [09-beancount.md](09-beancount.md) |
| 10 | Weryfikacja: docker-compose, testy live, przegląd bezpieczeństwa | [10-weryfikacja.md](10-weryfikacja.md) |

Mapa 40 baz na drivery, URL-e i sposób testu: [bazy.md](bazy.md).

## Kryteria ukończenia

- `cargo build --release` przechodzi z domyślnymi features (wszystkie drivery).
- `cargo test` przechodzi: testy jednostkowe parserów odpowiedzi, Beancount i BQL.
- `tests/live.rs` przechodzi dla każdej bazy z kolumny "Test" = docker w [bazy.md](bazy.md).
- TUI sprawdzona przez `tui run` na SQLite i Postgres: drzewo, Records ze stronicowaniem, Query.
- Przegląd bezpieczeństwa diffu (skill security-review), wynik w raporcie.

## Poza zakresem v1

Edycja komórek, zakładka Properties (kolumny, indeksy, klucze), autouzupełnianie,
podświetlanie składni, anulowanie trwającego zapytania, certyfikaty klienta TLS,
logowanie OAuth/SSO (tylko tokeny w URL lub env), zapis haseł w keychainie.

## Wymaga zgody Artura w trakcie

- Studio: `rustup update` (tam cargo 1.88, lokalnie 1.99) i uruchomienie OrbStack (daemon nie działa).
- Instalacja driverów ODBC do testu etapu 5 (`brew install sqliteodbc` albo `psqlodbc`).
- Testy live na kontach chmurowych: tylko jeśli dostanę tokeny w env.
