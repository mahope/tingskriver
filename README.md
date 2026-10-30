<p align="center"><img src="docs/logo.png" alt="Tingskriver" width="96"></p>

# Tingskriver

## Dansk

Tingskriver optager og transskriberer møder på din Windows-computer, og alt kører lokalt. Lyd, transskription og resuméer forlader ikke maskinen, medmindre du selv vælger en ekstern AI-tjeneste. Programmet er lavet til danske brugere, er gratis og open source, og der er ingen Pro-version eller betalingsmur.

Projektside: <https://tingskriver.mahoje.dk>

![Mødeside i lyst udseende](docs/screenshots/moedeside-lys.png)

| | |
|---|---|
| ![Optagelse i gang](docs/screenshots/optagelse-i-gang-lys.png) | ![Mødeside i mørkt udseende](docs/screenshots/moedeside-moerk.png) |
| ![Indstillinger](docs/screenshots/indstillinger-lys.png) | ![Opsætning](docs/screenshots/onboarding-model-lys.png) |

Skærmbillederne er taget med mock-data af `frontend/scripts/screenshots.cjs`.

### Installation

1. Hent installationsfilen (`.exe`) fra [den seneste udgivelse](https://github.com/mahope/tingskriver/releases/latest).
2. Kør filen. Installationsfilen er ikke signeret, så Windows SmartScreen viser en advarsel. Vælg "Flere oplysninger" og derefter "Kør alligevel".
3. Programmet opdaterer sig selv fra GitHub Releases.

Tingskriver understøtter kun Windows 11 (x64). Udgivelserne indeholder ikke macOS- eller Linux-versioner, selvom koden fra det oprindelige projekt stadig ligger i repoet.

### Diktering

Hold `Ctrl+Win`, tal, og slip. Teksten bliver renskrevet og skrevet ind dér, hvor markøren står, i alle programmer. To hurtige tryk dikterer håndfrit, til du trykker igen. En lille pille nederst på skærmen viser lydniveauet og "Skriver …", og den tager aldrig fokus fra det program, du skriver i. Genvejen kan ændres under Indstillinger → Diktering.

- **Tale til tekst:** Med en Syv.ai-nøgle bruges Syv.ai (dansk). Fejler Syv.ai, eller svarer den ikke inden for 2 sekunder, bruges den lokale Parakeet-model automatisk. Uden nøgle bruges kun den lokale model.
- **Renskrivning:** øh og altså fjernes, og du kan sige tegn: "punktum", "komma", "spørgsmålstegn", "udråbstegn", "kolon", "ny linje" og "nyt afsnit". Sætninger får stort begyndelsesbogstav. I kodeprogrammer og terminaler (fx VS Code og Windows Terminal) skrives teksten råt. Polering med en sprogmodel kan slås til, men er slået fra som standard.
- **Ordbog og snippets:** Egne ord og navne staves rigtigt, og en talt udløser som "mit CVR" kan indsætte en fast tekst.
- **Indsættelse:** Teksten indsættes via udklipsholderen og `Ctrl+V`, og din udklipsholder sættes tilbage bagefter. Kan udklipsholderen ikke bruges, eller kan `Ctrl+V` ikke sendes, skrives teksten tegn for tegn i stedet. Felter, der selv afviser indsæt, får ingen tekst, men den kan kopieres fra historikken, hvis den er slået til. Under en mødeoptagelse er mødets mikrofonspor stille, mens du dikterer, så dikteringen ikke kommer med i mødet.
- **Historik:** De seneste 500 dikteringer står på siden "Diktering" med svartid og program og kan kopieres igen. Lyden gemmes i 24 timer. Begge dele kan slås fra.

**Privatliv:** Med Syv.ai sendes kun lydoptagelsen af det, du dikterer, sammen med ordene i din ordbog som stikord. Uden nøgle, eller med "Kun lokal model", forlader intet computeren. Nøglen gemmes i Windows Legitimationsstyring, ikke i Tingskrivers filer. Genvejen bruger én tastaturhook, der kun holder øje med genvejens taster. Tastetryk gemmes eller logges aldrig. Programmer, der kører som administrator, kan ikke modtage tekst fra Tingskriver (Windows' UIPI), så kør ikke Tingskriver som administrator.

| | |
|---|---|
| ![Diktering](docs/screenshots/diktering-lys.png) | ![Indstillinger for diktering](docs/screenshots/indstillinger-diktering-moerk.png) |

![Pillen under diktering](docs/screenshots/diktering-pille.png)

### Skift af mikrofon og mute

Under en optagelse følger Tingskriver Windows' standardenheder. Optagelsen skifter kun mikrofon, når den aktive mikrofon reelt fejler:

- enheden fjernes eller deaktiveres (fx headsettet tages ud eller Bluetooth falder ud),
- Windows vælger en anden standardenhed,
- lydstrømmen melder en fejl (fx `AUDCLNT_E_DEVICE_INVALIDATED`),
- lydstrømmen leverer slet ingen lyd-frames i 2 sekunder.

Mute skifter aldrig mikrofon. Det gælder både mute-knappen på headsettet, mute i Windows og lav lydstyrke: en muted mikrofon leverer stadig frames, bare med stilhed, og optagelsen bliver på den. Loggen skriver én linje med "silent frames ignored", hver gang en stille periode begynder, og "delivers sound again", når der er lyd igen. Når headsettet kommer igen efter en reel fejl, skifter optagelsen tilbage til det.

### Om projektet

Tingskriver er en fork af [Meetily](https://github.com/Zackriya-Solutions/meetily) fra Zackriya Solutions (MIT-licens). Tingskriver har ingen tilknytning til Zackriya Solutions og er hverken godkendt eller sponsoreret af dem. Se [NOTICE.md](NOTICE.md), [LICENSE.md](LICENSE.md) og [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md) for licenser og tredjepartskomponenter, herunder FFmpeg (GPL-3.0).

### Udvikling

- Udviklingen foregår her på GitHub: [mahope/tingskriver](https://github.com/mahope/tingskriver). Issues og pull requests er velkomne.
- Et tag på formen `v*` bygger en udgivelse på GitHub Actions og lægger installationsfilen på GitHub Releases.
- Pushes til andre grene end `main` bygger uden at lave en udgivelse.
- Byggevejledning: [docs/BUILDING.md](docs/BUILDING.md). Frontenden ligger i `frontend/` (Tauri 2, Next.js, Rust).
- Logfiler: `%LOCALAPPDATA%\dk.mahoje.tingskriver\logs\` (`tingskriver.log`, roterer ved 5 MB, de to seneste filer gemmes også). Åbn mappen fra Indstillinger under "Logfiler". Enhedsskift står som `[DEVICE_FOLLOW]`-linjer med tidsstempel.
- Test af diktering: `cargo test -p tingskriver-core dictation` (renskrivning, genvej, dobbelttryk, reservevalg, SQLite-lager og Syv.ai-kaldet mod en falsk server; kører på Linux og i CI).

- Test af enhedsskift: `rustc --edition 2021 --test crates/tingskriver-core/src/audio/device_follow.rs -o /tmp/df && /tmp/df` (ren std, kører på Linux; det samme gør CI på hver gren). Manuel test på Windows: optag, tag headsettet af og på igen, og tjek at `audio.mp4` ikke har huller og at `[DEVICE_FOLLOW]`-linjerne i loggen viser skiftet inden for ca. 1 sekund. Tryk også på mute på headsettet i mindst 30 sekunder under optagelsen og tjek, at der ikke kommer noget skift.

## English

Tingskriver is a privacy-first meeting recorder and transcriber for Windows that runs locally. Audio, transcripts and summaries stay on your machine unless you choose an external AI service yourself. It is made for Danish users, is free and open source, and has no Pro version or paywall.

Project page: <https://tingskriver.mahoje.dk>

![Meeting page, light theme](docs/screenshots/moedeside-lys.png)

| | |
|---|---|
| ![Recording in progress](docs/screenshots/optagelse-i-gang-moerk.png) | ![Meeting page, dark theme](docs/screenshots/moedeside-moerk.png) |

The screenshots use mock data and are generated by `frontend/scripts/screenshots.cjs`. The interface is in Danish.

### Installation

1. Download the installer (`.exe`) from the [latest release](https://github.com/mahope/tingskriver/releases/latest).
2. Run it. The installer is unsigned, so Windows SmartScreen shows a warning. Choose "More info" and then "Run anyway".
3. The app updates itself from GitHub Releases.

Tingskriver supports Windows 11 (x64) only. Releases contain no macOS or Linux builds, although the code inherited from the upstream project is still in the repository.

### Dictation

Hold `Ctrl+Win`, speak, and release. The text is cleaned up and typed where your cursor is, in any program. Double-tap to dictate hands-free until the next press. A small pill at the bottom of the screen shows the input level and "Skriver …" (writing), and never takes focus from the program you are typing in. The shortcut can be changed under Settings → Diktering.

- **Speech to text:** With a Syv.ai key, Syv.ai (Danish) is used. If Syv.ai fails or takes longer than 2 seconds, the local Parakeet model is used automatically. Without a key, only the local model is used.
- **Cleanup:** Danish fillers ("øh", "altså") are removed and spoken punctuation ("punktum", "komma", "spørgsmålstegn", "udråbstegn", "kolon", "ny linje", "nyt afsnit") becomes characters. Sentences are capitalised. Code editors and terminals get raw text. Optional LLM polishing is off by default.
- **Dictionary and snippets:** Your own words and names are spelled correctly, and a spoken trigger such as "mit CVR" can insert a fixed block of text.
- **Insertion:** Text is pasted through the clipboard with `Ctrl+V` and your clipboard is restored afterwards. If the clipboard can't be used or `Ctrl+V` can't be sent, the text is typed character by character instead. Fields that refuse paste themselves get no text, but it can be copied from the history when that is on. During a meeting recording the meeting's microphone track is silent while you dictate, so dictation stays out of the meeting.
- **History:** The last 500 dictations are listed on the "Diktering" page with latency and program, and can be copied again. Audio is kept for 24 hours. Both can be turned off.

**Privacy:** With Syv.ai, only the audio of what you dictate is sent, along with your dictionary words as hints. Without a key, or with "local model only", nothing leaves the computer. The key is stored in Windows Credential Manager, not in Tingskriver's files. The shortcut uses one keyboard hook that only watches the shortcut keys; keystrokes are never stored or logged. Programs running as administrator cannot receive text from Tingskriver (Windows UIPI), so do not run Tingskriver as administrator.

### Microphone switching and mute

During a recording Tingskriver follows the Windows default devices. It only switches microphone when the active microphone actually fails:

- the device is removed or disabled (e.g. the headset is unplugged or Bluetooth drops),
- Windows picks another default device,
- the audio stream reports an error (e.g. `AUDCLNT_E_DEVICE_INVALIDATED`),
- the audio stream delivers no audio frames at all for 2 seconds.

Mute never switches microphone. That covers the mute button on the headset, mute in Windows and low volume: a muted microphone still delivers frames, they just contain silence, and the recording stays on it. The log writes one "silent frames ignored" line each time a silent stretch begins, and "delivers sound again" when sound returns. When the headset comes back after a real failure, the recording switches back to it.

### About

Tingskriver is a fork of [Meetily](https://github.com/Zackriya-Solutions/meetily) by Zackriya Solutions (MIT License). Tingskriver is not affiliated with, endorsed by or sponsored by Zackriya Solutions. See [NOTICE.md](NOTICE.md), [LICENSE.md](LICENSE.md) and [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md) for licenses and third-party components, including FFmpeg (GPL-3.0).

### Development

- Development happens here on GitHub: [mahope/tingskriver](https://github.com/mahope/tingskriver). Issues and pull requests are welcome.
- Pushing a `v*` tag builds a release on GitHub Actions and publishes the installer on GitHub Releases.
- Pushes to branches other than `main` build without creating a release.
- Build instructions: [docs/BUILDING.md](docs/BUILDING.md). The app lives in `frontend/` (Tauri 2, Next.js, Rust).
- Log files: `%LOCALAPPDATA%\dk.mahoje.tingskriver\logs\` (`tingskriver.log`, rotates at 5 MB, the two previous files are kept). Open the folder from Settings under "Logfiler". Device switches appear as `[DEVICE_FOLLOW]` lines with timestamps.
- Testing dictation: `cargo test -p tingskriver-core dictation` (cleanup, shortcut, double tap, fallback decisions, the SQLite store and the Syv.ai request against a fake server; runs on Linux and in CI).

- Testing device switching: `rustc --edition 2021 --test crates/tingskriver-core/src/audio/device_follow.rs -o /tmp/df && /tmp/df` (plain std, runs on Linux; CI runs it on every branch). Manual test on Windows: record, take the headset off and put it on again, and check that `audio.mp4` has no gaps and the `[DEVICE_FOLLOW]` lines in the log show the switch within about a second. Also press mute on the headset for at least 30 seconds while recording and check that no switch happens.
