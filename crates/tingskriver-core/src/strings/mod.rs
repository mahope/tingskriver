//! The Danish UI catalogue. Every constant and template is registered in ALL.
macro_rules! catalogue {
    ($($name:ident = $text:literal;)*) => {
        $(pub const $name: &str = $text;)*
        pub const ALL: &[&str] = &[$($name),*];
    };
}

catalogue! {
    LOCAL_ONLY_ENABLED = "Kun lokal er slået til";
    DELIVERY_PRIVACY_BLOCKED = "Kun lokal er slået til, eller privat tilstand blokerer aflevering";
    ABOUT = "Om";
    ABOUT_BUILD = "Byggedato";
    ABOUT_CHANNEL = "Kanal";
    ABOUT_STABLE = "stabil";
    ABOUT_BETA = "beta";
    ABOUT_PLATFORM = "Platform";
    ABOUT_UPDATES = "Opdateringer";
    ABOUT_UPDATES_UNAVAILABLE = "Opdateringer er ikke aktiveret i denne build";
    ABOUT_COPYING = "Kopierer diagnoseoplysninger …";
    ABOUT_COPIED = "Diagnoseoplysningerne er kopieret.";
    ABOUT_COPY_FAILED = "Kunne ikke kopiere diagnoseoplysninger. Prøv igen.";
    ABOUT_COPY = "Kopiér diagnoseoplysninger";
    ABOUT_GPU = "GPU-status";
    ABOUT_COMMIT = "Commit";
    ABOUT_MODELS = "Whisper: {whisper}\nParakeet: {parakeet}";
    ABOUT_MODEL_NOT_LOADED = "ikke indlæst";
    ABOUT_LICENSE = "Licens";
    ABOUT_THIRD_PARTY = "Tredjepartslicenser";
    ABOUT_NOTICE = "Tredjepartsattribution";
    ABOUT_PRIVACY = "Privatlivspolitik";
    ABOUT_CONTACT = "Kontakt: Mahoje (mahoje.dk)";
    // Job 8: the About dialog, copied from the Tauri app
    // (frontend/src/components/About.tsx, UpdateDialog.tsx, UpdateNotification.tsx).
    ABOUT_TAGLINE = "Referater der aldrig forlader din computer.";
    ABOUT_DIFFERENT = "Det adskiller Tingskriver";
    ABOUT_LOCAL_TITLE = "Lokalt fra start til slut";
    ABOUT_LOCAL_TEXT = "Optagelse, transskription og opsummering sker på din egen computer. Ingen sky, ingen lækager.";
    ABOUT_MODEL_TITLE = "Vælg din egen model";
    ABOUT_MODEL_TEXT = "Brug en lokal åben model, eller tilslut en ekstern udbyder. Du er ikke låst fast.";
    ABOUT_PRICE_TITLE = "Ingen pris pr. minut";
    ABOUT_PRICE_TEXT = "Kører modellerne lokalt, betaler du ikke for tid. Bruger du en ekstern udbyder, betaler du kun for de kald, du selv vælger.";
    ABOUT_MEETINGS_TITLE = "Virker med dine møder";
    ABOUT_MEETINGS_TEXT = "Google Meet, Zoom og Teams, og møder uden internet.";
    // The upstream project is named in NOTICE.md, not in the UI (danish_ui.rs).
    ABOUT_OPEN_SOURCE = "Gratis og open source. Tingskriver bygger på et projekt fra Zackriya Solutions (MIT-licens), se NOTICE.md.";
    ABOUT_NOTICE_LINK = "NOTICE.md";
    ABOUT_WEBSITE = "tingskriver.mahoje.dk";
    ABOUT_GITHUB = "GitHub";
    ABOUT_LINK_SEPARATOR = "·";
    ABOUT_BUILD_DETAILS = "Byggeoplysninger og diagnose";
    ABOUT_LEGAL = "Licenser og privatliv";
    UPDATE_CHECK = "Søg efter opdateringer";
    UPDATE_LATEST = "Du har den nyeste version";
    UPDATE_CHECK_FAILED = "Kunne ikke søge efter opdateringer. Tjek din internetforbindelse og prøv igen.";
    UPDATE_READY_LINE = "Opdatering klar: version {value}";
    UPDATE_READY = "Opdatering klar";
    UPDATE_READY_DETAIL = "Version {value} er hentet";
    UPDATE_AT_RESTART = "Opdateringen installeres automatisk, næste gang Tingskriver starter.";
    UPDATE_DOWNLOADING = "Henter opdatering";
    UPDATE_DOWNLOADING_DETAIL = "Henter den nyeste version…";
    UPDATE_FAILED = "Opdatering mislykkedes";
    UPDATE_FAILED_DETAIL = "Noget gik galt under opdateringen";
    UPDATE_CURRENT = "Nuværende version:";
    UPDATE_NEW = "Ny version:";
    UPDATE_DETAILS = "Se detaljer";
    UPDATE_CLOSE = "Luk";
    APP_NAME = "Tingskriver";
    MEETING_DELETE = "Slet";
    MEETING_NEW = "Ny optagelse";
    MEETING_REFRESH = "Opdater";
    MEETING_RECOVERY = "En optagelse blev afbrudt. Gendan den gemte lyd her.";
    MEETING_DELETE_CONFIRM = "Vil du slette mødet og dets tekst? Dette kan ikke fortrydes. Lydfilerne beholdes på disken.";
    MEETING_SCAN_FAILED = "Efterladte optagelser kunne ikke undersøges. Prøv at opdatere listen. Dine lydfiler er ikke ændret.";
    MEETING_LOAD_FAILED = "Møder eller efterladte optagelser kunne ikke indlæses. Prøv at opdatere listen.";
    // The sidebar, copied from the Tauri app (frontend/src/components/Sidebar/index.tsx).
    SIDEBAR_LABEL = "Hovedmenu";
    SIDEBAR_COLLAPSE = "Fold sidepanelet sammen";
    SIDEBAR_EXPAND = "Fold sidepanelet ud";
    SIDEBAR_SHOW_MEETINGS = "Vis mødelisten";
    SIDEBAR_RECORDING = "Optagelse i gang";
    SIDEBAR_FIRST_MEETING = "Tryk på \"Ny optagelse\", så dukker dit første møde op her.";
    ABOUT_APP = "Om Tingskriver";
    SEARCH_PLACEHOLDER = "Søg i møder";
    SEARCH_LABEL = "Søg i møder og transskripter";
    SEARCH_CLEAR = "Ryd søgning";
    SEARCHING = "Søger…";
    SEARCH_NO_MATCH = "Ingen møder matcher \"{value}\". Prøv et andet ord.";
    SEARCH_FAILED = "Søgningen kunne ikke gennemføres. Listen viser kun møder, hvis titel matcher.";
    DAY_TODAY = "I dag";
    DAY_YESTERDAY = "I går";
    IMPORT_TRANSCRIPT = "Importér transskript";
    IMPORT_TRANSCRIPT_TOOLTIP = "Importér transskript (.vtt, .srt)";
    IMPORT_AUDIO = "Importér lyd";
    IMPORT_LATER = "Import er endnu ikke flyttet til denne app. Importér lyd eller transskript i den nuværende app.";
    MEETING_RENAME_TITLE = "Omdøb møde";
    MEETING_RENAME_LABEL = "Omdøb mødet {value}";
    MEETING_DELETE_LABEL = "Slet mødet {value}";
    MEETING_NAME = "Mødets navn";
    MEETING_NAME_PLACEHOLDER = "Skriv et navn";
    MEETING_NAME_REQUIRED = "Mødet skal have et navn";
    MEETING_RENAMED = "Mødet er omdøbt";
    MEETING_RENAME_FAILED = "Mødet kunne ikke omdøbes";
    MEETING_DELETE_TITLE = "Bekræft sletning";
    MEETING_DELETED = "Mødet er slettet";
    MEETING_DELETE_FAILED = "Mødet kunne ikke slettes";
    DIALOG_CANCEL = "Annullér";
    DIALOG_SAVE = "Gem";
    // The idle home screen (frontend/src/app/_components/TranscriptPanel.tsx).
    HOME_TITLE = "Klar til dit næste møde";
    HOME_LEAD = "Tingskriver optager og skriver referat på din computer. Lyden bliver her, og du kan læse noterne bagefter.";
    HOME_HINT = "Tryk på \"Start optagelse\" nedenfor. Transskriptionen laves, når du stopper og gemmer optagelsen.";
    START_RECORDING = "Start optagelse";
    MEETINGS = "Møder";
    TRANSCRIPT_TRUNCATED = "Den nye transskription er væsentligt kortere end den forrige og er ikke gjort aktuel. Den er gemt som version, og den forrige er stadig i brug.";
    DICTATION = "Diktering";
    RERUN_TITLE = "Kør sidste igen";
    RERUN_CHANGED = "Teksten er ændret.";
    RERUN_UNCHANGED = "Teksten er uændret.";
    RERUN_BEFORE = "Før";
    RERUN_AFTER = "Efter";
    RERUN_CLOSE = "Luk";
    SETTINGS = "Indstillinger";
    // Settings frame and the Generelt tab (frontend/src/app/settings/page.tsx,
    // PreferenceSettings.tsx, ThemeSelector.tsx, delivery/DeliverySettings.tsx,
    // mcp/McpSettings.tsx).
    SETTINGS_BACK = "Tilbage";
    SETTINGS_TABS = "Indstillingsfaner";
    SETTINGS_TAB_GENERAL = "Generelt";
    SETTINGS_TAB_RECORDING = "Optagelser";
    SETTINGS_TAB_TRANSCRIPTION = "Transskription";
    SETTINGS_TAB_SUMMARY = "Opsummering";
    SETTINGS_TAB_BETA = "Beta";
    SETTINGS_MOVES_LATER = "Kommer i en senere version.";
    APPEARANCE_HELP = "Vælg et lyst eller mørkt udseende, eller lad Tingskriver følge din computers indstilling.";
    APPEARANCE_SYSTEM = "Følg systemet";
    APPEARANCE_LIGHT = "Lyst";
    APPEARANCE_DARK = "Mørkt";
    APPEARANCE_SAVE_FAILED = "Udseendet kunne ikke gemmes: {value}";
    APPEARANCE_LOAD_FAILED = "Det gemte udseende kunne ikke læses: {value}";
    NOTIFICATIONS_TITLE = "Notifikationer";
    NOTIFICATIONS_HELP = "Få en besked, når en optagelse starter og stopper.";
    NOTIFICATIONS_LABEL = "Notifikationer om start og slut";
    NOTIFICATIONS_HINT = "Slå fra, hvis du ikke vil forstyrres under møder.";
    STORAGE_TITLE = "Placering af data";
    STORAGE_HELP = "Her gemmer Tingskriver dine optagelser på din computer.";
    STORAGE_RECORDINGS = "Mødeoptagelser";
    STORAGE_UNKNOWN = "Placeringen kunne ikke hentes. Genstart Tingskriver, og prøv igen.";
    STORAGE_OPEN = "Åbn mappen";
    STORAGE_LOGS = "Logfiler";
    STORAGE_LOGS_HELP = "Appens log, fx skift af lydenhed under en optagelse. Send mappen, hvis noget fejler.";
    STORAGE_OPEN_LOGS = "Åbn logmappe";
    STORAGE_FOOTER = "Databasen og modellerne ligger sammen i programmets datamappe, så alt er samlet ét sted.";
    CRASH_DUMPS = "Gem nedbrudsfiler";
    CRASH_DUMPS_HINT = "Går Tingskriver ned, gemmer Windows en lille fil i mappen dumps ved siden af loggen. Den bliver på din computer, indtil du selv sender den.";
    CRASH_DUMPS_FAILED = "Indstillingen kunne ikke gemmes. Prøv igen.";
    DELIVERY_TITLE = "Aflevering af referat";
    DELIVERY_HELP = "Opret et mål, og godkend referatet på mødesiden før aflevering. Mappe og SSH får transskript, referat, noter og en metadatafil. Kanal-webhooks får et afkortet referat.";
    DELIVERY_LOAD_FAILED = "Afleveringsindstillingerne kunne ikke læses. Prøv at opdatere.";
    DELIVERY_REFRESH = "Opdatér";
    DELIVERY_ACTION_FAILED = "Handlingen mislykkedes. Kontrollér mål, adgang og privatindstillinger, og prøv igen.";
    DELIVERY_WORKING = "Arbejder …";
    DELIVERY_PRIVACY = "Privatliv";
    DELIVERY_LOCAL_ONLY = "Kun lokal. Blokér alle afleveringer, også mapper";
    DELIVERY_PRIVATE = "Privat tilstand. Sæt afleveringskøen på pause";
    DELIVERY_TARGETS = "Dine mål";
    DELIVERY_NO_TARGETS = "Du har ingen mål. Opret et nedenfor for at komme i gang.";
    DELIVERY_TEST = "Test mål";
    DELIVERY_TESTED = "Testen er lagt i kø uden mødedata. Se resultatet nedenfor.";
    DELIVERY_REMOVE = "Fjern mål";
    DELIVERY_REMOVE_CONFIRM = "Fjern målet {value}? Ventende afleveringer til målet kan ikke sendes.";
    DELIVERY_ADD = "Tilføj mål";
    DELIVERY_NAME = "Navn på målet";
    DELIVERY_KIND = "Måltype";
    DELIVERY_KIND_FOLDER = "Mappe";
    DELIVERY_KIND_WEBHOOK = "Webhook";
    DELIVERY_KIND_SSH = "SSH med SFTP";
    DELIVERY_PATH = "Fuld sti til mappe";
    DELIVERY_URL = "Webhook-adresse. Gemmes i systemets nøglelager";
    DELIVERY_FORMAT = "Format";
    DELIVERY_FORMAT_JSON = "Generisk JSON med alle dokumenter";
    DELIVERY_FORMAT_SLACK = "Slack";
    DELIVERY_FORMAT_DISCORD = "Discord";
    DELIVERY_FORMAT_TEAMS = "Teams-kanal";
    DELIVERY_HOST = "Servernavn";
    DELIVERY_PORT = "Port";
    DELIVERY_USER = "Brugernavn";
    DELIVERY_KEY_PATH = "Fuld sti til OpenSSH-nøgle";
    DELIVERY_REMOTE_DIR = "Uploadmappe på serveren, fx /upload";
    DELIVERY_SSH_HELP = "Serverens værtsnøgle skal være godkendt på forhånd. Brug en nøgle med adgang kun til upload.";
    DELIVERY_SAVE = "Gem mål";
    DELIVERY_SAVED = "Målet er gemt. Intet er sendt.";
    DELIVERY_QUEUE = "Afleveringskø";
    DELIVERY_PAUSED = "Køen er sat på pause af dine privatindstillinger.";
    DELIVERY_NO_JOBS = "Ingen afleveringer endnu.";
    DELIVERY_JOB = "{target}: {status}. Antal forsøg: {attempts}.";
    DELIVERY_REMOVED_TARGET = "Fjernet mål";
    DELIVERY_NEXT_ATTEMPT = "Næste forsøg: {value}";
    DELIVERY_TIME_FORMAT = "%-d.%-m.%Y %H.%M.%S";
    DELIVERY_RETRY = "Prøv igen";
    DELIVERY_STATUS_AWAITING = "Afventer godkendelse i mødet";
    DELIVERY_STATUS_PENDING = "Venter på aflevering";
    DELIVERY_STATUS_RUNNING = "Afleverer";
    DELIVERY_STATUS_COMPLETED = "Afleveret";
    DELIVERY_STATUS_FAILED = "Kræver handling";
    MCP_ACCESS_LABEL = "Adgangsniveau";
    MCP_ACCESS_HELP = "Ved Kun markerede møder deles nye møder ikke automatisk. Markér dem på mødesiden. Kun metadata viser titler, datoer, varighed og opgaver. Dikteringer og optagelseshandlinger kræver Alt. Andre handlinger kræver Alt eller et markeret møde og en åben app.";
    MCP_SETUP_LABEL = "Opsætning til {value}";
    MCP_CLIENT_CLAUDE = "Claude Code";
    MCP_CLIENT_CODEX = "Codex";
    MCP_TITLE = "AI-værktøjer (MCP)";
    MCP_PRIVACY = "MCP er slået fra som standard. Når du aktiverer det, kan en klient læse de møder og datadele, du tillader. Data kan derefter forlade computeren via AI-værktøjets udbyder. Alt giver også adgang til dikteringer og optagelseshandlinger. Handlinger kan ændre noter, danne referater og aflevere data via appens valgte mål. Aflevering kræver alle datadele. Slukning afbryder nye kald, men tilbagekalder ikke allerede delte data eller igangværende handlinger.";
    MCP_ENABLE = "Aktivér MCP-serveren";
    MCP_DISABLED = "Stoppet: MCP-adgang er slået fra.";
    MCP_AVAILABLE = "Stoppet og klar: klienten starter serveren via stdio. Ingen netværksport.";
    MCP_ACTIVE = "Kører: en klient har startet MCP-serveren via stdio. Ingen netværksport.";
    MCP_RUNNING = "Kørende klientprocesser ved seneste statusopdatering: {value}. Ingen netværksport. Slukning blokerer nye datakald; klienten ejer processen.";
    MCP_MISSING = "Fejl: MCP-programmet mangler. Geninstallér Tingskriver.";
    MCP_LOADING = "Indlæser MCP-indstillinger …";
    MCP_LOAD_FAILED = "MCP-indstillingerne kunne ikke læses. Åbn appen igen og prøv igen.";
    MCP_PATH = "Programsti: {value}";
    MCP_DATA_HELP = "Tilladte datadele. Valgene gælder også søgning og opgavelisten. Deltagere styrer talerfelter i transskripter; navne skrevet i noter eller transskripttekst fjernes ikke automatisk.";
    MCP_NOTES = "Noter";
    MCP_SUMMARY = "Referat og opgaver";
    MCP_TRANSCRIPT = "Transskript";
    MCP_PARTICIPANTS = "Deltagere og talere";
    MCP_SHARED_COUNT = "Delte møder: {value}";
    MCP_SHARE_HELP = "Del et møde med MCP ved at åbne det i sidebjælken.";
    MCP_TEST = "Test forbindelsen";
    MCP_TESTING = "Tester forbindelsen …";
    MCP_TEST_OK = "Forbindelsen virker. Serveren svarede med værktøjslisten. Intet mødeindhold blev læst. Testprocessen er stoppet igen.";
    MCP_TOOLS = "Serverens værktøjer";
    MCP_LOG = "Seneste 100 kald. Kun værktøj, tid, møde-id og resultat gemmes lokalt.";
    MCP_LOG_EMPTY = "Ingen registrerede kald. Opdatér listen, efter en klient har brugt serveren.";
    MCP_LOG_REFRESH = "Opdatér status og kaldslog";
    MCP_LOG_CLEAR = "Ryd kaldslog";
    MCP_LOG_CLEARED = "Kaldsloggen er ryddet.";
    MCP_LOG_ENTRY = "{time} · {tool} · møde-id: {id} · {result}";
    MCP_OK = "lykkedes";
    MCP_FAILED = "fejlede";
    MCP_NO_ID = "intet";
    MCP_SHARE = "Del med AI-værktøjer";
    MCP_UNSHARE = "Stop deling med AI-værktøjer";
    MCP_REFRESH = "Opdatér mødelisten";
    MCP_MEETING_NOT_LISTED = "Mødet er ikke i delingslisten endnu. Opdatér mødelisten for at kunne dele det.";
    MCP_MARKED = "Kun markerede møder";
    MCP_METADATA = "Kun metadata";
    MCP_ALL = "Alt";
    MCP_OFF = "Fra";
    MCP_SAVED = "MCP-adgang er gemt.";
    MCP_SAVE_FAILED = "MCP-adgang kunne ikke gemmes. Prøv igen.";
    MCP_COPY_CLAUDE = "Kopiér opsætning til Claude Code";
    MCP_COPY_CODEX = "Kopiér opsætning til Codex";
    MCP_COPIED = "MCP-opsætningen er kopieret.";
    MCP_ACTION = "AI-værktøj: {value}";
    MCP_ACTION_UNKNOWN = "handling";
    MCP_START_RECORDING = "Start optagelse";
    MCP_STOP_RECORDING = "Stop optagelse";
    MCP_REGENERATE = "Dan referat igen";
    MCP_HANDOFF = "Aflever referat";
    MCP_ADD_NOTE = "Tilføj note";
    MCP_CLAUDE_COMMAND = "claude mcp add tingskriver -- \"{value}\"";
    MCP_CODEX_CONFIG = "[mcp_servers.tingskriver]\ncommand = {value}\n";
    MEASURE_TITLE = "Måling";
    MEASURE_HELP = "Sammenlign dansk nøjagtighed på din egen stemme. Importér cirka 60 sekunder som WAV (mono, 16 kHz, PCM16, højst 90 sekunder), og skriv præcis det, du siger.";
    MEASURE_PATH = "Sti til lydprøve";
    MEASURE_PATH_HINT = "C:\u{005c}prøver\u{005c}prøve.wav";
    MEASURE_REFERENCE = "Facit";
    MEASURE_REFERENCE_HINT = "Skriv de ord, der bliver sagt i prøven.";
    MEASURE_MODELS = "Vælg udbydere og modeller";
    MEASURE_PRIVACY = "Ingen modeller er valgt på forhånd. Lyden sendes kun til de tjenester, du vælger. Hver model måles separat uden reserveudbyder. Facit sendes aldrig.";
    MEASURE_REMOTE = "modtager lyd over netværket";
    MEASURE_LOCAL = "kun på computeren";
    MEASURE_RATE = "Din tarif i DKK pr. lydtime (valgfri, uden strøm og hardware)";
    MEASURE_RATE_HINT = "Fx 12,50. Tomt betyder ukendt pris.";
    MEASURE_RELOAD = "Genindlæs udbyderindstillinger";
    MEASURE_SELECT = "Vælg:";
    MEASURE_SELECTED = "Valgt:";
    MEASURE_START = "Kør valgte modeller";
    MEASURE_RUNNING = "Måler … Vent, til alle valgte modeller er kørt.";
    MEASURE_INTERRUPTED = "Målingen blev afbrudt. Prøv igen.";
    MEASURE_SAVED = "Målingen er afsluttet. Resultatet er gemt lokalt.";
    MEASURE_RESULTS = "Resultater";
    MEASURE_MODEL_LABEL = "{provider}: {model} ({location})";
    MEASURE_ERRORS = "{value} ordfejl";
    MEASURE_WER = "WER {value} %";
    MEASURE_MS = "{value} ms";
    MEASURE_PRICE = "{value} {currency}/time";
    MEASURE_RESULT_LINE = "{wer}, {errors}, {latency}, {price}";
    MEASURE_PRICE_UNKNOWN = "pris ukendt";
    MEASURE_HISTORY = "Tidligere resultater ligger som JSON i datamappens measurements-mappe. Der gemmes kun måletal, ingen lyd eller facit. Svartid omfatter transskription og netværk, men ikke indlæsning af lokale modeller. Prisen er din angivne tarif, ikke en fakturamåling.";
    MEASURE_OPEN = "Åbn gemte resultater";
    MEASURE_NO_FOLDER = "Datamappen kunne ikke findes.";
    MEASURE_CHOOSE = "Vælg mindst én udbyder og model.";
    MEASURE_INVALID_RATE = "Angiv timeprisen som et tal, fx 12,50.";
    VERSION = "Version {value}";
    PROBE_LINE = "Skriv fx æøå, é, ü, ê, @ og € her";
    PROBE_TEXT = "Flere linjer: prøv også IME-komposition, fx Windows' emoji-panel (Win+.)";
    INTERRUPTED_BY_SLEEP = "Mødet blev afsluttet, fordi computeren gik i dvale.";
    INTERRUPTED_BY_LOGOFF = "Mødet blev afsluttet, fordi Windows blev logget af eller lukket ned.";
    INTERRUPTED_TITLE = "Et møde blev afsluttet af systemet";
    INTERRUPTED_WHEN = "Tidspunkt: {value}";
    INTERRUPTED_AUDIO_OK = "Lyden er gemt.";
    INTERRUPTED_AUDIO_PARTIAL = "Lyden er delvist gemt. De rå spor er bevaret og kan gendannes.";
    INTERRUPTED_AUDIO_FAILED = "Lyden kunne ikke færdigbehandles. De rå spor er bevaret.";
    INTERRUPTED_AUDIO_UNKNOWN = "Lyden nåede ikke at blive bekræftet. Appen tjekker den, næste gang den starter.";
    INTERRUPTED_NO_RESTART = "Der starter ikke en ny optagelse af sig selv.";
    INTERRUPTED_DISMISS = "Forstået";
    LOADING_MEETINGS = "Indlæser møder …";
    NO_MEETINGS = "Ingen møder endnu";
    NO_MEETINGS_YET = "Ingen møder at vise endnu";
    MEETINGS_LATER = "Mødelisten kommer hertil i en senere version. Indtil da finder du dine møder i den nuværende app.";
    // The Diktering page, copied from the Tauri app
    // (frontend/src/components/dictation/DictationHistory.tsx).
    DICTATION_SETTINGS = "Indstillinger";
    DICTATION_HOLD = "Hold {value}, tal, og slip. Teksten bliver skrevet ind dér, hvor markøren står. Tryk to gange hurtigt for at diktere uden at holde tasterne nede, og tryk igen, når du er færdig.";
    DICTATION_ONLY_WINDOWS = "Genvejen og indsættelse virker kun på Windows. Historik, ordbog og snippets kan stadig redigeres.";
    DICTATION_DISABLED = "Diktering er slået fra under Indstillinger.";
    DICTATION_NO_ENGINE = "Der er hverken en Syv.ai-nøgle eller en lokal model. Tilføj en af dem under Indstillinger.";
    DICTATION_READY = "Genvejen er klar. Transskriberes med {value}.";
    DICTATION_ENGINE_LOCAL = "den lokale model";
    DICTATION_ENGINE_SYV = "Syv.ai (lokal model som reserve)";
    DICTATION_HISTORY = "Seneste dikteringer";
    DICTATION_CLEAR = "Ryd historik";
    DICTATION_CLEAR_CONFIRM = "Klik igen for at rydde alt";
    DICTATION_HISTORY_OFF = "Historikken er slået fra, så nye dikteringer gemmes ikke.";
    DICTATION_EMPTY = "Ingen dikteringer endnu";
    DICTATION_EMPTY_HINT = "Stil markøren i et tekstfelt, hold {value} og sig noget. Så dukker det op her.";
    DICTATION_COPY = "Kopiér";
    DICTATION_COPY_LABEL = "Kopiér teksten";
    DICTATION_DELETE_LABEL = "Slet dikteringen";
    DICTATION_LATENCY = "Tid fra du slap tasterne, til teksten var indsat";
    DICTATION_FALLBACK = "reserve brugt";
    DICTATION_ENGINE_SYV_SHORT = "Syv.ai";
    DICTATION_ENGINE_LOCAL_SHORT = "Lokal model";
    DICTATION_COPIED = "Teksten er kopieret";
    DICTATION_LOAD_FAILED = "Historikken kunne ikke hentes: {value}";
    DICTATION_LOAD_RETRYING = "Siden prøver igen af sig selv om et øjeblik.";
    DICTATION_LOAD_GAVE_UP = "Åbn siden igen for at prøve igen.";
    DICTATION_DELETE_FAILED = "Dikteringen kunne ikke slettes";
    DICTATION_CLEARED = "Historikken er ryddet";
    DICTATION_CLEAR_FAILED = "Historikken kunne ikke ryddes";
    // GPUI only: the keyboard test field folds away under the history.
    PROBE = "Test tastatur";
    PROBE_HELP = "Brug felterne til at teste tastaturet: æøå, dødtaster (^ ¨ ~ ´ `) efterfulgt af en vokal, AltGr-tegn (@ £ $ € { [ ] } | \\) og IME-komposition.";
    SPECIAL_EMPTY = "Særlige tegn vises her med deres Unicode-nummer.";
    SPECIAL = "Særlige tegn: {value}";
    DARK = "mørk";
    LIGHT = "lys";
    UNKNOWN = "ukendt";
    DB_OPENING = "åbnes …";
    DB_NOT_CREATED = "ikke oprettet endnu";
    DB_READY = "klar";
    DB_FAILED = "kunne ikke åbnes";
    NO_LOG = "ingen logfil (kun konsollen)";
    SETTINGS_LATER = "Indstillingerne flytter hertil i en senere version. Her kan du se, hvordan appen kører.";
    VOICEPRINT_TITLE = "Stemmeaftryk";
    VOICEPRINT_STATUS_OFF = "Slået fra. Tingskriver husker ingen stemmer.";
    // Beta tab (Job 6): there is no switch to turn voiceprints on yet.
    VOICEPRINT_NO_SWITCH = "Der er endnu ingen kontakt til at slå stemmeaftryk til.";
    VOICEPRINT_STATUS_ON = "Slået til. Tingskriver husker kun stemmer for personer, du har samtykke fra.";
    VOICEPRINT_WHAT = "Et stemmeaftryk er en række tal, der beskriver, hvordan en bestemt stemme lyder. Med et aftryk kan Tingskriver foreslå navnet på en deltager, der går igen, så du ikke skal skrive det hver gang.";
    VOICEPRINT_LAW = "Et stemmeaftryk kan genkende en person og er derfor biometriske oplysninger. De har særlig stærk beskyttelse i GDPR (artikel 9), så du må kun gemme dem med personens samtykke.";
    VOICEPRINT_LOCAL = "Aftrykkene ligger kun på denne pc, i Windows Legitimationsstyring. De sendes aldrig nogen steder hen, synkroniseres ikke og indgår ikke i eksport, aflevering, logfiler, diagnoserapporter eller sikkerhedskopier.";
    VOICEPRINT_CONSENT = "Samtykke: Du må kun gemme en persons aftryk, når personen er informeret om det og har sagt ja. Du bekræfter det i appen for hver person. Samtykket kan trækkes tilbage når som helst, og så slettes personens aftryk med det samme.";
    VOICEPRINT_DELETE = "Sletning: Du kan glemme én person eller alle på én gang. Aftryk, der ikke er genkendt i 30, 90 eller 180 dage (du vælger, standard er 90), slettes automatisk. Afinstallerer du med \"slet data\", fjernes de også.";
    VOICEPRINT_SUGGEST = "Tingskriver navngiver aldrig automatisk. Du får et forslag, som du bekræfter med ét klik. Grænserne for, hvornår to stemmer ligner hinanden nok, er målt på engelsk og er ikke kalibreret til dansk. Forslag kan derfor være forkerte.";
    APPEARANCE = "Udseende";
    DATA = "Data";
    DATA_DIR = "Datamappe: {value}";
    DATABASE = "Database: {value}";
    LANGUAGES = "Sprog";
    LANGUAGE_PRIMARY = "Hovedsprog";
    LANGUAGE_PRIMARY_HELP = "Bruges til referat og chat og er altid med som transskriptionssprog. Standard er dansk.";
    LANGUAGE_EXTRA = "Ekstra talesprog";
    LANGUAGE_EXTRA_HELP = "Sprog, der også tales i møderne, fx engelsk. De giver transskriptionen kontekst.";
    LANGUAGE_PROVIDERS = "Hovedsproget og de ekstra talesprog sendes til Syv.ai og andre netudbydere som sprog og kontekst. Den lokale model bruger kun hovedsproget.";
    LANGUAGE_NEW_MEETINGS = "Ændringer gælder nye optagelser og nye referater. Et eksisterende transskript oversættes ikke. Vil du have et referat på det nye sprog, så lav referatet om.";
    LANGUAGE_SAVE_FAILED = "Sprogvalget kunne ikke gemmes: {value}";
    LANGUAGE_CURRENT = "Valgt nu: hovedsprog {value}";
    LANGUAGE_NO_EXTRA = "ingen ekstra";
    LANGUAGE_EXTRA_LIST = "ekstra talesprog: {value}";
    LANGUAGE_LOAD_FAILED = "Sprogindstillingerne kunne ikke læses: {value}";
    LOG = "Log";
    LOG_FILE = "Logfil: {value}";
    OPEN_LOG = "Åbn logmappen";
    QUIT = "Afslut Tingskriver";
    LOG_FAILED = "Logfilen kunne ikke oprettes, så detaljerne står kun i konsollen.";
    MODEL_HASH_MISMATCH = "Modelfilen {value} har ikke den forventede kontrolsum (SHA-256) og er slettet. Prøv at hente den igen.";
    MODEL_INSTALLED_MISMATCH = "Modelfilen {value} har ikke den forventede kontrolsum (SHA-256) og indlæses ikke. Slet modellen, og hent den igen.";
    MODEL_DOWNLOAD_FAILED = "Modelfilen {value} kunne ikke hentes. Tjek forbindelsen, og prøv igen.";
    DATABASE_FAILED = "Tingskriver kunne ikke åbne dine møder";
    RETRY = "Prøv igen";
    PROPOSALS_TITLE = "Ventende ændringer";
    PROPOSALS_ONE = "1 ventende ændring";
    PROPOSALS_MANY = "{value} ventende ændringer";
    PROPOSALS_EMPTY = "Ingen ventende ændringer";
    PROPOSALS_EMPTY_HELP = "Når en assistent eller et værktøj foreslår en ændring af noter eller referat, kan du gennemgå den her. Intet ændres, før du trykker Anvend.";
    PROPOSALS_REVIEW_HELP = "Gennemgå forslaget linje for linje. Dine noter og dit referat er uændrede, indtil du trykker Anvend.";
    PROPOSALS_LOADING = "Indlæser forslag …";
    PROPOSALS_LOAD_FAILED = "Forslagene kunne ikke indlæses.";
    PROPOSAL_FIELD_NOTES = "Noter";
    PROPOSAL_FIELD_SUMMARY = "Referat";
    PROPOSAL_SOURCE = "Foreslået af: {value}";
    PROPOSAL_SOURCE_REGENERATE = "genskabt referat";
    PROPOSAL_SOURCE_ASSISTANT = "assistenten";
    PROPOSAL_SOURCE_CHAT = "chatten";
    PROPOSAL_SOURCE_COMMAND = "kommandotilstand";
    PROPOSAL_SOURCE_MCP = "et eksternt værktøj (MCP)";
    PROPOSAL_APPLY = "Anvend";
    PROPOSAL_REJECT = "Afvis";
    PROPOSAL_UNDO = "Fortryd";
    PROPOSAL_STATUS_PENDING = "Venter";
    PROPOSAL_STATUS_APPLIED = "Anvendt";
    PROPOSAL_STATUS_REJECTED = "Afvist";
    PROPOSAL_STATUS_STALE = "Forældet";
    PROPOSAL_STATUS_EXPIRED = "Udløbet";
    PROPOSAL_STATUS_UNDONE = "Fortrudt";
    PROPOSAL_DIFF_ADDED = "Tilføjet";
    PROPOSAL_SYMBOL_ADDED = "+";
    PROPOSAL_SYMBOL_REMOVED = "−";
    PROPOSAL_SYMBOL_UNCHANGED = " ";
    PROPOSAL_LEGEND = "+ Tilføjet   − Fjernet";
    PROPOSAL_TITLE = "{field}: {status}";
    PROPOSAL_DIFF_REMOVED = "Fjernet";
    PROPOSAL_NO_LINES = "Ingen linjer";
    PROPOSAL_STALE_REASON = "Forslaget blev afvist automatisk, fordi {value} er ændret, siden forslaget blev lavet. Bed om et nyt forslag.";
    PROPOSAL_STALE_NOTES = "noterne";
    PROPOSAL_STALE_SUMMARY = "referatet";
    PROPOSAL_EXPIRED_REASON = "Forslaget udløb, da samtalen blev lukket.";
    PROPOSAL_UNDO_CONFLICT = "Fortryd kan ikke gennemføres, fordi du har ændret de samme linjer, efter forslaget blev anvendt. Intet er ændret.";
    PROPOSAL_NO_CHANGE = "Forslaget ændrer ikke noget.";
    PROPOSAL_NOT_PENDING = "Forslaget er allerede behandlet.";
    PROPOSAL_NOT_APPLIED = "Kun et anvendt forslag kan fortrydes.";
    PROPOSAL_NOT_FOUND = "Forslaget findes ikke længere.";
    PROPOSAL_EMPTY_SUMMARY = "Et tomt referat kan ikke anvendes.";
    PROPOSAL_NO_SUMMARY = "Mødet har intet referat at ændre endnu.";
    PROPOSAL_UNSUPPORTED_SUMMARY = "Referatets format understøtter ikke ændringsforslag.";
    PROPOSAL_INVALID = "Forslagets indhold kunne ikke læses.";
    PROPOSAL_FAILED = "Forslaget kunne ikke gemmes eller anvendes. Intet er ændret.";
    PROMPT_MEETING_TITLE = "Er du i et møde?";
    PROMPT_MEETING_BODY = "Tingskriver kan optage mødet og lave referat bagefter.";
    PROMPT_RECORD = "Optag";
    PROMPT_IGNORE = "Ignorér";
    PROMPT_REMINDER_TITLE = "Mødet starter om {value}";
    PROMPT_REMINDER_RECORD = "Optag mødet";
    PROMPT_REMINDER_LATER = "Ikke nu";
    PROMPT_END_TITLE = "Er mødet slut?";
    PROMPT_END_BODY = "Optagelsen stopper om {value}, hvis du ikke svarer.";
    PROMPT_END_STOP = "Stop optagelsen";
    PROMPT_END_KEEP = "Fortsæt";
    PROMPT_KEYS_HINT = "Ctrl+Alt+Enter vælger den første knap, Ctrl+Alt+Esc lukker, og Ctrl+Alt+P flytter fokus til vinduet. Så virker Tab, Enter og Esc.";
    PROMPT_QUIET_MARK = "Stille tilstand: spørgsmål udsat";
    PROMPT_WINDOW_NAME = "Tingskriver spørger";
    // Job 3 (docs/design-paritet.md): the home and recording screen. GPUI
    // only: why recording cannot start here yet, and the name of the
    // transcript list. The rest is verbatim from Tauri RecordingControls.tsx,
    // RecordingStatusBar.tsx, TranscriptPanel.tsx,
    // VirtualizedTranscriptView.tsx, StatusOverlays.tsx and
    // RecordingStateContext.tsx.
    RECORDING_UNAVAILABLE = "Der kan ikke optages, fordi Tingskriver ikke fandt sin datamappe. Genstart appen, og prøv igen.";
    RECORDING_TRANSCRIPT_LABEL = "Transskription";
    RECORDING_STATUS_LABEL = "{state} {clock}, Mikrofon: {microphone}, Systemlyd: {system}";
    RECORDING_PAUSE = "Pause";
    RECORDING_RESUME = "Genoptag";
    RECORDING_STOP = "Stop og gem";
    RECORDING_ACTIVE = "Optager";
    RECORDING_PAUSED = "På pause";
    RECORDING_MICROPHONE = "Mikrofon:";
    RECORDING_SYSTEM = "Systemlyd:";
    RECORDING_DEFAULT_MICROPHONE = "Standardmikrofon";
    RECORDING_DEFAULT_SYSTEM = "Standard for systemlyd";
    RECORDING_MICROPHONE_LOST = "ingen (kun systemlyd)";
    RECORDING_LISTENING = "Lytter …";
    RECORDING_EMPTY_LISTENING = "Lytter efter tale …";
    RECORDING_EMPTY_LISTENING_HINT = "Tal, så vises transskriptionen her";
    RECORDING_EMPTY_PAUSED = "Optagelsen er sat på pause";
    RECORDING_EMPTY_PAUSED_HINT = "Vælg Genoptag for at fortsætte optagelsen";
    RECORDING_COPY = "Kopiér";
    RECORDING_COPY_TITLE = "Kopiér transskriptionen";
    RECORDING_COPIED = "Transskriptet er kopieret";
    RECORDING_LOW_CONFIDENCE = "Lav sikkerhed: ordene kan være opfattet forkert";
    RECORDING_NOTICE_DISMISS = "Luk beskeden";
    RECORDING_ERROR_DISMISS = "Luk besked";
    RECORDING_SAVING = "Gemmer optagelsen…";
    RECORDING_FINISHING = "Gør transskriptionen færdig…";
    RECORDING_SAVING_MEETING = "Gemmer mødet…";
    RECORDING_DEVICE_SWITCHED = "Mikrofonen er skiftet til {value}. Optagelsen fortsætter som normalt.";
    RECORDING_DEVICE_SWITCH_FAILED = "Vi kunne ikke skifte til {value}. Optagelsen fortsætter på den forrige mikrofon, hvis den stadig virker.";
    RECORDING_DEVICE_LOST = "Mikrofonen {value} kunne ikke genoprettes. Der optages kun systemlyd. Stop og start optagelsen igen for at rette det.";
    RECORDING_ERROR_MICROPHONE_TITLE = "Mikrofonen er ikke tilgængelig";
    RECORDING_ERROR_MICROPHONE = "Tingskriver kan ikke bruge din mikrofon. Tjek at:\n• mikrofonen er sluttet til\n• Tingskriver har adgang til mikrofonen i Windows under Indstillinger > Anonymitet og sikkerhed\n• ingen anden app bruger mikrofonen";
    RECORDING_ERROR_SYSTEM_TITLE = "Systemlyd er ikke tilgængelig";
    RECORDING_ERROR_SYSTEM = "Tingskriver kan ikke optage lyden fra computeren. Tjek at:\n• der er valgt en lydenhed til systemlyd under Indstillinger\n• lydenheden er tændt i Windows";
    RECORDING_ERROR_PERMISSION_TITLE = "Tilladelse mangler";
    RECORDING_ERROR_PERMISSION = "Tingskriver skal have adgang til at optage. Giv adgang til mikrofonen i Windows-indstillingerne, og genstart derefter Tingskriver.";
    RECORDING_ERROR_START_TITLE = "Optagelsen kunne ikke starte";
    RECORDING_ERROR_START = "Tjek lydenhederne under Indstillinger, og prøv igen.";
    // ---- Recording in the GPUI app (RecordingControls.tsx, useRecordingStart.ts,
    // useRecordingStop.ts, RecordingStateContext.tsx) ----
    RECORDING_STARTING = "Gør klar…";
    RECORDING_PAUSE_FAILED = "Optagelsen kunne ikke sættes på pause";
    RECORDING_PAUSE_FAILED_HELP = "Prøv igen. Optagelsen kører stadig.";
    RECORDING_RESUME_FAILED = "Optagelsen kunne ikke genoptages";
    RECORDING_RESUME_FAILED_HELP = "Prøv igen, eller stop og gem optagelsen.";
    RECORDING_STOP_FAILED = "Optagelsen kunne ikke stoppes";
    RECORDING_STOP_FAILED_HELP = "Optagelsen kører stadig. Prøv igen.";
    RECORDING_START_WHILE_SAVING = "Den forrige optagelse gemmes stadig. Prøv igen om et øjeblik.";
    RECORDING_MIC_UNAVAILABLE = "Ingen mikrofon tilgængelig. Der optages kun systemlyd.";
    RECORDING_SAVED = "Optagelsen er gemt";
    RECORDING_SAVE_FAILED = "Mødet kunne ikke gemmes";
    RECORDING_TRANSCRIBE_FAILED = "Mødet er gemt, men lyden kunne ikke transskriberes";
    RECORDING_DEFAULT_TITLE = "Møde {date} {time}";
    RECORDING_STOPPED_BY_ERROR = "Optagelsen stoppede på grund af en fejl i lydenheden. Lyden indtil da er gemt.";
    // No transcript is made while recording (the audio is saved first), so
    // the empty transcript says what happens instead of "Lytter efter tale".
    RECORDING_EMPTY_RECORDING = "Optager mødet …";
    RECORDING_EMPTY_RECORDING_HINT = "Lyden gemmes løbende. Transskriptionen laves, når du vælger Stop og gem.";
    // ---- Gendan afbrudte møder (TranscriptRecovery.tsx, useTranscriptRecovery.ts) ----
    RECOVERY_TITLE = "Gendan afbrudte møder";
    RECOVERY_FOUND_ONE = "Vi fandt 1 afbrudt møde. Vælg et møde for at se et uddrag og gendanne det.";
    RECOVERY_FOUND_MANY = "Vi fandt {value} afbrudte møder. Vælg et møde for at se et uddrag og gendanne det.";
    RECOVERY_LIST = "Afbrudte møder";
    RECOVERY_PREVIEW = "Uddrag";
    RECOVERY_SELECT = "Vælg et møde for at se et uddrag";
    RECOVERY_LAST_WRITTEN = "Sidst gemt {value}";
    RECOVERY_DURATION = "{value} lyd";
    RECOVERY_HAS_AUDIO = "Lyd findes";
    RECOVERY_TRACKS_BOTH = "Mikrofon og systemlyd";
    RECOVERY_TRACKS_MICROPHONE = "Kun mikrofon";
    RECOVERY_TRACKS_SYSTEM = "Kun systemlyd";
    RECOVERY_NO_TRANSCRIPT = "Der er intet transskript at vise. Lyden transskriberes, når mødet er gendannet.";
    RECOVERY_CANCEL = "Annullér";
    RECOVERY_DELETE = "Slet";
    RECOVERY_DELETING = "Sletter…";
    RECOVERY_RESTORE = "Gendan";
    RECOVERY_RESTORING = "Gendanner…";
    RECOVERY_OPEN = "Gendan afbrudte møder…";
    RECOVERY_CONFIRM_DELETE = "Vil du slette dette møde? Det kan ikke fortrydes.";
    RECOVERY_RESTORED = "Mødet er gendannet";
    RECOVERY_RESTORED_AUDIO = "Lyden er gendannet og gemt i mødets mappe.";
    RECOVERY_RESTORED_PARTIAL = "Lyden er delvist gendannet. Dele af optagelsen kunne ikke reddes.";
    RECOVERY_RESTORE_FAILED = "Mødet kunne ikke gendannes. Prøv igen, og genstart appen hvis det fortsætter.";
    RECOVERY_DELETE_FAILED = "Mødet kunne ikke slettes. Prøv igen.";
    RECOVERY_DELETED = "Mødet er slettet";
    // ---- Job 4: the meeting document (frontend/src/components/MeetingDetails) ----
    DOC_LOADING = "Henter mødet …";
    DOC_LOAD_FAILED = "Mødet kunne ikke indlæses. Prøv at åbne det igen.";
    DOC_RENAME = "Omdøb mødet";
    DOC_SHARED = "Må deles med AI-værktøjer";
    DOC_SHARING_LOAD_FAILED = "Deling kunne ikke læses. Åbn mødet igen for at prøve igen.";
    DOC_SHARING_SAVE_FAILED = "Deling kunne ikke gemmes. Prøv igen.";
    DOC_REGENERATE = "Lav om";
    DOC_GENERATE = "Lav opsummering";
    DOC_LANGUAGE_AUTO = "Automatisk";
    DOC_LANGUAGE_LABEL = "Vælg opsummeringssprog";
    DOC_MODEL = "AI-model";
    DOC_TEMPLATE = "Skabelon";
    DOC_SAVE = "Gem";
    DOC_COPY = "Kopiér";
    DOC_COPY_SUMMARY = "Kopiér opsummeringen";
    DOC_SUMMARY_COPIED = "Opsummeringen er kopieret";
    DOC_NO_SUMMARY_TO_COPY = "Der er ingen opsummering at kopiere";
    DOC_EMPTY_TITLE = "Ingen opsummering endnu";
    DOC_EMPTY_TEXT = "Lad AI lave en opsummering af mødets transskript med hovedpunkter, handlepunkter og beslutninger.";
    DOC_SUMMARY_RUNNING = "Skriver opsummeringen…";
    DOC_SUMMARY_RUNNING_HELP = "Det kan tage et par minutter for lange møder.";
    DOC_SUMMARY_FAILED = "Referatet kunne ikke laves. Mødet og transskriptet er gemt.";
    DOC_SUMMARY_RETRY = "Lav referat igen";
    DOC_SUMMARY_INVALID = "Det gemte referat mangler eller kan ikke læses. Lav referatet igen.";
    DOC_DELIVERY_TITLE = "Aflever referatet til";
    DOC_DELIVERY_LATER = "Aflevering af referatet er endnu ikke flyttet til denne app.";
    DOC_TRANSCRIPT = "Transskript";
    DOC_LINES_NONE = "Ingen linjer";
    DOC_LINES_ONE = "1 linje";
    DOC_LINES_MANY = "{value} linjer";
    DOC_SHOW = "Vis";
    DOC_HIDE = "Skjul";
    DOC_COPY_TRANSCRIPT = "Kopiér transskriptet";
    DOC_NO_TRANSCRIPT_TO_COPY = "Der er intet transskript at kopiere";
    DOC_TRANSCRIPT_COPIED = "Transskriptet er kopieret";
    DOC_RECORDING = "Optagelse";
    DOC_OPEN_RECORDING = "Åbn mappen med optagelsen";
    DOC_NO_AUDIO = "Mødet har ingen lyd, kun et importeret transskript.";
    DOC_IMPROVE = "Forbedr";
    DOC_TRANSCRIPT_EMPTY = "Ingen transskription";
    DOC_TRANSCRIPT_EMPTY_HELP = "Der blev ikke genkendt tale i denne optagelse.";
    DOC_NUMBERED = "{value}.";
    DOC_ACTIONS = "Handlinger for opsummeringen";
    DOC_SUMMARY_ACTIONS = "Gem og kopiér";
    DOC_TRANSCRIPT_ACTIONS = "Handlinger for transskriptet";
    // ---- end of Job 4 block ----
}

