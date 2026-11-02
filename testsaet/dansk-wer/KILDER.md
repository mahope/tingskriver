# Kilder og rettigheder til dansk WER

Der følger **ingen lydoptagelser** med denne ændring. `manifest.json` er en
bevidst tom indsamlingsskabelon, som loaderen afviser ved evaluering.
`fixtures/` indeholder alene nye, repository-forfattede teksttestdata under
repoets MIT-licens. De er ikke optagelser eller målinger af talegenkendelse.

## Kontrolleret kilde, 1. november 2026

[FLEURS-kildekortet hos Google](https://huggingface.co/datasets/google/fleurs/blob/70bb2e84b976b7e960aa89f1c648e09c59f894dd/README.md)
angiver CC BY 4.0 og dansk konfiguration `da_dk`, 16 kHz. Licensen tillader
genbrug med tilskrivning. Se også [licensvilkårene](https://creativecommons.org/licenses/by/4.0/).
Kildekortets licens er ikke dokumentation for individuelt samtykke til nye
optagelser. Udfyld aldrig samtykke som dokumenteret uden belæg.

Forsøg på at hente 20 danske test-rækker enkeltvis via
`https://datasets-server.huggingface.co/rows?dataset=google%2Ffleurs&config=da_dk&split=test&offset=0&length=20`
gav HTTP 500, `TooBigContentError`. Der blev ikke hentet lyd. Et fuldt
arkiv/parquet-udtræk er ikke udvalgt eller rettighedsregistreret her.
CoRal-v3-kildekortet kunne ikke åbnes på den undersøgte URL; der er derfor
ikke godkendt eller hentet CoRal-klip.

## Indsamling hos Mads

1. Hent FLEURS `da_dk` fra den fastlåste revision ovenfor på en maskine med
   adgang til datasættets filer. Brug test-split og gem oprindeligt filnavn,
   split, række-id og revision. Udvælg højst ca. 10 oplæsningsklip med fulde
   ytringer på højst 8 sekunder. Kopiér referencen fra præcis samme række.
2. Find ca. 10 samtaleklip med dokumenteret fri licens og kendte dialekter,
   eller brug egne optagelser **kun med alle medvirkendes dokumenterede
   samtykke til optagelse, evaluering og eventuel offentlig distribution**.
   En tilladelse til lokal test er ikke tilladelse til at lægge lyd i GitHub.
   Hvis offentlige rettigheder mangler, behold lyd og manifest lokalt.
3. Læg PCM16 WAV, 16 kHz mono, højst 8 sekunder pr. klip i `clips/`.
   Højst 20 klip; denne størrelse giver under 5,2 MB PCM-data, under 8 MB.
   Beskær ikke talte ord. Er en ytring for lang, vælg en anden.
4. Tilføj én manifestpost pr. klip og én kildepost nedenfor med oprindeligt
   id, URL/revision, licenslink, tilskrivning, ændringer (fx formatkonvertering)
   og grundlaget for samtykke. `dialect: "unknown"` hvis ukendt.
   Ved FLEURS kan `source-documented` kun bruges, når grundlaget i kilden er
   gennemgået og noteret; ellers skal klippet afvente afklaring.
5. Tilskriv fx “FLEURS, Google og datasættets bidragydere, CC BY 4.0,
   da_dk/test/<oprindeligt id>, revision 70bb2e8…, konverteret til PCM16”.
   Bevar også den præcise tilskrivning, som kilden kræver.

## Registrerede lydklip

Ingen endnu. Udfyld pr. klip før offentlig commit; opfind ikke kilde,
licens, dialekt eller samtykke.
