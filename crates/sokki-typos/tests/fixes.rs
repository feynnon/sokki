//! Made-up sentences, the phone's tables and the Mac word lists.
use std::path::Path;

fn typos() -> sokki_typos::Typos {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    sokki_typos::Typos::load(
        &root.join("../../mobile/keyboard/src/main/assets/sokki"),
        &root.join("data"),
    )
    .unwrap()
}

fn fixed(t: &sokki_typos::Typos, text: &str) -> Vec<(String, String)> {
    t.fixes(text)
        .into_iter()
        .map(|f| (f.typed, f.fix))
        .collect()
}

#[test]
fn fixes_swapped_letters_in_both_languages() {
    let t = typos();
    let got = fixed(
        &t,
        "Ich habe das nihct gesehen. I think the plan wroks. Ich hbae keine Zeit.",
    );
    assert!(got.contains(&("nihct".into(), "nicht".into())), "{got:?}");
    assert!(got.contains(&("wroks".into(), "works".into())), "{got:?}");
    assert!(got.contains(&("hbae".into(), "habe".into())), "{got:?}");
}

#[test]
fn leaves_real_words_names_and_mixed_language_alone() {
    let t = typos();
    for text in [
        "Das Meeting ist morgen um zehn.",
        "Kannst du das Deployment heute machen?",
        "Qwen and Ollama run locally.",
        "Das ist echt geil.",
    ] {
        assert!(fixed(&t, text).is_empty(), "{text}: {:?}", fixed(&t, text));
    }
}

#[test]
fn keeps_offsets_in_chars() {
    let t = typos();
    let text = "Grüße, wir müssen nihct warten.";
    let f = t.fixes(text);
    let start = f.iter().find(|x| x.typed == "nihct").unwrap().start;
    assert_eq!(
        text.chars().skip(start).take(5).collect::<String>(),
        "nihct"
    );
}

#[test]
fn english_nouns_keep_lower_case_after_an() {
    // Robin, 26.11.2026: "option" became "Option" in an English sentence
    let t = typos();
    for s in [
        "pick an option and go",
        "there is an option here",
        "you have an option",
        "an apple a day",
    ] {
        assert_eq!(fixed(&t, s), vec![], "{s}");
    }
    assert_eq!(
        fixed(&t, "Die option ist gut"),
        vec![("option".into(), "Option".into())]
    );
}

#[test]
fn english_after_in_stays_english() {
    // Robin, 26.11.2026: "in these cases" became "in These cases", and "Justt" was left
    let t = typos();
    assert_eq!(
        fixed(&t, "I guess leaving as typed is ok in these cases"),
        vec![]
    );
    assert_eq!(
        fixed(&t, "Justt now in typing"),
        vec![("Justt".into(), "Just".into())]
    );
    assert_eq!(
        fixed(&t, "Das steht in der these"),
        vec![("these".into(), "These".into())]
    );
}

#[test]
fn correct_long_compounds_stay_as_typed() {
    // 26.11.2026, edited German text: compound repair took a Bloom filter's false "common word"
    let t = typos();
    for s in [
        "Die Schiffe haben Gasturbinenantriebe.",
        "Der Medienentwicklungsplan kommt.",
    ] {
        assert_eq!(fixed(&t, s), vec![], "{s}");
    }
}