macro_rules! parameterized {
    ($($function:ident => $template:ident;)*) => {
        $(pub fn $function(value: &str) -> String { $template.replace("{value}", value) })*
    };
}
parameterized! {
    recovery_last_written => RECOVERY_LAST_WRITTEN;
    recovery_duration => RECOVERY_DURATION;
    mcp_claude_command => MCP_CLAUDE_COMMAND;
    mcp_codex_config => MCP_CODEX_CONFIG;
    meeting_rename_label => MEETING_RENAME_LABEL;
    meeting_delete_label => MEETING_DELETE_LABEL;
    search_no_match => SEARCH_NO_MATCH;
    measure_errors => MEASURE_ERRORS;
    measure_wer => MEASURE_WER;
    measure_ms => MEASURE_MS;
    version => VERSION;
    update_ready_line => UPDATE_READY_LINE;
    update_ready_detail => UPDATE_READY_DETAIL;
    special_characters => SPECIAL;
    appearance_save_failed => APPEARANCE_SAVE_FAILED;
    appearance_load_failed => APPEARANCE_LOAD_FAILED;
    delivery_remove_confirm => DELIVERY_REMOVE_CONFIRM;
    delivery_next_attempt => DELIVERY_NEXT_ATTEMPT;
    mcp_setup_label => MCP_SETUP_LABEL;
    dictation_hold => DICTATION_HOLD;
    dictation_ready => DICTATION_READY;
    dictation_empty_hint => DICTATION_EMPTY_HINT;
    dictation_load_failed => DICTATION_LOAD_FAILED;
    data_dir => DATA_DIR;
    database => DATABASE;
    log_file => LOG_FILE;
    interrupted_when => INTERRUPTED_WHEN;
    proposals_many => PROPOSALS_MANY;
    proposal_source => PROPOSAL_SOURCE;
    proposal_stale_reason => PROPOSAL_STALE_REASON;
    prompt_reminder_title => PROMPT_REMINDER_TITLE;
    prompt_end_body => PROMPT_END_BODY;
    model_hash_mismatch => MODEL_HASH_MISMATCH;
    model_installed_mismatch => MODEL_INSTALLED_MISMATCH;
    model_download_failed => MODEL_DOWNLOAD_FAILED;
    language_save_failed => LANGUAGE_SAVE_FAILED;
    language_load_failed => LANGUAGE_LOAD_FAILED;
    language_current => LANGUAGE_CURRENT;
    language_extra_list => LANGUAGE_EXTRA_LIST;
    // Job 3: device notices during a recording.
    recording_device_switched => RECORDING_DEVICE_SWITCHED;
    recording_device_switch_failed => RECORDING_DEVICE_SWITCH_FAILED;
    recording_device_lost => RECORDING_DEVICE_LOST;
}

