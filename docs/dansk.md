# Dansk polering

Kommastilen gemmes pr. tilstand som `commaStyle`: `grammatical`
(grammatisk komma med startkomma), `clarity` (klarhedskomma uden
startkomma) eller `unchanged` (bevar eksisterende kommaer).
Standarden er `grammatical`, også for eksisterende tilstande uden feltet.
Andre indstillinger bevares, og formatversionen ændres ikke. Valget gælder
AI-poleringen i core.
Polering har et samlet budget på højst 2 sekunder, også hvis et ældre
`polishBudgetMs` er højere. Et kortere budget respekteres. Fejl og timeout
giver lag 1-teksten.

## 20 sætninger til manuel A/B-vurdering (#103)

Manuel vurdering ved Mads mangler. Kør samme lag 1-tekst gennem begge
kommastile, og sammenlign med `unchanged`. Notér resultat A, resultat B,
foretrukken version og eventuelle ændringer i betydning eller engelske ord.
Sætningerne er input uden facit. OdinCore `odin-gec` er ikke afprøvet;
leverandøren er uverificeret, og forsøget kræver en nøgle.

1. Jeg tror at vi kan nå det inden fredag.
2. Når mødet er slut sender jeg referatet.
3. Hun sagde at planen var klar men at budgettet manglede.
4. Den fil som du sendte i går kan ikke åbnes.
5. Hvis du har tid kan vi gennemgå ændringen sammen.
6. Jeg ved ikke om testen er færdig.
7. Vi bliver her indtil alle er kommet.
8. Fordi toget var forsinket kom vi senere end aftalt.
9. Han spørger om vi kan vente til hun er tilbage.
10. Det er vigtigt at alle ved hvad de skal gøre.
11. Jeg købte æbler pærer brød og mælk.
12. Vi mødes tirsdag og vi afleverer rapporten onsdag.
13. Rapporten er efter min mening klar til godkendelse.
14. Ja det kan vi godt gøre.
15. Nej vent til testen er færdig.
16. Vi skal reviewe den pull request som retter fejlen.
17. Hvis buildet er grønt kan vi deploye når mødet er slut.
18. Hun skriver på dansk men bruger ordene backlog og sprint planning.
19. Jeg kommer torsdag nej fredag hvis vejret tillader det.
20. Jeg tror, at du har ret, men vi bør kontrollere tallene.
