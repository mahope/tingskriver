//! Tests for layer 1 cleanup (`cleanup.rs`).

use super::*;

fn normal(input: &str) -> String {
    clean(input, Style::Normal, &Rules::default())
}

fn raw(input: &str) -> String {
    clean(input, Style::Raw, &Rules::default())
}

fn rules() -> Rules {
    Rules {
        replacements: vec![
            Replacement {
                from: "Mahoje".into(),
                to: "Mahoje".into(),
                keep_english: false,
            },
            Replacement {
                from: "maho je".into(),
                to: "Mahoje".into(),
                keep_english: false,
            },
            Replacement {
                from: "syv ai".into(),
                to: "Syv.ai".into(),
                keep_english: false,
            },
        ],
        snippets: vec![
            Snippet {
                trigger: "min signatur".into(),
                text: "Mads\nMahope".into(),
            },
            Snippet {
                trigger: "mit cvr".into(),
                text: "39413892".into(),
            },
        ],
        ..Rules::default()
    }
}

#[test]
fn empty_and_whitespace() {
    assert_eq!(normal(""), "");
    assert_eq!(normal("   "), "");
}

#[test]
fn removes_leading_fillers() {
    assert_eq!(normal("øh jeg kommer i morgen"), "Jeg kommer i morgen");
    assert_eq!(normal("Øhm, jeg kommer i morgen."), "Jeg kommer i morgen.");
    assert_eq!(normal("Altså, det er fint."), "Det er fint.");
    assert_eq!(normal("øh altså øhh det er fint"), "Det er fint");
}

#[test]
fn removes_fillers_between_sentences() {
    assert_eq!(
        normal("Det er godt. Øh, vi ses i morgen."),
        "Det er godt. Vi ses i morgen."
    );
    assert_eq!(
        normal("Det er godt. Altså, vi ses i morgen."),
        "Det er godt. Vi ses i morgen."
    );
    assert_eq!(normal("jeg tror øh at det går"), "Jeg tror at det går");
}

#[test]
fn keeps_altsaa_mid_sentence() {
    assert_eq!(normal("han er altså sød"), "Han er altså sød");
}

#[test]
fn filler_before_sentence_end_keeps_period() {
    assert_eq!(normal("det var det øh."), "Det var det.");
}

#[test]
fn does_not_touch_fillers_inside_words() {
    assert_eq!(normal("køhm og søhest"), "Køhm og søhest");
    assert_eq!(normal("altsåi"), "Altsåi");
}

#[test]
fn spoken_punctuation() {
    assert_eq!(
        normal("hej Mads komma hvordan går det spørgsmålstegn"),
        "Hej Mads, hvordan går det?"
    );
    assert_eq!(normal("det er fint punktum tak"), "Det er fint. Tak");
    assert_eq!(normal("husk udråbstegn"), "Husk!");
    assert_eq!(
        normal("tre ting kolon mælk og brød"),
        "Tre ting: mælk og brød"
    );
    assert_eq!(normal("a semikolon b"), "A; b");
}

#[test]
fn spoken_punctuation_with_model_punctuation() {
    assert_eq!(
        normal("Hej Mads, komma, hvordan går det, spørgsmålstegn."),
        "Hej Mads, hvordan går det?"
    );
    assert_eq!(normal("Det er fint. Punktum."), "Det er fint.");
    assert_eq!(normal("Komma, hej"), "Hej");
}

#[test]
fn new_line_and_paragraph() {
    assert_eq!(
        normal("hej ny linje hvordan går det"),
        "Hej\nHvordan går det"
    );
    assert_eq!(normal("Hej, ny linje, tak."), "Hej\nTak.");
    assert_eq!(normal("Hej. Nyt afsnit. Tak."), "Hej.\n\nTak.");
    assert_eq!(normal("hej ny linje ny linje tak"), "Hej\n\nTak");
    assert_eq!(normal("hej ny linie tak"), "Hej\nTak");
}

#[test]
fn fixes_space_before_punctuation() {
    assert_eq!(normal("hej , du . hvad så ?"), "Hej, du. Hvad så?");
    assert_eq!(normal("hej  med   dig"), "Hej med dig");
}

