# Etap 0: szkielet

## Kroki
1. `cargo new dbtui` w `/Users/artur/Developer/db`, `git init`, `.gitignore` (`target/`).
2. Zależności bazowe: `ratatui` (z re-eksportem crossterm), `tokio`, `anyhow`, `async-trait`, `serde`, `toml`, `dirs`, `url`.
3. `src/driver/mod.rs`:
   - `Grid`, `Group`, trait `Driver` z domyślnymi `groups`/`browse` opartymi na `query`.
   - `connect(url) -> Result<Box<dyn Driver>>`: `match` po schemacie URL, aliasy (`cockroachdb`, `redshift`, `pglite` -> postgres; `mariadb`, `tidb`, `oceanbase`, `databend` -> mysql; ...); goła ścieżka pliku -> po rozszerzeniu (`.db`/`.sqlite`, `.duckdb`, `.beancount`).
   - Helpery: `quote_ident(name, '"' | '`' | '[')`, `group_rows(grid) -> Vec<Group>` (zwija 2 kolumny schema/table).
4. `src/config.rs`: odczyt `~/.config/dbtui/config.toml`, lista `[[connection]] name, url`. Brak pliku = pusta lista.
5. `src/main.rs`: `dbtui <url>` łączy od razu, `dbtui` bez argumentu pokazuje listę z configu.
6. Cargo features: jedna na ciężką rodzinę (`duckdb`, `odbc`, `mongodb`, `cql`, `kafka`), wszystkie w `default`.
   Nieskompilowany driver zwraca błąd "dbtui built without feature X".

## Gotowe, gdy
- `cargo build` przechodzi, `connect("x://")` zwraca czytelny błąd.
- Testy jednostkowe dispatchu URL (alias -> rodzina) i `group_rows`.
