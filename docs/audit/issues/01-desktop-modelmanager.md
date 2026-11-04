# Byg modelhåndtering i GPUI med download, status og sletning

Labels: `enhancement`, `område/ux`, `prio/høj`, `størrelse/L`, `findes-i-planen`

## Baggrund

Core har modelhåndtering til Whisper, Parakeet og lokale sprogmodeller, og Tauri har modelmanagere. GPUI har ingen tilsvarende side. #53 dækker udbydervalg, model-id og forbindelsestest; #134 dækker valgfrit modeldownload på Android. Dette issue dækker desktop-modellernes livscyklus og supplerer #53 uden at overtage dens udbydereditor.

Se revisionen i `docs/plans/feature-synlighed-audit.md` (#169).

## Opgave

- Byg en modeloversigt under GPUI-indstillinger med motor, model-id/revision, licens, størrelse, kilde og installeret/valgt/indlæst-status. Skeln aktive valg pr. funktion fra modeller valgt til Måling.
- Genbrug `crates/tingskriver-core/src/whisper_engine/commands.rs`, `parakeet_engine/commands.rs` og `summary/summary_engine/model_manager.rs`. Kobl valg til udbydermodellen fra #53 og dikteringsvalg fra #50.
- Tilbyd eksplicit download, fremdrift, annullering, genforsøg og sletning med bekræftelse. Åbning af siden må ikke hente modeller. Vis manglende lokal model, diskfejl, netudfald og korruption forståeligt. Vis kun kontrolsummer/verifikation som gennemført, når motoren faktisk har verificeret dem.
- Bevar en eksisterende brugbar model ved mislykket download eller aktivering. Afklar motorernes integritets- og atomiske aktiveringskontrakt før UI lover det. Blokér sletning af en model i brug, og forklar hvorfor.
- Vis relevante warm/idle-valg og GPU/CPU-/reserve-status uden at gøre interne procesdetaljer til brugerkrav. Lever sikre modeloplysninger til diagnosekopien i #171.
- Brug strengkataloget, tastatur og AccessKit/UIA samt lys/mørk. Dokumentér modelvalg, download, licenser og fejlhåndtering i brugerhjælp og release notes.

## Acceptkriterier

- [ ] Brugeren kan se installerede og manglende desktop-modeller og vælge en brugbar model gennem #53/#50 uden at redigere filer.
- [ ] Ingen download eller eksternt kald starter ved åbning; download kræver brugerens valg og viser størrelse/kilde.
- [ ] Tests med små fixtures dækker annullering, timeout, korruption, utilstrækkelig disk, genstart og bevaret eksisterende model. Ingen store modeldownloads i tests.
- [ ] Aktiv model kan ikke slettes under optagelse, diktering eller igangværende inference; en forklaring vises.
- [ ] Modelrevision/licens og faktisk verifikationsstatus kan ses; uunderstøttede metadata vises som ukendte.
- [ ] Mock-skærmbilleder dækker tom, installeret, henter, annulleret og fejl i lys/mørk via #49; tastatur og UIA-navne verificeres efter #73.
- [ ] Hjælp og ændringslog beskriver den leverede GPUI-funktion og dens begrænsninger.

Afhænger af udbyderindstillingerne i #53; koordinér med #50, #171, #49 og #73. Android-download forbliver i #134.
