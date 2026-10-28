# Etap 9: Beancount (własny parser + własny BQL)

## parser.rs
Dyrektywy: `option`, `include` (rekurencyjnie, ścieżka względna do pliku), `open`, `close`, `commodity`,
transakcje (`txn`, `*`, `!`) z payee/narration, tagami `#`, linkami `^`, `balance`, `price`, `pad`, `note`, `event`, `document`.
Postingi: `Konto  kwota WALUTA [{koszt}] [@ cena | @@ cena łączna]`, flaga postingu, metadane `klucz: wartość` (pomijane).
Komentarze `;`. Jeden posting bez kwoty dostaje kwotę wyrównującą per waluta.
Transakcja niezbilansowana (tolerancja 0.005) = błąd z numerem linii.
Kwoty: `rust_decimal`. Daty: napis ISO (porównanie leksykograficzne jest poprawne).

## Tabele (drzewo: grupa `ledger`)
| Tabela | Kolumny |
|---|---|
| `postings` (domyślna) | date, flag, payee, narration, account, number, currency, position, cost_number, cost_currency, price, tags, links, filename, lineno |
| `transactions` | date, flag, payee, narration, tags, links |
| `accounts` | account, open, close, currencies |
| `balances` | date, account, number, currency |
| `prices` | date, currency, number, quote |

## bql.rs: składnia
```
SELECT [DISTINCT] expr [AS name], ... | *
  [FROM tabela]
  [WHERE expr]
  [GROUP BY expr|numer, ...]
  [ORDER BY expr [ASC|DESC], ...]
  [LIMIT n]
BALANCES [WHERE expr]       => SELECT account, sum(position) ... GROUP BY account ORDER BY account
JOURNAL 'regex'             => postingi konta z kolumną balance (saldo narastające)
```
- Wyrażenia: literały (liczba, `'napis'`, `"napis"`, data `2024-01-31`, `TRUE`/`FALSE`/`NULL`), kolumny, `= != < <= > >=`, `~` (regex, bez wielkości liter), `AND OR NOT`, `+ - * /`, nawiasy, `IN (...)`.
- Funkcje skalarne: `year month day`, `lower upper length`, `parent root(acc, n) leaf`, `units number currency abs`, `str`.
- Agregaty: `sum` (na pozycjach = inwentarz wielowalutowy, wyświetlany jako `12.50 USD, 3 EUR`), `count`, `first`, `last`, `min`, `max`.
- Implementacja: tokenizer -> parser Pratta -> wykonanie na `Vec<Row>` (filtr, grupowanie przez `BTreeMap` po kluczu, sortowanie, limit).

## Gotowe, gdy
Testy jednostkowe na fixture `tests/fixtures/ledger.beancount`:
- auto-bilans postingu bez kwoty, błąd niezbilansowanej transakcji z linią;
- `BALANCES` daje oczekiwane salda (sprawdzone ręcznie);
- `SELECT year(date), sum(position) WHERE account ~ 'expenses' GROUP BY 1 ORDER BY 1`;
- `JOURNAL 'Assets:Bank'` z saldem narastającym;
- błąd składni z pozycją.