#[test]
fn capitalises_sentence_starts() {
    assert_eq!(normal("hej. hvad så? godt! ja"), "Hej. Hvad så? Godt! Ja");
    assert_eq!(normal("ældre æbler"), "Ældre æbler");
    assert_eq!(normal("øl er godt"), "Øl er godt");
}

#[test]
fn abbreviations_do_not_end_sentences() {
    assert_eq!(normal("vi ses kl. tre"), "Vi ses kl. 3");
    assert_eq!(normal("vi ses kl. halv tre"), "Vi ses kl. halv tre");
    assert_eq!(normal("frugt f.eks. æbler"), "Frugt f.eks. æbler");
    assert_eq!(normal("ca. ti"), "Ca. ti");
}

#[test]
fn keeps_numbers_and_inner_punctuation() {
    assert_eq!(normal("det koster 3.5 kroner"), "Det koster 3,5 kr.");
    assert_eq!(raw("det koster 3.5 kroner"), "det koster 3.5 kroner");
    assert_eq!(
        normal("skriv til mads@mahoje.dk"),
        "Skriv til mads@mahoje.dk"
    );
}

#[test]
fn dictionary_replacements() {
    let r = rules();
    assert_eq!(
        clean("jeg arbejder hos maho je", Style::Normal, &r),
        "Jeg arbejder hos Mahoje"
    );
    assert_eq!(
        clean("mahoje bruger syv ai.", Style::Normal, &r),
        "Mahoje bruger Syv.ai."
    );
    assert_eq!(clean("MAHOJE", Style::Normal, &r), "Mahoje");
}

#[test]
fn snippets() {
    let r = rules();
    assert_eq!(clean("Min signatur.", Style::Normal, &r), "Mads\nMahope");
    assert_eq!(
        clean("vores cvr er mit cvr punktum", Style::Normal, &r),
        "Vores cvr er 39413892."
    );
}

#[test]
fn raw_style() {
    assert_eq!(raw("øh git status"), "git status");
    assert_eq!(raw("altså npm install"), "altså npm install");
    assert_eq!(raw("cargo test komma tak"), "cargo test, tak");
    assert_eq!(raw("ls ny linje pwd"), "ls\npwd");
}

#[test]
fn model_newlines_are_kept() {
    assert_eq!(
        normal("første linje\nanden linje"),
        "Første linje\nAnden linje"
    );
    assert_eq!(normal("a\n\n\n\nb"), "A\n\nB");
}

#[test]
fn only_fillers_gives_empty_text() {
    assert_eq!(normal("øh øhm."), "");
    assert_eq!(normal("Øh."), "");
}

#[test]
fn prompt_words_are_unique_targets() {
    assert_eq!(prompt_words(&rules()), vec!["Mahoje", "Syv.ai"]);
}

#[test]
fn style_parsing() {
    assert_eq!(Style::parse("raw"), Style::Raw);
    assert_eq!(Style::parse("rå"), Style::Raw);
    assert_eq!(Style::parse("normal"), Style::Normal);
    assert_eq!(Style::parse("whatever"), Style::Normal);
}

#[test]
fn long_text_is_fast() {
    let sentence = "øh jeg tror altså at det komma som vi sagde punktum ";
    let input = sentence.repeat(200);
    let start = std::time::Instant::now();
    let out = clean(&input, Style::Normal, &rules());
    assert!(!out.is_empty());
    assert!(
        start.elapsed().as_millis() < 50,
        "took {:?}",
        start.elapsed()
    );
}

// ---------------------------------------------------------------------------
// Spoken signs and Windows voice typing words (#14)
// ---------------------------------------------------------------------------

