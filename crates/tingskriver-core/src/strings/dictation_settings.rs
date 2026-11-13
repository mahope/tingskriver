//! The Diktering settings tab, copied verbatim from the Tauri app
//! (`frontend/src/components/dictation/DictationSettings.tsx`, line numbers
//! of that file; the shortcut modes from `dictation/strings.ts`).
//!
//! The `{value}` templates have a function below.

catalogue! {
    // Header and load/save results (243-251, 222-228, 122-135).
    INTRO = "Hold genvejen, tal, og slip. Teksten bliver renskrevet og skrevet ind i det program, du arbejder i.";
    ONLY_WINDOWS = "Genvejen og indsættelse virker kun på Windows.";
    LOAD_FAILED = "Indstillingerne for diktering kunne ikke hentes: {value}";
    SAVED = "Indstillingen er gemt";
    SAVE_FAILED = "Indstillingen kunne ikke gemmes";

    // Genvej (254-316; strings.ts).
    SHORTCUT_TITLE = "Genvej";
    ENABLED_LABEL = "Diktering med genvej";
    ENABLED_HINT = "Lyt efter genvejen i alle programmer, også når Tingskriver er skjult";
    SHORTCUT_LABEL = "Tastekombination";
    SHORTCUT_SAVE = "Gem genvej";
    SHORTCUT_HINT = "Fx Ctrl+Win, Ctrl+Alt+Space eller F9. Vælg nedenfor, hvordan genvejen starter og stopper dikteringen.";
    MODE_LABEL = "Genvejstilstand";
    MODE_HOLD = "Hold: tal mens tasterne holdes nede";
    MODE_TOGGLE = "Skift: tryk for at starte og stoppe";
    MODE_HYBRID = "Hybrid: kort tryk låser, langt tryk bruger hold";
    MODE_DOUBLE_TAP = "Dobbelttryk-lås: to hurtige tryk låser";
    HYBRID_LABEL = "Grænse for kort tryk (ms)";
    HYBRID_HELP = "100–1000 ms. Standard er 300 ms. Et kortere tryk låser mikrofonen, indtil du trykker igen.";

    // Mikrofon og lyd (318-390).
    MIC_TITLE = "Mikrofon og lyd";
    MIC_HELP = "Gælder kun diktering. Møder bruger deres egen mikrofon.";
    MIC_LABEL = "Mikrofon";
    MIC_DEFAULT = "Windows' standardmikrofon";
    MIC_MISSING = "{value} (ikke tilsluttet)";
    CHANNEL_LABEL = "Kanal";
    CHANNEL_MIX = "Alle kanaler blandet";
    CHANNEL_LEFT = "Venstre";
    CHANNEL_RIGHT = "Højre";
    MIC_NOTE = "Findes den valgte mikrofon ikke, bruges standardmikrofonen. Kommer der ingen lyd inden for 1 sekund, får du besked.";
    KEEP_OPEN_LABEL = "Hold mikrofonen åben";
    KEEP_OPEN_HINT = "Dikteringen starter hurtigere, men Windows viser mikrofonindikatoren hele tiden, mens Tingskriver kører.";
    SOUND_LABEL = "Lyd ved start og stop";
    SOUND_HINT = "Et kort bip, når mikrofonen er klar, og et, når du slipper. Der spilles intet, hvis mikrofonen ikke giver lyd.";
    VOLUME_LABEL = "Lydstyrke";
    VOLUME_VALUE = "{value} %";
    VOLUME_TEXT = "{value} procent";

    // Transskription (392-454, 137-168).
    TRANSCRIPTION_TITLE = "Transskription";
    TRANSCRIPTION_HELP = "Med en Syv.ai-nøgle bruges Syv.ai. Svarer Syv.ai ikke hurtigt nok, eller fejler det, bruges den lokale model automatisk.";
    ENGINE_LABEL = "Tale til tekst";
    ENGINE_AUTO = "Syv.ai, med lokal model som reserve";
    ENGINE_LOCAL = "Kun lokal model (lyden forlader ikke computeren)";
    NO_LOCAL_MODEL = "Der er ingen lokal Parakeet-model endnu. Hent den under Transskription, så der er en reserve.";
    KEY_LABEL = "Syv.ai-nøgle";
    KEY_STORED = "Der er gemt en nøgle i Windows Legitimationsstyring.";
    KEY_NONE = "Ingen nøgle. Indtil der er en, bruges den lokale model.";
    KEY_REPLACE_PLACEHOLDER = "Indsæt en ny nøgle for at skifte";
    KEY_NEW_PLACEHOLDER = "hv_…";
    KEY_SAVE = "Gem nøgle";
    KEY_TEST = "Test";
    KEY_TESTING = "Tester…";
    KEY_REMOVE = "Fjern";
    KEY_SAVED = "Nøglen er gemt i Windows Legitimationsstyring";
    KEY_SAVE_FAILED = "Nøglen kunne ikke gemmes";
    KEY_REMOVED = "Nøglen er fjernet";
    KEY_REMOVE_FAILED = "Nøglen kunne ikke fjernes";
    SYV_ANSWERS = "Syv.ai svarer";
    SYV_LATENCY = "Svartid {value}";
    SYV_TEST_FAILED = "Syv.ai-testen fejlede";
    FALLBACK_LABEL = "Brug lokal model, hvis Syv.ai er længere end";
    // `formatMs` in `dictation/api.ts`.
    MILLISECONDS = "{value} ms";
    SECONDS = "{value} s";

    // Renskrivning (456-523, 170-176).
    CLEANUP_TITLE = "Renskrivning";
    CLEANUP_HELP = "Fyldord som øh og altså fjernes, og du kan sige tegn: punktum, komma, spørgsmålstegn, udråbstegn, kolon, ny linje og nyt afsnit.";
    PREVIEW_LABEL = "Prøv det";
    PREVIEW_SAMPLE = "øh hej Mads komma kan vi mødes i morgen spørgsmålstegn";
    PREVIEW_SHOW = "Vis";
    PREVIEW_FAILED = "Renskrivningen kunne ikke vises";
    STUTTERS_LABEL = "Saml gentagelser";
    STUTTERS_HINT = "Tre eller flere ens ord i træk bliver til ét: \"nej nej nej\" bliver \"nej\". To ens ord og tal bevares, og det samme gælder ord fra din ordbog.";
    POLISH_LABEL = "Polering med sprogmodel";
    POLISH_HINT = "Retter grammatik og selvrettelser. Tager den mere end 2 sekunder, indsættes teksten uden polering. En lokal model er ofte for langsom.";
    POLISH_PROVIDER = "Udbyder";
    PROVIDER_BUILTIN = "Indbygget lokal model";
    PROVIDER_OLLAMA = "Ollama";
    POLISH_MODEL = "Model";
    POLISH_MODEL_OLLAMA_PLACEHOLDER = "qwen2.5:3b";
    POLISH_MODEL_PLACEHOLDER = "Samme som opsummering";
    OLLAMA_ENDPOINT = "Ollama-adresse";
    OLLAMA_ENDPOINT_PLACEHOLDER = "http://localhost:11434";

    // Tekst-hook (525-561).
    HOOK_TITLE = "Tekst-hook";
    HOOK_HELP = "Avanceret: send den færdige tekst til et script, du selv har lagt i hook-mappen, og indsæt det, scriptet skriver.";
    HOOK_LABEL = "Kør et script på teksten";
    HOOK_HINT = "Scriptet kører med dine rettigheder. Teksten sendes kun på stdin, ikke som argument og ikke gennem en kommandolinje. Fejler scriptet eller tager det over 30 sekunder, indsættes den oprindelige tekst. Teksten skrives aldrig i loggen.";
    HOOK_SCRIPT = "Script";
    HOOK_NONE = "Intet script valgt";
    HOOK_MISSING = "{value} (findes ikke)";
    HOOK_NO_SCRIPTS = "Der er ingen scripts endnu. Læg en .cmd- eller .ps1-fil i mappen {value}. Hent listen igen ved at åbne siden på ny.";
    HOOK_SCRIPTS_IN = "Scripts (.cmd og .ps1) ligger i mappen {value}. Hent listen igen ved at åbne siden på ny.";
    HOOK_FOLDER_UNKNOWN = "dictation_hooks i appens datamappe";

    // Stil pr. program (562-604).
    STYLES_TITLE = "Stil pr. program";
    STYLES_HELP = "I kodeprogrammer og terminaler skrives teksten råt: intet stort begyndelsesbogstav, og kun øh fjernes. Navnet er programfilen, fx Code.exe.";
    STYLE_PROGRAM = "Program";
    STYLE_RAW = "Rå";
    STYLE_NORMAL = "Normal";
    STYLE_FOR = "Stil for {value}";
    STYLE_THE_PROGRAM = "programmet";
    STYLE_REMOVE = "Fjern {value}";
    STYLE_THE_ROW = "rækken";
    STYLE_ADD = "Tilføj program";
    STYLES_SAVE = "Gem programmer";

    // Ordbog (606-652, 178-198).
    WORDS_TITLE = "Ordbog";
    WORDS_HELP = "Navne og fagord, som skal staves rigtigt. Ordene sendes med som stikord til Syv.ai, og stavemåder, du skriver under 'Høres som', rettes til ordet.";
    WORD_LABEL = "Ord";
    WORD_PLACEHOLDER = "Mahoje";
    ALIASES_LABEL = "Høres som (valgfrit)";
    ALIASES_PLACEHOLDER = "maho je, mahoye";
    WORD_ADD = "Tilføj";
    WORD_SAVE = "Gem";
    WORDS_EMPTY = "Ordbogen er tom.";
    WORD_HEARD_AS = " · høres som {value}";
    EDIT = "Ret {value}";
    DELETE = "Slet {value}";
    WORD_SAVE_FAILED = "Ordet kunne ikke gemmes";
    WORD_DELETE_FAILED = "Ordet kunne ikke slettes";

    // Snippets (654-701, 200-220).
    SNIPPETS_TITLE = "Snippets";
    SNIPPETS_HELP = "Sig udløseren, så indsættes teksten. Fx 'min signatur' eller 'mit CVR'.";
    PHRASE_LABEL = "Udløser";
    PHRASE_PLACEHOLDER = "min signatur";
    SNIPPET_TEXT = "Tekst";
    SNIPPET_ADD = "Tilføj snippet";
    SNIPPET_SAVE = "Gem snippet";
    SNIPPETS_EMPTY = "Ingen snippets endnu.";
    SNIPPET_SAVE_FAILED = "Snippet kunne ikke gemmes";
    SNIPPET_DELETE_FAILED = "Snippet kunne ikke slettes";

    // Historik og privatliv (703-722).
    HISTORY_TITLE = "Historik og privatliv";
    HISTORY_LABEL = "Gem historik";
    HISTORY_HINT = "De seneste 500 dikteringer med tekst, svartid og program";
    AUDIO_LABEL = "Gem lyden i 24 timer";
    AUDIO_HINT = "Optagelsen slettes automatisk efter et døgn. Teksten bliver i historikken.";
    PRIVACY_NOTE = "Med Syv.ai sendes kun lydoptagelsen af det, du dikterer, og ordene i din ordbog. Tastetryk gemmes aldrig, og nøglen ligger i Windows Legitimationsstyring, ikke i Tingskrivers filer.";
}

