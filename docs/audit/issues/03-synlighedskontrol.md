# Indfør fast kontrol af funktionssynlighed i PR- og releaseflowet

Labels: `enhancement`, `område/ux`, `prio/mellem`, `størrelse/S`, `findes-i-planen`

## Baggrund

#169 er en engangsrevision. Mads' regel er, at alle nye features skal vises i interface, indstillinger, autoopdateringer og øvrig relevant hjælp. Et løbende leverancekrav mangler. #97 opdaterer GPUI's bygge- og bidragsvejledning; dette issue gør synlighedskontrollen til en del af hver fremtidig featureleverance. Det bygger ingen ny produktskærm.

## Opgave

- Opdatér `.github/pull_request_template.md` med konkrete felter for brugerens indgang til funktionen, relevante indstillinger, tom/loading/fejl, dansk tekst, tastatur/AccessKit og lys/mørk-bevis. Tillad begrundet ikke relevant for interne ændringer.
- Udvid `docs/release-checkliste.md` med sammenhæng mellem feature, faktisk leveret skærm, Om-/modelmetadata (#171), opdateringsflow (#170), brugerhjælp og release notes. Umergede grene og Tauri-only UI må ikke beskrives som leveret GPUI.
- Beskriv i CONTRIBUTING sammen med #97, hvordan checklisten bruges, og hvem der verificerer den. Genbrug `docs/plans/feature-synlighed-audit.md` som baseline; undgå en ny konkurrerende featureliste.
- Kræv kontrol af offentlige featureløfter i README og `site/` ved relevante ændringer. Ingen persondata, private stier eller hemmeligheder i eksempeldata og skærmbilleder.

## Acceptkriterier

- [ ] En ny feature-PR angiver skærm, indstilling, hjælp og release-note-tekst eller en konkret begrundelse for ikke relevant.
- [ ] Releasechecklisten kontrollerer leveret version og kanal, Om-/modeloplysninger og brugerens opdateringsvalg, hvor relevant.
- [ ] Checklisten skelner main, umerget gren, Tauri og GPUI, så eksisterende skærmbilleder ikke bruges som bevis for en anden app.
- [ ] En gennemgang med en UI-feature og en intern rettelse viser, at kravene kan anvendes uden meningsløse nye indstillinger.
- [ ] #97 beholder ejerskabet til byggevejledningen; #170/#171 beholder ejerskabet til produktimplementeringen.
