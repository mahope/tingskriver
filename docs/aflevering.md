# Ingest-kontrakt for aflevering

Sidecar-skemaet er [docs/schemas/delivery-v1.schema.json](schemas/delivery-v1.schema.json).
Issuets navn `schema/sidecar-v1.json` henviser til denne eksisterende fil.
Der findes kun ét autoritativt skema.

En afleveringsmappe indeholder `sidecar.json`, `transcript.md`, `summary.md`
og `notes.md`. Sidecaren kræver `kilde: "tingskriver"`, `skemaversion: 1`,
en ikke-tom titel, RFC 3339-tidspunkt med tidszone samt listerne `talere`
og `opgaver`. Hver opgave kræver tekst og ansvarlig (tekst eller null).
Ukendte felter afvises. V1 har ingen filreferencer i JSON; de tre faste
Markdown-filer skal eksistere, også når noter er tomme. Valideringen
kontrollerer kontrakt og filtilstedeværelse, ikke indholdets faglige kvalitet.

Kør uden at åbne appvinduet:

```sh
tingskriver-app --valider-aflevering docs/examples/aflevering/gyldig
tingskriver-app --validate-delivery docs/examples/aflevering/ugyldig
```

Exitkode 0 betyder gyldig. Exitkode 1 betyder ugyldig med fejl på stderr.
Manglende mappe eller et flag i stedet for en mappe giver en brugsfejl.

På Windows hedder binæren `tingskriver-app.exe`. Release-buildet bruger
Windows GUI-subsystemet, så cmd og PowerShell venter ikke automatisk på
processen. Vent eksplicit for at kunne læse exitkoden. I cmd:

```bat
start "" /wait tingskriver-app.exe --valider-aflevering "<mappe>"
echo %ERRORLEVEL%
```

I PowerShell:

```powershell
$process = Start-Process -FilePath .\tingskriver-app.exe -ArgumentList '--valider-aflevering', '"<mappe>"' -Wait -PassThru
$process.ExitCode
```

Erstat `<mappe>` med mappestien (brug anførselstegn om stier med mellemrum).

Eksemplerne indeholder udelukkende syntetiske data; den ugyldige variant
har en forkert skemaversion. Core tilbyder den synkrone funktion
`delivery_validate::validate_delivery`; async værter skal kalde den på en
blokerende worker.

Enhver indholdsændring i en udgivet skemafil kræver en højere
`properties.skemaversion.const`; ændret `$id` alene er ikke nok.
En ny `delivery-vN.schema.json` skal have `const` lig N og højere end alle
udgivne versionskonstanter. Foretræk en ny versionsfil med tilsvarende `$id`
og versionskonstant, og behold gamle
publicerede skemaer til eksisterende modtagere. Opdatér valideringskoden
og dens tests sammen med kontrakten. CI sammenligner med merge-basen på
`origin/main` og afviser ændringer uden versionsbump:

```sh
python3 scripts/check-delivery-schema-version.py --base origin/main
python3 scripts/tests/test_delivery_schema_version.py
```
