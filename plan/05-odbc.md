# Etap 5: ODBC (Oracle, SAP HANA, Teradata, Dameng DM8)

## Kroki
1. `odbc-api` z `default-features = false`, features `odbc_version_3_80` + `vendored-unix-odbc` (bez zależności od systemowego unixODBC przy linkowaniu; `prompt`/winit wyłączony).
2. URL: `odbc:<connection string>` albo `odbc:DSN=nazwa`. Driver bazy instaluje użytkownik (Oracle Instant Client ODBC, HDBODBC, Teradata ODBC, DM8 ODBC).
3. `query`: `execute`, kursor tekstowy (`TextRowSet`, 200 wierszy na batch), NULL -> `NULL`.
4. `groups`: funkcja katalogowa `tables()` (schema, name).
5. `browse`: dialekt zależny od bazy, więc `SELECT * FROM "s"."t"` + pobranie tylko jednej strony z kursora (pominięcie `page*200` wierszy). Bez `LIMIT`, bo każda z 4 baz ma inną składnię.

## Gotowe, gdy
- live: ścieżka ODBC przez SQLite ODBC driver (`brew install sqliteodbc`, za zgodą) albo psqlODBC na kontenerze Postgres.
- Oracle, HANA, Teradata i DM8 nie są testowane na żywo (brak obrazów ARM albo licencji); raport mówi to wprost.