#[test]
fn spoken_signs_table() {
    let cases: &[(&str, &str)] = &[
        ("hej komma du", "Hej, du"),
        ("slut punktum", "Slut."),
        ("tre ting kolon a", "Tre ting: a"),
        ("a semikolon b", "A; b"),
        ("kommer du spørgsmålstegn", "Kommer du?"),
        ("hurra udråbstegn", "Hurra!"),
        ("e bindestreg mail", "E-mail"),
        ("e binde streg mail", "E-mail"),
        ("vi kommer tankestreg måske", "Vi kommer – måske"),
        ("og skråstreg eller", "Og/eller"),
        ("og skrå streg eller", "Og/eller"),
        ("c kolon omvendt skråstreg windows", "C:\\windows"),
        ("fil understreg navn", "Fil_navn"),
        ("se parentes start bilag parentes slut", "Se (bilag)"),
        ("se start parentes bilag slut parentes", "Se (bilag)"),
        ("se venstre parentes bilag højre parentes", "Se (bilag)"),
        (
            "se begynd parentes bilag luk parentes punktum",
            "Se (bilag).",
        ),
        (
            "liste kantet parentes start x kantet parentes slut",
            "Liste [x]",
        ),
        (
            "kode tuborgparentes start x tuborgparentes slut",
            "Kode {x}",
        ),
        (
            "han sagde anførselstegn hej anførselstegn",
            "Han sagde \"hej\"",
        ),
        (
            "han sagde citationstegn hej citationstegn til mig",
            "Han sagde \"hej\" til mig",
        ),
        ("citat start ja citat slut sagde hun", "\"Ja\" sagde hun"),
        ("han sagde start citat nej slut citat", "Han sagde \"nej\""),
        ("rock apostrof n roll", "Rock'n roll"),
        ("og så prik prik prik", "Og så..."),
        ("og så tre prikker", "Og så..."),
        ("hej snabel-a alle", "Hej@alle"),
        ("x procenttegn", "X %"),
        ("to plustegn to", "To + to"),
        ("a minustegn b", "A - b"),
        ("a lighedstegn b", "A = b"),
        ("tre gangetegn fire", "Tre × fire"),
        ("vi bruger hashtag sommer", "Vi bruger #sommer"),
        ("vi bruger havelåge sommer", "Vi bruger #sommer"),
        ("hansen og-tegn søn", "Hansen & søn"),
        ("a stjernetegn b", "A * b"),
        ("a asterisk b", "A * b"),
        ("se paragraftegn tre", "Se § tre"),
        ("tyve gradtegn", "20°"),
        ("pris dollartegn", "Pris $"),
        ("pris eurotegn", "Pris €"),
        ("pris pundtegn", "Pris £"),
        ("hej smiley", "Hej :-)"),
        ("øv sur smiley", "Øv :-("),
        ("hej ny linje tak", "Hej\nTak"),
        ("hej linjeskift tak", "Hej\nTak"),
        ("hej nyt afsnit tak", "Hej\n\nTak"),
        ("hej afsnitsskift tak", "Hej\n\nTak"),
    ];
    for (input, expected) in cases {
        assert_eq!(normal(input), *expected, "{input}");
    }
}

#[test]
fn windows_words_give_the_plan_result() {
    let pairs = [
        ("linjeskift", "ny linje"),
        ("linieskift", "ny linje"),
        ("næste linje", "ny linje"),
        ("afsnitsskift", "nyt afsnit"),
        ("næste afsnit", "nyt afsnit"),
        ("udråbstegn", "udråbstegn"),
        ("spørgsmålstegn", "spørgsmålstegn"),
    ];
    for (windows, plan) in pairs {
        let a = format!("hej {windows} med dig");
        let b = format!("hej {plan} med dig");
        assert_eq!(normal(&a), normal(&b), "{windows}");
    }
    let commands = [
        ("slet det", "slet dette"),
        ("vælg det", "markér det"),
        ("vælg det", "marker det"),
        ("slet det", "fortryd det"),
    ];
    let rules = Rules::default();
    let options = Options::new(Style::Normal, &rules);
    for (plan, windows) in commands {
        assert_eq!(
            clean_with(plan, &options),
            clean_with(windows, &options),
            "{windows}"
        );
    }
    assert_eq!(
        clean_with("hej stop dikteringen", &options),
        clean_with("hej stop diktering", &options)
    );
}

#[test]
fn literal_words() {
    assert_eq!(normal("skriv ordet punktum"), "punktum");
    assert_eq!(normal("Skriv ordet punktum."), "punktum.");
    assert_eq!(
        normal("han sagde skriv ordet komma til mig"),
        "Han sagde komma til mig"
    );
    assert_eq!(normal("skriv ordet ny linje"), "ny linje");
    assert_eq!(normal("Skriv ordet, komma."), "komma.");
    // Without the fixed phrase the sign is used.
    assert_eq!(normal("ordet komma"), "Ordet,");
}

fn command(input: &str) -> Cleaned {
    let rules = Rules::default();
    clean_with(input, &Options::new(Style::Normal, &rules))
}

