# Etap 6: NoSQL

## mongo.rs (MongoDB)
- `mongodb` crate. Drzewo: bazy -> kolekcje.
- `browse`: `find` ze `skip`/`limit`, dokumenty na Grid przez `json_to_grid` (relaxed extended JSON).
- `query`: tekst = komenda JSON (`{"find":"users","filter":{"age":{"$gt":30}}}`), `run_command` na bazie z grupy; `cursor.firstBatch` -> wiersze, inaczej dokument spłaszczony.

## surreal.rs (SurrealDB)
- HTTP `POST /sql`, Basic auth, nagłówki `Surreal-NS`, `Surreal-DB` z URL.
- Drzewo: `INFO FOR DB` -> tabele. `browse`: `SELECT * FROM t LIMIT 200 START n`.
- Odpowiedź: tablica `{status, result}`; bierzemy ostatni wynik, `ERR` -> błąd.

## redis.rs (Redis)
- `redis` crate (tokio). Drzewo: grupa `db<N>` -> klucze z `SCAN` (limit 1000, shortcut opisany w kodzie).
- `browse` po `TYPE`: string, hash (`HGETALL`), list (`LRANGE`), set (`SSCAN`), zset (`ZRANGE WITHSCORES`), stream (`XRANGE`).
- `query`: surowa komenda dzielona po białych znakach + obsługa `"..."`; odpowiedź rekurencyjnie na Grid.

## dynamodb.rs (DynamoDB)
- HTTP JSON 1.0 (`X-Amz-Target: DynamoDB_20120810.*`), SigV4 napisany ręcznie (`hmac` + `sha2`, około 60 linii, test na wektorze z dokumentacji AWS).
- Drzewo: `ListTables`. `browse`: `Scan` z `Limit` + `ExclusiveStartKey` (strony przechodzone po kolei). `query`: PartiQL `ExecuteStatement`.
- AttributeValue (`S`, `N`, `BOOL`, `M`, `L`, ...) -> tekst.

## cql.rs (Cassandra, ScyllaDB)
- `scylla` crate. Drzewo: `system_schema.keyspaces` -> `system_schema.tables` (bez keyspace'ów `system*`).
- `browse`: `SELECT * FROM ks.t` z pagingiem drivera, pominięcie `page*200` wierszy (CQL nie ma OFFSET).
- `CqlValue` -> tekst.

## Gotowe, gdy
live: mongo, surrealdb, redis, dynamodb-local, cassandra, scylla.
