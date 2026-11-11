# Review

Til dig, der gennemgår en PR. Reglerne i [AGENTS.md](AGENTS.md) og testpolitikken i [docs/testing.md](docs/testing.md) gælder.

1. Passer ændringen til issuet? Står `Fixes #N` i teksten, og er hvert acceptkriterium opfyldt eller udtrykkeligt ikke opfyldt?
2. Tabes der lyd, transskript eller data nogen steder? Tjek stop, nedbrud, fuld disk og enhedsskift. Optagelsen må ikke afhænge af transskription.
3. Er der en test for hver rettet fejl, og fejler den på den gamle kode? Navngiver den fejlen?
4. Har ændringen rørt en crate? Læs dens `AGENTS.md` og tjek Invarianter. Core må ikke afhænge af UI-crates.
5. Er kontrollerne fra tabellen i [AGENTS.md](AGENTS.md) kørt? En sprunget kontrol skal have kommando og årsag i PR-teksten.
6. Er brugertekst dansk med æøå, fra `strings`, og kan ny adfærd ses og styres i appen? Tastatur, skærmlæser, lys og mørk tilstand, tomme tilstande og fejltilstande?
7. Sendes noget af sted, som brugeren ikke har bedt om? Ingen telemetri, ingen skjulte netværkskald.
8. Er der hemmeligheder, personoplysninger eller private stier i kode, tests, docs eller skærmbilleder?
9. Er fremmed kode registreret under "Porteret fra" i [NOTICE.md](NOTICE.md), og er nye afhængigheder licenstjekket (`cargo deny`)?
10. Er diffen kun én ændring? Ingen drive-by-omdøbning, ingen reformatering af urørte filer, intet versionsbump.