#[test]
fn voice_commands() {
    assert_eq!(command("slet det").command, Some(VoiceCommand::DeleteThat));
    assert_eq!(command("Slet det.").command, Some(VoiceCommand::DeleteThat));
    assert_eq!(
        command("øh slet det").command,
        Some(VoiceCommand::DeleteThat)
    );
    assert_eq!(command("slet det").text, "");
    assert_eq!(command("vælg det").command, Some(VoiceCommand::SelectThat));
    // In a sentence the words are text.
    let c = command("det er slet det ikke");
    assert_eq!((c.text.as_str(), c.command), ("Det er slet det ikke", None));
    let c = command("Hej Anna, stop diktering.");
    assert_eq!(
        (c.text.as_str(), c.command),
        ("Hej Anna", Some(VoiceCommand::StopDictation))
    );
    let c = command("stop diktering");
    assert_eq!(
        (c.text.as_str(), c.command),
        ("", Some(VoiceCommand::StopDictation))
    );
}

// ---------------------------------------------------------------------------
// "Indsæt og tryk Enter" (#13)
// ---------------------------------------------------------------------------

fn chat(input: &str) -> Cleaned {
    let rules = Rules::default();
    let triggers = vec!["send".to_string(), "send den".to_string()];
    let options = Options {
        enter_triggers: &triggers,
        ..Options::new(Style::Normal, &rules)
    };
    clean_with(input, &options)
}

#[test]
fn enter_trigger_at_the_end_presses_enter() {
    let c = chat("ses i morgen send");
    assert_eq!((c.text.as_str(), c.press_enter), ("Ses i morgen", true));
    let c = chat("Ses i morgen, send.");
    assert_eq!((c.text.as_str(), c.press_enter), ("Ses i morgen", true));
    let c = chat("Er du klar? Send den.");
    assert_eq!((c.text.as_str(), c.press_enter), ("Er du klar?", true));
}

#[test]
fn enter_trigger_in_the_middle_is_text() {
    let c = chat("jeg send dig filen i morgen");
    assert_eq!(
        (c.text.as_str(), c.press_enter),
        ("Jeg send dig filen i morgen", false)
    );
    let c = chat("send den til Anna");
    assert!(!c.press_enter);
    // Only the trigger word: nothing to send.
    assert!(!chat("send").press_enter);
    // Without triggers (all modes except chat) it is text.
    assert_eq!(command("ses i morgen send").text, "Ses i morgen send");
    // A literal word is never a trigger.
    let c = chat("skriv ordet send");
    assert_eq!((c.text.as_str(), c.press_enter), ("send", false));
}

// ---------------------------------------------------------------------------
// Addresses (#27)
// ---------------------------------------------------------------------------

