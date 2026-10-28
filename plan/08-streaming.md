# Etap 8: streaming i koordynacja

## kafka.rs (Apache Kafka)
- `rskafka` (czysty Rust, bez librdkafka/cmake).
- Drzewo: grupa `topics` -> tematy z metadanych.
- `browse`: rekordy z każdej partycji od najwcześniejszego offsetu, strona = 200 rekordów; kolumny `partition, offset, timestamp, key, value` (UTF-8 lossy).
- `query`: `produce <topic> <value>`, `consume <topic> [partition] [offset]`, `topics`.

## etcd.rs (etcd)
- Gateway JSON etcd v3 (`POST /v3/kv/range`, `/v3/kv/put`, `/v3/kv/deleterange`), klucze w base64. Bez gRPC i protoc.
- Drzewo: grupa `keys` -> klucze (`keys_only`, limit 1000). `browse`: wartość + `create_revision`, `mod_revision`, `version`.
- `query`: `get <key>`, `get --prefix <p>`, `put <key> <value>`, `del <key>`.

## Gotowe, gdy
live: kafka (produce -> temat w drzewie -> browse widzi rekord), etcd (put -> get -> del).
