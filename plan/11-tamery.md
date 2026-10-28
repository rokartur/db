# Etap 11: UI jak Tamery

Cel: dbtui odwzorowuje Tamery.app (https://github.com/wannabespace/tamery) w terminalu:
ekran połączeń, widok bazy, sortowanie i filtry kolumn, edycja komórek i usuwanie wierszy.

Stan wyjściowy (niezacommitowany w chwili pisania): `src/picker.rs` (lista z Recent, grupy env/typ,
sort, filtr `/`, dodawanie `a`, usuwanie `d`), paleta `Ctrl-P` nad widokiem bazy, `?` na liście,
Postgres TLS w semantyce libpq (`prefer`/`require` bez weryfikacji, `verify-ca`/`verify-full` z nią).

Każdy etap UI zaczyna się od mocków A/B/C (skill html-communication) i wyboru przez Artura.

## 11a. Ekran połączeń 1:1

```
                 LAST OPENED
                 [pg] adescom / vapp_admin
                 [pg] adescom local docker / app

                 3 CONNECTIONS      g: Group by label  s: Date (newest first)  n: New
               ┌──────────────────────────────────────────────────────────────────┐
               │ [pg] adescom vpn             10.125.1.14      vapp_admin     …   │
               │ [pg] adescom local docker    localhost:5433   app            …   │
               │ [pg] adescom                 89.25.247.27     vapp_admin     …   │
               └──────────────────────────────────────────────────────────────────┘
```

- Kolumna na środku, szerokość do 90 znaków.
- LAST OPENED: do 5 pozycji `połączenie / baza`. `x` usuwa pozycję z ostatnio otwartych.
  Zastępuje sekcję Recent.
- Grupowanie (`g`): Group by label, Group by type, No grouping. Domyślnie label.
- Sortowanie (`s`): Date (newest first), Date (oldest first), Name (A–Z), Name (Z–A).
  Date to kolejność w `config.toml`, bez nowego pola.
- Pole `env` zmienia nazwę na `label`. Dowolny tekst, w formularzu podpowiedzi local/test/prod.
  Bez pola: `local` dla localhost i plików. Kodu jeszcze nikt nie używa, więc bez migracji.
- Wiersz: skrót typu w kolorze (pg, my, sq, rd...), nazwa, `host:port` bez sekretów, baza.
- `m` albo `.` otwiera menu: Edit, Copy connection string (OSC 52, z hasłem, z ostrzeżeniem
  w statusie), Remove (dialog potwierdzenia). Pomijamy Open in New Window, Copy database
  i Clear saved credentials (brak odpowiednika).
- Edit używa formularza New. Zapis przez `config::save`, który przepisuje plik i traci komentarze.
- `/` filtr i paleta `Ctrl-P` zostają: w Tamery ich rolę pełni ⌘K.

## 11b. Widok bazy

```
┌ Tables ────────── r ┐ customers │ products │ SQL 1 │
│ / pro               │ public / products                       / filtr AI: pomijamy
│ v public            │ 7 columns • 16 rows
│     products        │ ┌────────────────────┬──────────────┬─────────────────────┐
│ v auth              │ │ id            ↑    │ name         │ created_at          │
│     saml_providers  │ │ uuid  (pk)         │ text         │ timestamptz         │
│     sso_providers   │ ├────────────────────┼──────────────┼─────────────────────┤
└─────────────────────┘ │ 8d618275-2b94-...  │ Digital Cam. │ 2025-03-10 15:20    │
```

- Panel Tables: wyszukiwarka `/` z podświetleniem trafień, zwijane schematy, `r` odświeża.
- Zakładki: Enter na tabeli otwiera zakładkę albo przełącza na już otwartą, `t` otwiera nową
  zakładkę SQL, `x` zamyka, `[`/`]` przełączają. Zakładka trzyma własny Grid, stronę, sort i filtry.
  Stronicowanie przechodzi na `n`/`p`.
- Nagłówek: `schema / table`, `N columns • M rows`.
- Kolumny z typem pod nazwą i oznaczeniem klucza głównego.

Zmiany w kontrakcie drivera, w każdym metoda domyślna:
- `columns(group, item) -> Vec<Column { name, kind, primary }>`. Domyślnie `information_schema.columns`
  z kluczami z `table_constraints`. Bazy bez schematu (Redis, Kafka, etcd) zwracają pustą listę.
- `count(group, item) -> Option<u64>`. Domyślnie `SELECT count(*)`, `None` tam, gdzie to drogie
  albo niemożliwe (Kafka, DynamoDB).

## 11c. Sortowanie i filtry

- `o` na kolumnie: rosnąco, malejąco, bez sortowania. Strzałka ↑/↓ w nagłówku.
- `f` dodaje chip filtra dla bieżącej kolumny: operator (`=`, `!=`, `>`, `>=`, `<`, `<=`, `like`,
  `not like`, `is null`, `is not null`) i wartość. `F` czyści wszystkie. Chipy w linii pod nagłówkiem.
- `browse` dostaje `Browse { page, sort, filters }` zamiast `page`.
- Rodziny SQL budują `WHERE`/`ORDER BY`. Identyfikatory przez `quote_ident`. Wartości jako
  parametry tam, gdzie driver je ma (tokio-postgres, mysql_async, tiberius, rusqlite, duckdb,
  ClickHouse `{p:String}`). Inaczej literał z ucieczką dialektu: MySQL ucieka też backslash.
  Test live na wartość `' OR 1=1 --` i `\'`.
- Mongo: filtry mapowane na `$eq`/`$gt`/`$regex`. Pozostałe NoSQL zwracają błąd
  "filters are not supported for <typ>".

## 11d. Edycja komórek i usuwanie wierszy

- Tylko tabele z kluczem głównym w rodzinach: Postgres, MySQL, MSSQL, SQLite, DuckDB, libSQL.
  Pozostałe: status "read-only: <powód>".
- `Grid` rozróżnia NULL od tekstu "NULL": komórki stają się `Option<String>`. Zmiana dotyka
  wszystkich 20 driverów i ma osobny commit.
- `e` na komórce: popup edytora (wieloliniowy, JSON sformatowany). `Ctrl-S` zapisuje,
  `Ctrl-N` ustawia NULL, `Esc` anuluje.
- `Space` zaznacza wiersze, `D` usuwa zaznaczone.
- Każdy zapis pokazuje przed wykonaniem dialog z dokładnym SQL i liczbą wierszy (`y` wykonuje).
  `UPDATE`/`DELETE` idą w transakcji i są wycofywane, gdy dotkną innej liczby wierszy niż wybrana.
- Driver: `update(group, item, key: Vec<(col, value)>, col, value)` i `delete(group, item, keys)`
  z parametrami, nigdy przez sklejanie wartości.

## Kolejność i weryfikacja

1. 11a, potem 11b, 11c, 11d. Jeden commit na etap, `NULL` w Grid osobno przed 11d.
2. Każdy etap: `cargo test`, `cargo clippy --all-targets`, `tui run` na SQLite i Postgres z dockera.
   Etapy 11c i 11d: testy live na kontenerach z `docker-compose.yml`.
3. 11c i 11d: przegląd bezpieczeństwa (skill security-review). Główne ryzyko to wstrzyknięcie
   przez wartości filtrów i edycji.
4. Edycja nigdy nie jest testowana na bazach Artura (adescom), tylko na kontenerach.