pub fn about_models(whisper: &str, parakeet: &str) -> String {
    ABOUT_MODELS
        .replace("{whisper}", whisper)
        .replace("{parakeet}", parakeet)
}

/// "1 ventende ændring" / "N ventende ændringer".
pub fn proposals_banner(count: usize) -> String {
    if count == 1 {
        PROPOSALS_ONE.to_string()
    } else {
        proposals_many(&count.to_string())
    }
}

/// The number of a numbered summary item: "1.".
pub fn numbered_item(number: u32) -> String {
    DOC_NUMBERED.replace("{value}", &number.to_string())
}

/// "Ingen linjer", "1 linje", "1.234 linjer" (Tauri `MeetingDocument`).
pub fn transcript_lines(count: usize) -> String {
    match count {
        0 => DOC_LINES_NONE.to_string(),
        1 => DOC_LINES_ONE.to_string(),
        n => DOC_LINES_MANY.replace("{value}", &crate::format_da::integer(n as i64)),
    }
}

pub fn proposal_title(field: &str, status: &str) -> String {
    PROPOSAL_TITLE
        .replace("{field}", field)
        .replace("{status}", status)
}

/// The status bar of a recording as a screen reader reads it.
pub fn recording_status_label(state: &str, clock: &str, microphone: &str, system: &str) -> String {
    RECORDING_STATUS_LABEL
        .replace("{state}", state)
        .replace("{clock}", clock)
        .replace("{microphone}", microphone)
        .replace("{system}", system)
}

