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
}

macro_rules! parameterized {
    ($($function:ident => $template:ident;)*) => {
        $(pub fn $function(value: &str) -> String { $template.replace("{value}", value) })*
    };
}
parameterized! {
    version => VERSION;
    special_characters => SPECIAL;
    appearance => APPEARANCE_DETAIL;
    data_dir => DATA_DIR;
    database => DATABASE;
    log_file => LOG_FILE;
}

#[cfg(test)]
mod tests {
    #[test]
    fn templates_replace_only_the_parameter() {
        assert_eq!(super::version("1.2.3"), "Version 1.2.3");
        assert_eq!(super::database("{value}"), "Database: {value}");
    }
}