#[test]
fn addresses_table() {
    let cases: &[(&str, &str)] = &[
        ("anna snabel-a eksempel punktum d k", "anna@eksempel.dk"),
        (
            "skriv til anna snabel-a eksempel punktum dk",
            "Skriv til anna@eksempel.dk",
        ),
        ("mads snabel-a mahoje punktum dk", "mads@mahoje.dk"),
        (
            "anna punktum hansen snabel-a firma punktum com",
            "anna.hansen@firma.com",
        ),
        (
            "anna bindestreg marie snabel-a eksempel punktum dk",
            "anna-marie@eksempel.dk",
        ),
        (
            "kontakt snabel-a mit bindestreg firma punktum dk",
            "kontakt@mit-firma.dk",
        ),
        ("info snabel-a gmail punktum com", "info@gmail.com"),
        ("Anna snabel a Eksempel punktum DK", "anna@eksempel.dk"),
        (
            "jens understreg hansen snabel-a eksempel punktum dk",
            "jens_hansen@eksempel.dk",
        ),
        ("a n n a snabel-a eksempel punktum dk", "anna@eksempel.dk"),
        ("w w w punktum mahoje punktum dk", "www.mahoje.dk"),
        ("gå ind på www punktum dr punktum dk", "Gå ind på www.dr.dk"),
        (
            "www punktum mit bindestreg firma punktum com",
            "www.mit-firma.com",
        ),
        (
            "www punktum mahoje punktum dk skråstreg kontakt",
            "www.mahoje.dk/kontakt",
        ),
        ("mahoje punktum dk", "mahoje.dk"),
        ("læs mere på github punktum com", "Læs mere på github.com"),
        ("se borger punktum dk", "Se borger.dk"),
        (
            "skriv til support snabel-a firma punktum co punktum uk",
            "Skriv til support@firma.co.uk",
        ),
        ("post snabel-a eksempel punktum d e", "post@eksempel.de"),
        (
            "skriv til anna snabel-a eksempel punktum dk punktum",
            "Skriv til anna@eksempel.dk.",
        ),
        (
            "Mail til anna snabel-a eksempel. Dk.",
            "Mail til anna@eksempel.dk.",
        ),
        ("se mit-firma punktum dk", "Se mit-firma.dk"),
        (
            "skriv til anna snabel-a eksempel.dk",
            "Skriv til anna@eksempel.dk",
        ),
        ("skriv til anna@eksempel.dk", "Skriv til anna@eksempel.dk"),
        ("se mahoje prik dk", "Se mahoje.dk"),
        // A full stop after an address ends the sentence (#27 review).
        (
            "Skriv til anna@eksempel.dk. Vi ses i morgen.",
            "Skriv til anna@eksempel.dk. Vi ses i morgen.",
        ),
        ("Mail anna@eksempel.dk. Tak.", "Mail anna@eksempel.dk. Tak."),
        (
            "skriv til anna snabel-a eksempel punktum dk punktum vi ses i morgen",
            "Skriv til anna@eksempel.dk. Vi ses i morgen",
        ),
        (
            "Mail til anna snabel-a eksempel. Dk. Tak.",
            "Mail til anna@eksempel.dk. Tak.",
        ),
        ("Den er god. Com", "Den er god. Com"),
        (
            "ring til mig. anna snabel-a eksempel punktum dk",
            "Ring til mig. anna@eksempel.dk",
        ),
    ];
    for (input, expected) in cases {
        let out = normal(input);
        assert_eq!(out, *expected, "{input}");
        // The address itself is lower case without spaces.
        if let Some(address) = out
            .split_whitespace()
            .find(|w| w.contains('@') || w.contains(".dk") || w.contains(".com"))
        {
            assert_eq!(address, address.to_lowercase(), "{input}");
        }
    }
}

#[test]
fn punktum_without_address_is_unchanged() {
    let sentences = [
        "det er fint punktum tak",
        "vi ses i morgen punktum de kommer klokken tre",
        "jeg er færdig punktum se du det",
        "han kom hjem punktum nu er han her",
        "det var det punktum",
        "skriv til mig punktum",
        "han kom punktum de var glade",
        "det er slut punktum is er godt",
    ];
    let rules = Rules::default();
    for s in sentences {
        let with = clean_with(s, &Options::new(Style::Normal, &rules)).text;
        let without = clean_with(
            s,
            &Options {
                format_addresses: false,
                ..Options::new(Style::Normal, &rules)
            },
        )
        .text;
        assert_eq!(with, without, "{s}");
    }
}

#[test]
fn cvr_numbers() {
    assert_eq!(
        normal("vores cvr er tre ni fire en tre otte ni to"),
        "Vores cvr er 39413892"
    );
    assert_eq!(normal("CVR-nummer 39 41 38 92"), "CVR-nummer 39413892");
    assert_eq!(
        normal("cvr nummer niogtredive enogfyrre otteogtredive tooghalvfems"),
        "Cvr nummer 39413892"
    );
    assert_eq!(
        normal("tallet er tre ni fire en tre otte ni to"),
        "Tallet er 39413892"
    );
    // Not eight digits: unchanged.
    assert_eq!(normal("cvr tre ni fire"), "Cvr tre ni fire");
}

// ---------------------------------------------------------------------------
// Numbers, dates, times and amounts (#28)
// ---------------------------------------------------------------------------