/// The name a new recording gets: "Møde 29. okt. 2026 20.31" (Tauri
/// `defaultMeetingTitle`).
pub fn recording_default_title(at: chrono::NaiveDateTime) -> String {
    RECORDING_DEFAULT_TITLE
        .replace("{date}", &crate::format_da::short_date(at.date()))
        .replace("{time}", &crate::format_da::clock(at.time()))
}

/// The name of a recording that starts now.
pub fn recording_title_now() -> String {
    recording_default_title(chrono::Local::now().naive_local())
}

/// "Vi fandt 1 afbrudt møde …" / "Vi fandt N afbrudte møder …".
pub fn recovery_found(count: usize) -> String {
    if count == 1 {
        RECOVERY_FOUND_ONE.to_string()
    } else {
        RECOVERY_FOUND_MANY.replace("{value}", &count.to_string())
    }
}

pub fn mcp_action(action: &str) -> String {
    let label = match action {
        "start_recording" => MCP_START_RECORDING,
        "stop_recording" => MCP_STOP_RECORDING,
        "regenerate_summary" => MCP_REGENERATE,
        "handoff" => MCP_HANDOFF,
        "add_note" => MCP_ADD_NOTE,
        _ => MCP_ACTION_UNKNOWN,
    };
    MCP_ACTION.replace("{value}", label)
}

