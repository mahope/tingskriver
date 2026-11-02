# Selvdiagnose

Kør `tingskriver-app --diagnose` (distributionsnavnet kan være `tingskriver`),
eller `tingskriver-app --diagnose --json`. Diagnosen kører før GPUI, database,
ONNX-initialisering og single-instance-håndtering. Exitkode 1 betyder mindst ét
fejlfund; 0 betyder ingen påviste fejl. Ukendte kontroller er advarsler og giver
ikke exitkode 1. En rapport uden fejl er derfor ikke et bevis for et fejlfrit miljø.
Windows-releaseprogrammet tilslutter den kaldende konsol, hvis stdout ikke allerede
er omdirigeret. Brug `Start-Process -Wait -RedirectStandardOutput diagnose.json`
fra PowerShell, hvis terminalen ellers ikke venter på GUI-programmet.

## Kontroller og stabile id'er

| Id | Kontrol | Rettelseshandling ved fejl |
| --- | --- | --- |
| `cpu.avx2` | CPU-feature-detektion med Rust | `UseCpuPath` |
| `runtime.vulkan` | Indlæs kun `vulkan-1.dll` fra System32; frigiv igen | `InstallVulkanRuntime` |
| `permission.microphone` | ConsentStore microphone og `NonPackaged` `Value`, HKCU og HKLM | `OpenSettings("ms-settings:privacy-microphone")` |
| `audio.output` | Eksisterende `audio::devices::default_output_device` | `OpenSettings("ms-settings:sound")` |
| `shortcut.conflict` | Midlertidig RegisterHotKey/UnregisterHotKey | `ChooseOtherShortcut` |
| `clipboard.locked` | OpenClipboard, højst tre forsøg med 10 ms mellemrum | `CloseClipboardOwner` |
| `window.integrity` | Sammenlign token-integritetsniveau for eget og aktivt program | `RunAsSameUser` |

Mikrofontjekket læser både microphone-roden og `NonPackaged` for skrivebordsapps
i HKCU og HKLM. Deny i en af de fire værdier giver fejl; OK kræver Allow i alle
fire. Manglende værdier, andre værdier og adgangsfejl giver ukendt, medmindre en
anden værdi er Deny. Gruppepolitikker undersøges ikke; Allow i registreringsdatabasen
beviser ikke, at en virkelig optagelse virker. En DLL, der kan indlæses, beviser ikke GPU/driverfunktionalitet.
Lydopslagets fejl kan både betyde manglende enhed og mislykket navneopslag, så
resultatet er ukendt ved fejl, og enhedens navn eksporteres aldrig.
Genvej leveres som `NativeProbe { shortcut: Some((modifier_flags, virtual_key)) }`.
CLI kender ikke brugerens binding og rapporterer derfor ukendt. Win32-fejl 1409
betyder optaget genvej; andre registreringsfejl er ukendte. RegisterHotKey tester
ikke konflikter mellem tastaturhooks. UIPI er et øjebliksbillede; utilgængelige
proces-tokens giver ukendt. Udklipsholderens ejer/navn og indhold læses aldrig.
På Linux/macOS svarer de native Windows-kontroller ikke relevant.

## Backend og simulering

`recommended_backend(&probe)` er en ren funktion: GPU anbefales kun ved bekræftet
AVX2 og Vulkan. Ellers anbefales CPU med en stabil årsag. Rapporten har både
anbefalet backend og valgt Whisper-backend; en CPU-kompilering bliver på CPU.
På Windows anvendes beslutningen ved Whispers faktiske oprettelse af kontekst:
`use_gpu=false` og `flash_attn=false`. Den eksisterende beslutningslog viser disse
flag og den oprindeligt kompilerede backend. Andre platformes backend bevares.
Parakeet vælger allerede CPUExecutionProvider; ONNX-fejlregistreringen ændres ikke.

- `TINGSKRIVER_SIMULATE_NO_VULKAN=1`: Vulkan-kontrollen svarer mangler.
- `TINGSKRIVER_SIMULATE_NO_AVX2=1`: AVX2-kontrollen svarer mangler.

