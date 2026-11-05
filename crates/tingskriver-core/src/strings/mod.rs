//! The Danish UI catalogue. Every constant and template is registered in ALL.
macro_rules! catalogue {
    ($($name:ident = $text:literal;)*) => {
        $(pub const $name: &str = $text;)*
        pub const ALL: &[&str] = &[$($name),*];
    };
}

catalogue! {
    APP_NAME = "Tingskriver";
    MEETINGS = "Møder";
    DICTATION = "Diktering";
    SETTINGS = "Indstillinger";
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
    LOADING_MEETINGS = "Indlæser møder …";
    NO_MEETINGS = "Ingen møder endnu";
    FIRST_MEETING = "Når du har optaget dit første møde, står det her.";
    NO_MEETINGS_YET = "Ingen møder at vise endnu";
    MEETINGS_LATER = "Mødelisten kommer hertil i en senere version. Indtil da finder du dine møder i den nuværende app.";
    DICTATION_LATER = "Hold Ctrl+Win nede, tal, og slip. Teksten skrives der, hvor markøren står. Diktering flytter hertil i en senere version.";
    PROBE = "Prøvefelt";
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
    APPEARANCE = "Udseende";
    APPEARANCE_DETAIL = "Følger Windows' indstilling for lys og mørk tilstand. Lige nu: {value}.";
    DATA = "Data";
    DATA_DIR = "Datamappe: {value}";
    DATABASE = "Database: {value}";
    LOG = "Log";
    LOG_FILE = "Logfil: {value}";
    OPEN_LOG = "Åbn logmappen";
    QUIT = "Afslut Tingskriver";
    LOG_FAILED = "Logfilen kunne ikke oprettes, så detaljerne står kun i konsollen.";
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
    PROPOSAL_DIFF_REMOVED = "Fjernet";
    PROPOSAL_DIFF_UNCHANGED = "Uændret";
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
    PROPOSAL_INVALID = "Forslagets indhold kunne ikke læses.";
    PROPOSAL_FAILED = "Forslaget kunne ikke gemmes eller anvendes. Intet er ændret.";
}

macro_rules! parameterized {
    ($($function:ident => $template:ident;)*) => {
        $(pub fn $function(value: &str) -> String { $template.replace("{value}", value) })*
    };
}
parameterized! {
    measure_errors => MEASURE_ERRORS;
    measure_wer => MEASURE_WER;
    measure_ms => MEASURE_MS;
    version => VERSION;
    special_characters => SPECIAL;
    appearance => APPEARANCE_DETAIL;
    data_dir => DATA_DIR;
    database => DATABASE;
    log_file => LOG_FILE;
    proposals_many => PROPOSALS_MANY;
    proposal_source => PROPOSAL_SOURCE;
    proposal_stale_reason => PROPOSAL_STALE_REASON;
}

/// "1 ventende ændring" / "N ventende ændringer".
pub fn proposals_banner(count: usize) -> String {
    if count == 1 {
        PROPOSALS_ONE.to_string()
    } else {
        proposals_many(&count.to_string())
    }
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

#[cfg(test)]
mod tests {
    #[test]
    fn templates_replace_only_the_parameter() {
        assert_eq!(super::version("1.2.3"), "Version 1.2.3");
        assert_eq!(super::database("{value}"), "Database: {value}");
    }
}
