# Etap 7: wyszukiwarki i bazy wektorowe

## search.rs (Elasticsearch, Typesense, Weaviate)
Jeden driver, trzy warianty drzewa:

| Baza | Drzewo | browse |
|---|---|---|
| Elasticsearch | `GET /_cat/indices?format=json` | `POST /{i}/_search` `{from, size}` -> `hits.hits` |
| Typesense | `GET /collections` | `GET /collections/{c}/documents/search?q=*&page&per_page` -> `hits[].document` |
| Weaviate | `GET /v1/schema` -> klasy | `GET /v1/objects?class=C&limit&offset` -> `objects` |

`query`:
- linia `METHOD /path` + opcjonalne JSON body w kolejnych liniach -> surowe żądanie, odpowiedź przez `json_to_grid` (pierwsza tablica obiektów w odpowiedzi);
- Elasticsearch: tekst bez metody HTTP -> `POST /_sql?format=json` (`columns`, `rows`).
Typesense: nagłówek `X-TYPESENSE-API-KEY` z URL. Weaviate: opcjonalny Bearer.

## Gotowe, gdy
live: elasticsearch (indeks + dokument + `_sql`), typesense (kolekcja + dokument), weaviate (klasa + obiekt + GraphQL `Get`).