parameterized! {
    load_failed => LOAD_FAILED;
    mic_missing => MIC_MISSING;
    volume_value => VOLUME_VALUE;
    volume_text => VOLUME_TEXT;
    syv_latency => SYV_LATENCY;
    hook_missing => HOOK_MISSING;
    hook_no_scripts => HOOK_NO_SCRIPTS;
    hook_scripts_in => HOOK_SCRIPTS_IN;
    style_for => STYLE_FOR;
    style_remove => STYLE_REMOVE;
    word_heard_as => WORD_HEARD_AS;
    edit => EDIT;
    delete => DELETE;
}

/// A duration as Tauri's `formatMs`: "800 ms", "1,5 s", "2 s".
pub fn duration_ms(ms: u64) -> String {
    if ms < 1000 {
        return MILLISECONDS.replace("{value}", &ms.to_string());
    }
    let tenths = (ms + 50) / 100;
    let seconds = if tenths % 10 == 0 {
        (tenths / 10).to_string()
    } else {
        format!("{},{}", tenths / 10, tenths % 10)
    };
    SECONDS.replace("{value}", &seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_follow_tauri_format_ms() {
        assert_eq!(duration_ms(800), "800 ms");
        assert_eq!(duration_ms(1000), "1 s");
        assert_eq!(duration_ms(1500), "1,5 s");
        assert_eq!(duration_ms(2000), "2 s");
        assert_eq!(duration_ms(1234), "1,2 s");
    }

    #[test]
    fn templates_fill_the_value() {
        assert_eq!(style_for("Code.exe"), "Stil for Code.exe");
        assert_eq!(word_heard_as("maho je"), " · høres som maho je");
    }
}