#[test]
fn danish_number_formats() {
    let cases: &[(&str, &str)] = &[
        ("tretten hundrede og halvtreds", "1.350"),
        (
            "det koster tretten hundrede og halvtreds kroner",
            "Det koster 1.350 kr.",
        ),
        ("vi ses den tredje november", "Vi ses den 3. november"),
        ("den første januar", "Den 1. januar"),
        (
            "fødselsdag den enogtredivte december",
            "Fødselsdag den 31. december",
        ),
        (
            "den tredje november to tusind og seksogtyve",
            "Den 3. november 2026",
        ),
        (
            "den fjerde juli nitten hundrede og firs",
            "Den 4. juli 1980",
        ),
        ("den femte maj tyve seksogtyve", "Den 5. maj 2026"),
        (
            "det bliver en anden maj end sidst",
            "Det bliver en anden maj end sidst",
        ),
        ("vi ses den anden maj", "Vi ses den 2. maj"),
        ("den tredje", "Den tredje"),
        ("mødet er klokken fjorten tredive", "Mødet er kl. 14.30"),
        ("klokken otte", "Kl. 8"),
        ("klokken ni nul fem", "Kl. 9.05"),
        ("klokken tyve", "Kl. 20"),
        ("vi ses klokken 14:30", "Vi ses kl. 14.30"),
        ("vi ses kl. 14.30", "Vi ses kl. 14.30"),
        ("klokken halv tre", "Klokken halv tre"),
        ("klokken ti over fem", "Klokken ti over fem"),
        ("tolv procent", "12 %"),
        ("tolv komma fem procent", "12,5 %"),
        ("rabatten er 12%", "Rabatten er 12 %"),
        ("renten er 3.5%", "Renten er 3,5 %"),
        ("fem tusind kroner", "5.000 kr."),
        ("det koster 5000 kroner", "Det koster 5.000 kr."),
        ("det koster fem kroner", "Det koster 5 kr."),
        ("det koster 1,350 kroner", "Det koster 1.350 kr."),
        (
            "prisen er to komma fem millioner kroner",
            "Prisen er 2,5 millioner kr.",
        ),
        ("fem tusind kroner punktum tak", "5.000 kr. Tak"),
        ("uge fireogfyrre", "Uge 44"),
        ("vi holder ferie i uge syv", "Vi holder ferie i uge 7"),
        (
            "ring på telefon tolv tredive fyrre halvtreds",
            "Ring på telefon 12 30 40 50",
        ),
        (
            "mobil to otte ni fire fem seks syv tre",
            "Mobil 28 94 56 73",
        ),
        (
            "telefon plus fem og fyrre tolv tredive fyrre halvtreds",
            "Telefon +45 12 30 40 50",
        ),
        ("telefonnummer 12345678", "Telefonnummer 12 34 56 78"),
        ("der kom tretten", "Der kom 13"),
        ("der kom to", "Der kom to"),
        ("der var tolv", "Der var tolv"),
        ("fem og fyrre minutter", "45 minutter"),
        ("seksogtyve", "26"),
        ("i to tusind og seksogtyve", "I 2026"),
        ("siden nitten hundrede og firs", "Siden 1980"),
        ("to hundrede og halvtreds gæster", "250 gæster"),
        ("et hundrede procent", "100 %"),
        ("hundrede procent", "100 %"),
        ("et par hundrede", "Et par hundrede"),
        ("tre millioner indbyggere", "3 millioner indbyggere"),
        ("tolv tusind fem hundrede", "12.500"),
        (
            "vi var tre komma fire kom senere",
            "Vi var tre, fire kom senere",
        ),
        ("der er nul fejl", "Der er nul fejl"),
        ("det er tyve grader", "Det er 20 grader"),
        ("det er fem grader", "Det er 5 grader"),
    ];
    assert!(cases.len() >= 40);
    for (input, expected) in cases {
        assert_eq!(normal(input), *expected, "{input}");
    }
}

#[test]
fn number_formatting_can_be_switched_off() {
    let rules = Rules::default();
    let off = Options {
        format_numbers: false,
        ..Options::new(Style::Normal, &rules)
    };
    assert_eq!(
        clean_with("tolv procent klokken fjorten", &off).text,
        "Tolv procent klokken fjorten"
    );
    assert_eq!(
        clean_with("tolv procent", &Options::new(Style::Normal, &rules)).text,
        "12 %"
    );
}

#[test]
fn code_urls_and_file_names_are_unchanged() {
    let cases = [
        "åbn filen config_2026.json",
        "se www.eksempel.dk/side-12",
        "kør main.rs med 3.5",
        "version v2.10.3",
        "filen rapport-2026.pdf",
        "gå til https://mahoje.dk/priser?id=12%",
        "version 1.350",
        "brug port 8080",
    ];
    for input in cases {
        let out = normal(input);
        let expected = upper_first(input);
        assert_eq!(out, expected, "{input}");
    }
    // Code mode: nothing is formatted.
    assert_eq!(
        raw("klokken fjorten tredive tolv procent"),
        "klokken fjorten tredive tolv procent"
    );
    assert_eq!(
        raw("anna snabel-a eksempel punktum dk"),
        "anna@eksempel. dk"
    );
}