Kun værdien `1` aktiverer simulering. Flagene påvirker kun diagnose og Whispers
backend-valg, også ved test på Linux/macOS. Andre miljøvariabler læses ikke af
modulet. Testene indsprøjter værdier uden at ændre processens miljø.

**Begrænsning:** Windows-distributionens native Whisper er bygget med AVX2.
`use_gpu=false` gør ikke den native CPU-kode portabel til CPU'er uden AVX2.
Vulkan er desuden en kompileret feature: hvis en distributions DLL-import kræver
Vulkan inden Rust-main, kan denne runtime-beslutning ikke afhjælpe det.
Et sikkert opstarts- og transskriptionsløfte for disse maskiner kræver en separat
portabel CPU-build/dynamisk backend og kontrol af distributionens DLL-importer.
Denne ændring ændrer ikke build/release eller ONNX/VAD-bibliotekernes CPU-krav.

## Data til UI og eksport

`Report::collect(&probe)` tager et snapshot gennem `SystemProbe`.
`Finding` indeholder stabilt `id`, `state`, `severity`, dansk `title` og
`explanation` samt en serialiserbar `Action`. UI kan oversætte handlingen til en
knap og skal selv bede brugeren om at aktivere den. Diagnosen udfører ingen
rettelseshandlinger. OK-fund har `Action::None`; ukendte fund har en manuel
rettelsesmulighed. Ikke relevante kontroller udelades fra fundlisten.

`to_text()` og `to_json()` eksporterer kun OS-type, numerisk OS-version,
AVX2-tilstand, backend-beslutning, faste fundtekster/handlinger og UTC-start/slut.
Ingen lyd, transskription, sti, brugernavn, miljøudskrift, procesnavn eller nøgle
kan afleveres via probe-grænsefladen. Eksporttesten placerer genkendelig lyd,
transskription og brugernavnssti ved siden af rapporten og kontrollerer begge
formater. Rapporten skrives kun til stdout, aldrig til den almindelige applog.

## Manuel Windows-test

1. Kør begge diagnoseformater på en almindelig Windows-brugerkonto. Kontrollér
   tidsstempler, OS-build, JSON og exitkode. Kør også med stdout til fil.
2. Sæt hvert simuleringsflag til `1`, et ad gangen og sammen. Kør diagnosen:
   det tilsvarende fund skal være fejl, exitkode 1 og CPU valgt. Fjern flagene.
3. På en CPU med AVX2 og Vulkan: start appen med hvert flag, indlæs en lokal
   Whisper-model, og kontrollér `use_gpu=false` og `flash_attn=false` i loggen.
   Gentag uden flag for at kontrollere den almindelige backend.
4. Afvis mikrofonadgang i Windows, gentag diagnosen og gendan tilladelsen.
   Slå derefter kun adgangen for skrivebordsapps fra med rodadgangen tilladt:
   `permission.microphone` skal vise fejl. Gendan desktop-adgangen og kontrollér
   OK, hvis alle fire værdier er Allow, ellers ukendt.
   Afbryd standardafspilningsenheden og kontrollér ukendt/OK efter gentilkobling.
5. Hold udklipsholderen åben fra et testprogram, og kontrollér fejlfundet. Luk
   testprogrammet, og kontrollér positivt at kontrollen igen er OK.
6. Test en udtrykkeligt oplyst genvej via NativeProbe: først ledig, derefter
   reserveret af et testprogram. CLI uden binding skal vise ukendt.
7. Aktivér et program som administrator, og kør diagnosen fra en almindelig
   konsol. UIPI skal være fejl eller ukendt ved adgangsafvisning. Gentag med et
   almindeligt program aktivt; UIPI skal være OK. Fokus må ikke skifte under testen.
8. På en ren VM uden Vulkan og på fysisk CPU uden AVX2: undersøg opstart og
   transskription med distributionsbuild. Disse hardware-/loader-tests er ikke
   dækket af Linux-tests eller af simuleringsflagene.