pub fn measure_model_label(provider: &str, model: &str, location: &str) -> String {
    MEASURE_MODEL_LABEL
        .replace("{provider}", provider)
        .replace("{model}", model)
        .replace("{location}", location)
}

pub fn measure_price(amount: &str, currency: &str) -> String {
    MEASURE_PRICE
        .replace("{value}", amount)
        .replace("{currency}", currency)
}

pub fn measure_result_line(wer: &str, errors: &str, latency: &str, price: &str) -> String {
    MEASURE_RESULT_LINE
        .replace("{wer}", wer)
        .replace("{errors}", errors)
        .replace("{latency}", latency)
        .replace("{price}", price)
}

/// The Diktering settings tab (docs/design-paritet.md, Job 7).
pub mod dictation_settings;
/// Making, stopping and editing a summary; the Opsummering tab's fields.
pub mod meeting_summary;
/// Onboarding (docs/design-paritet.md, Job 9).
pub mod onboarding;
/// The settings tabs Optagelser, Transskription, Opsummering and Beta
/// (docs/design-paritet.md, Job 6).
pub mod settings_tabs;

#[cfg(test)]
mod tests {
    #[test]
    fn templates_replace_only_the_parameter() {
        assert_eq!(super::version("1.2.3"), "Version 1.2.3");
        assert_eq!(super::database("{value}"), "Database: {value}");
    }

    #[test]
    fn recording_titles_match_tauri() {
        let at = chrono::NaiveDate::from_ymd_opt(2026, 10, 29)
            .unwrap()
            .and_hms_opt(20, 31, 5)
            .unwrap();
        assert_eq!(
            super::recording_default_title(at),
            "Møde 29. okt. 2026 20.31"
        );
        assert!(super::recovery_found(1).starts_with("Vi fandt 1 afbrudt møde."));
        assert!(super::recovery_found(3).starts_with("Vi fandt 3 afbrudte møder."));
    }

    #[test]
    fn proposal_banner_uses_singular_and_plural() {
        assert_eq!(super::proposals_banner(1), "1 ventende ændring");
        assert_eq!(super::proposals_banner(3), "3 ventende ændringer");
    }

    #[test]
    fn transcript_line_count_uses_singular_plural_and_grouping() {
        assert_eq!(super::transcript_lines(0), "Ingen linjer");
        assert_eq!(super::transcript_lines(1), "1 linje");
        assert_eq!(super::transcript_lines(10), "10 linjer");
        assert_eq!(super::transcript_lines(1234), "1.234 linjer");
    }
}
