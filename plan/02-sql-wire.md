# Etap 2: SQL wire

## postgres.rs (PostgreSQL, CockroachDB, PGlite, Redshift)
- `tokio-postgres` + `postgres-native-tls`; `simple_query` zwraca tekst, więc nie trzeba mapować typów.
- Schemat URL przepisany na `postgres://`; `sslmode` z URL (Redshift: `require`).
- `groups`: domyślny z information_schema; wykluczone `pg_catalog`, `information_schema`, `crdb_internal`, `pg_extension`.
- Wiele instrukcji w zapytaniu: pokazujemy wynik ostatniego SELECT, dla DML liczbę wierszy.

## mysql.rs (MySQL, MariaDB, TiDB, OceanBase, Databend)
- `mysql_async`, protokół tekstowy (`query_iter`), `Value` -> String.
- `browse` z backtickami; `groups` wyklucza `mysql`, `sys`, `performance_schema`, `information_schema`, `metrics_schema`, `oceanbase`, `system`.
- `query(group, ..)`: najpierw `USE group`, gdy grupa jest niepusta.

## mssql.rs (SQL Server)
- `tiberius` + `tokio-util` compat; konfiguracja z URL (host, port, user, hasło, baza, `trust_cert`).
- `browse`: `SELECT * FROM [s].[t] ORDER BY (SELECT NULL) OFFSET n ROWS FETCH NEXT 200 ROWS ONLY`.
- `ColumnData` -> String (liczby, napisy, daty, GUID, binaria jako hex).

## Gotowe, gdy
Testy live: postgres, cockroach, pglite, mysql, mariadb, tidb, databend, mssql przechodzą scenariusz z [10-weryfikacja.md](10-weryfikacja.md).
