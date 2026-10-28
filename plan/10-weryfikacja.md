# Etap 10: weryfikacja

## docker-compose.yml
Jeden serwis na bazę z [bazy.md](bazy.md) (kolumna "docker"), profile do uruchamiania partiami
(`sql`, `analytics`, `nosql`, `search`, `stream`), bo wszystko naraz nie zmieści się w pamięci.
Gdzie uruchamiać: Studio (32 GB) po uruchomieniu OrbStack, za zgodą; inaczej lokalnie po jednej partii.

## tests/live.rs
Tabela przypadków: `(nazwa, env z URL, instrukcje setup, grupa, element, oczekiwana komórka)`.
Scenariusz dla każdej bazy:
1. `connect(url)`;
2. setup przez `query` (utwórz tabelę/kolekcję/klucz/temat, wstaw 1 rekord);
3. `groups()` zawiera utworzony element;
4. `browse(grupa, element, 0)` zwraca wstawioną wartość;
5. `query` z błędem składni zwraca `Err`, nie panikuje.
Brak zmiennej env: przypadek wypisany jako pominięty (żeby nie dawał fałszywej zieleni), `DBTUI_LIVE_STRICT=1` zamienia pominięcie w błąd.

## Kolejność
1. `cargo test` (jednostkowe, fixture, BQL).
2. Partia po partii: `docker compose --profile X up -d --wait`, `cargo test --test live`, `docker compose down -v`.
3. TUI: `tui run dbtui ./target/debug/dbtui sqlite://...`, potem Postgres; zrzut ekranu i porównanie z układem z [01-ui.md](01-ui.md).
4. Sprawdzenie, że test potrafi paść: zepsuta oczekiwana komórka -> czerwono.
5. Przegląd bezpieczeństwa diffu (skill security-review). Główne ryzyka: tokeny w URL w pasku statusu i logach (maskowanie), wstrzyknięcie przez nazwy tabel w `browse` (cytowanie identyfikatorów), TLS domyślnie włączony dla chmur.

## Raport końcowy
Macierz 40 baz: test live / fixture / brak testu (z powodem).
