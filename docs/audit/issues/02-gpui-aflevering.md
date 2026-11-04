# Byg aflevering af referater i GPUI med mål, samtykke og køstatus

Labels: `enhancement`, `område/integrationer`, `område/ux`, `prio/høj`, `størrelse/L`, `findes-i-planen`

## Baggrund

#70 er lukket efter implementering af `crates/tingskriver-core/src/delivery/`, versioneret afleveringsskema og Tauri-kontroller. `docs/delivery.md` siger udtrykkeligt, at GPUI-porten er separat. GPUI har hverken afleveringsmål, test, godkendelse eller køvisning. #51 dækker mødedokumentet, #85 generelle indstillinger og #67 manuel eksport; dette issue dækker den vedvarende afleveringsfunktion.

Se revisionen i `docs/plans/feature-synlighed-audit.md` (#169).

## Opgave

- Byg Indstillinger → Aflevering i GPUI med mål til mappe, webhook og SSH, kanalformat, opret/redigér/slet og eksplicit Test mål med syntetiske data. Genbrug core-validering og Credential Manager via eksisterende KeyStore. Vis aldrig gemte webhook-hemmeligheder igen.
- Tilslut én delt Delivery-service til GPUI-værten med opstart/genoptagelse og periodisk `run_due()` uden at blokere UI. Del den med MCP-handlinger, når #105 integreres; åbn ikke parallelle serviceejere til samme kø.
- Tilføj Godkend og aflever på mødedokumentet fra #51: vælg mål, gem redigeringer og godkend et klart afgrænset snapshot. Generering af referat er ikke godkendelse til aflevering.
- Vis kø med afventer, kører, pauset af privatliv, gennemført og fejlet samt forsøg/næste forsøg og eksplicit genforsøg. Forklar risikoen for dubletter ved fjernmodtagere.
- Kobl Kun lokal, Privat tilstand og privat møde til eksisterende fail-closed afleveringspolitik og koordinér med #84/#63. Beskriv, at denne politik styrer aflevering og ikke alene blokerer alle AI-udbydere.
- Følg `docs/delivery.md` for låste mål med ufærdige jobs, upload-only SSH, sikre fejltekster og test af mål. Behold #99 som ejer af ingest-kontrakt/validerings-CLI.
- Brug danske strenge, tastatur, AccessKit/UIA, lys/mørk og brugerhjælp/release notes.

## Acceptkriterier

- [ ] Mål kan administreres og testes fra GPUI uden Tauri eller manuel filredigering; at gemme et mål sender intet.
- [ ] Mødeindhold sendes kun efter eksplicit godkendelse eller en tilladt MCP-handling efter #105's politik; almindelig UI-generering udløser ingen aflevering.
- [ ] Kun lokal, Privat tilstand og privat møde stopper aflevering, test og genforsøg efter core-kontrakten; tests dækker politikændring under en igangværende aflevering.
- [ ] App-genstart genoptager køen, og genforsøg/status vises uden UI-blokering. Tests bruger syntetisk indhold, temp-mapper og lokale fake-modtagere.
- [ ] Ingen nøgle, webhook-URL, privat målsti eller mødeindhold vises i statusfejl, diagnosekopi eller testlogs.
- [ ] Mock-skærmbilleder omfatter ingen mål, gemmefejl, godkendelse, kø, pause og fejl i lys/mørk efter #49; tastatur/UIA verificeres efter #73.
- [ ] `docs/delivery.md`, brugerhjælp og release notes beskriver GPUI-flowet og begrænsningerne.

Afhænger af GPUI-mødedokumentet #51 og indstillingsværten #85. Core fra #70 findes allerede. Koordinér politik med #84/#63 og serviceejerskab med #105.