// ---------------------------------------------------------------------------
// English terms in Danish sentences (#29)
// ---------------------------------------------------------------------------

fn english_rules() -> Rules {
    let word = |from: &str, to: &str, keep_english| Replacement {
        from: from.into(),
        to: to.into(),
        keep_english,
    };
    Rules {
        replacements: vec![
            word("npm", "npm", true),
            word("i phone", "iPhone", true),
            word("iphone", "iPhone", true),
            word("hashtag", "hashtag", true),
            word("fire base", "Firebase", true),
            word("mahoje", "mahoje", false),
        ],
        ..Rules::default()
    }
}

#[test]
fn english_tag_is_respected_by_layer_1() {
    let r = english_rules();
    // Case is kept, also at the start of a sentence.
    assert_eq!(
        clean("npm er installeret", Style::Normal, &r),
        "npm er installeret"
    );
    assert_eq!(
        clean("i phone virker. iphone er fin", Style::Normal, &r),
        "iPhone virker. iPhone er fin"
    );
    // Not read as a sign or a number.
    assert_eq!(
        clean("brug et hashtag", Style::Normal, &r),
        "Brug et hashtag"
    );
    assert_eq!(
        clean("vi bruger fire base", Style::Normal, &r),
        "Vi bruger Firebase"
    );
    // Untagged dictionary words still get a capital letter.
    assert_eq!(clean("mahoje er fint", Style::Normal, &r), "Mahoje er fint");
    assert_eq!(
        english_terms(&r),
        vec!["npm", "iPhone", "hashtag", "Firebase"]
    );
}

/// Mixed sentences (`tests/fixtures/dictation/blandet_sprog.tsv`): sentence,
/// tab, the English terms that must survive (comma separated).
#[test]
fn english_terms_survive_mixed_sentences() {
    let fixture = include_str!("../../../tests/fixtures/dictation/blandet_sprog.tsv");
    let rules = Rules::default();
    let mut total = 0;
    let mut kept = 0;
    let mut sentences = 0;
    let mut lost = Vec::new();
    for line in fixture
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
    {
        let (sentence, terms) = line.split_once('\t').expect("sentence<TAB>terms");
        sentences += 1;
        let out = clean(sentence, Style::Normal, &rules);
        let unformatted = clean_with(
            sentence,
            &Options {
                format_numbers: false,
                ..Options::new(Style::Normal, &rules)
            },
        )
        .text;
        // Only a capital first letter at a sentence start is allowed.
        let out_lower_first: String = out
            .split_inclusive(['.', '?', '!', '\n'])
            .map(|s| {
                let t = s.trim_start();
                let lead = &s[..s.len() - t.len()];
                let mut c = t.chars();
                match c.next() {
                    Some(f) => format!("{lead}{}{}", f.to_lowercase(), c.as_str()),
                    None => s.to_string(),
                }
            })
            .collect();
        for term in terms.split(',').map(str::trim).filter(|t| !t.is_empty()) {
            total += 1;
            if out.contains(term) || out_lower_first.contains(term) {
                kept += 1;
            } else {
                lost.push(format!("{term:?} in {out:?}"));
            }
        }
        // Nothing is translated: every word of the sentence is still there.
        let normalize = |s: &str| {
            s.to_lowercase()
                .replace(|c: char| !c.is_alphanumeric() && c != ' ', " ")
        };
        for word in normalize(sentence).split_whitespace() {
            // Spoken signs are not words of the sentence.
            let sign = ["punktum", "komma"].contains(&word);
            if word.chars().all(|c| c.is_ascii_alphabetic()) && word.len() > 3 && !sign {
                let found = normalize(&unformatted)
                    .split_whitespace()
                    .any(|w| w == word);
                assert!(found, "{word:?} missing in {unformatted:?}");
            }
        }
    }
    assert!(sentences >= 30, "only {sentences} sentences");
    assert!(kept * 100 >= total * 90, "kept {kept} of {total}: {lost:?}");
}
