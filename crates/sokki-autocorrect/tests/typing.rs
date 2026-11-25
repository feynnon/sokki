//! Typing into a simulated text field, key by key, as Sokki sees it through Accessibility.
//! Made-up sentences only.
use std::cell::{Cell, RefCell};
use std::path::Path;

use sokki_autocorrect::{u16_len, Autocorrect, Field, Outcome};

fn typos() -> sokki_typos::Typos {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    sokki_typos::Typos::load(
        &root.join("mobile/keyboard/src/main/assets/sokki"),
        &root.join("crates/sokki-typos/data"),
    )
    .unwrap()
}

/// A field like Chrome's: text, a selection, typing replaces the selection.
struct Fake {
    app: String,
    text: RefCell<String>,
    sel: Cell<(usize, usize)>,
    replaceable: bool,
    /// a key the user presses while Sokki swaps a word (between select and insert)
    race: RefCell<Option<char>>,
    keys: Cell<u64>,
}

impl Fake {
    fn new(app: &str) -> Fake {
        Fake {
            app: app.into(),
            text: RefCell::new(String::new()),
            sel: Cell::new((0, 0)),
            replaceable: true,
            race: RefCell::new(None),
            keys: Cell::new(0),
        }
    }

    fn put(&self, s: &str) {
        let (at, len) = self.sel.get();
        let t = self.text.borrow().clone();
        let units: Vec<u16> = t.encode_utf16().collect();
        let mut out: Vec<u16> = units[..at].to_vec();
        out.extend(s.encode_utf16());
        out.extend_from_slice(&units[at + len..]);
        *self.text.borrow_mut() = String::from_utf16(&out).unwrap();
        self.sel.set((at + u16_len(s), 0));
    }

    /// The user presses a key: the field changes, the hook counts it.
    fn key(&self, c: char) {
        self.keys.set(self.keys.get() + 1);
        if c == '\u{8}' {
            let (at, _) = self.sel.get();
            self.sel.set((at - 1, 1));
            self.put("");
        } else {
            self.put(&c.to_string());
        }
    }
}

impl Field for Fake {
    fn app(&self) -> &str {
        &self.app
    }
    fn value(&self) -> Option<String> {
        Some(self.text.borrow().clone())
    }
    fn selection(&self) -> Option<(usize, usize)> {
        Some(self.sel.get())
    }
    fn replaceable(&self) -> bool {
        self.replaceable
    }
    fn select(&self, start: usize, len: usize) -> bool {
        self.sel.set((start, len));
        if let Some(c) = self.race.borrow_mut().take() {
            self.key(c); // typed over the selected word
        }
        true
    }
    fn insert(&self, text: &str) -> bool {
        self.put(text);
        true
    }
}

/// Types `s` key by key; Sokki looks after each key, as its thread does.
fn type_in(ac: &mut Autocorrect, f: &Fake, s: &str) -> Vec<Outcome> {
    let mut out = Vec::new();
    for c in s.chars() {
        f.key(c);
        let o = ac.on_key(f, &|| f.keys.get());
        if o != Outcome::Nothing {
            out.push(o);
        }
    }
    out
}

#[test]
fn fixes_a_typo_when_the_space_is_typed_and_keeps_the_cursor() {
    let mut ac = Autocorrect::new(typos(), None);
    let f = Fake::new("com.google.Chrome");
    let got = type_in(&mut ac, &f, "Ich habe das nihct gesehen");
    assert_eq!(
        got,
        vec![Outcome::Fixed {
            typed: "nihct".into(),
            fix: "nicht".into(),
            exact: true
        }]
    );
    assert_eq!(f.text.borrow().as_str(), "Ich habe das nicht gesehen");
    assert_eq!(f.sel.get(), (u16_len("Ich habe das nicht gesehen"), 0));
}

#[test]
fn backspace_right_after_a_fix_brings_the_word_back_for_good() {
    let dir = std::env::temp_dir().join(format!("sokki-ac-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let kept = dir.join("kept.txt");
    let _ = std::fs::remove_file(&kept);
    let mut ac = Autocorrect::new(typos(), Some(kept.clone()));
    let f = Fake::new("com.tinyspeck.slackmacgap");
    type_in(&mut ac, &f, "I think the plan wroks ");
    assert_eq!(f.text.borrow().as_str(), "I think the plan works ");
    let got = type_in(&mut ac, &f, "\u{8}");
    assert_eq!(
        got,
        vec![Outcome::Undone {
            typed: "wroks".into(),
            fix: "works".into()
        }]
    );
    assert_eq!(f.text.borrow().as_str(), "I think the plan wroks");
    // kept from now on, also after a restart
    type_in(&mut ac, &f, " and wroks ");
    assert_eq!(
        f.text.borrow().as_str(),
        "I think the plan wroks and wroks "
    );
    assert_eq!(std::fs::read_to_string(&kept).unwrap().trim(), "wroks");
    let mut again = Autocorrect::new(typos(), Some(kept));
    let g = Fake::new("com.tinyspeck.slackmacgap");
    assert!(type_in(&mut again, &g, "it wroks ").is_empty());
}

#[test]
fn never_touches_pastes_paths_or_fields_it_cant_replace() {
    let mut ac = Autocorrect::new(typos(), None);
    let f = Fake::new("com.google.Chrome");
    // a paste: many characters at once, then a space typed
    f.put("Ich habe das nihct");
    f.keys.set(f.keys.get() + 1);
    assert_eq!(ac.on_key(&f, &|| f.keys.get()), Outcome::Nothing);
    let got = type_in(&mut ac, &f, " see src/maion ");
    // the pasted word wasn't just typed; the path isn't a word
    assert!(got.is_empty(), "{got:?}");
    let mut g = Fake::new("com.google.Chrome");
    g.replaceable = false;
    let got = type_in(&mut ac, &g, "Ich hbae ");
    assert_eq!(got, vec![Outcome::Skipped("field can't be replaced")]);
    assert_eq!(g.text.borrow().as_str(), "Ich hbae ");
}

#[test]
fn a_key_during_the_swap_is_noticed_and_logged() {
    let mut ac = Autocorrect::new(typos(), None);
    let f = Fake::new("com.google.Chrome");
    type_in(&mut ac, &f, "Ich hbae");
    *f.race.borrow_mut() = Some('k');
    let got = type_in(&mut ac, &f, " ");
    // the user's key landed on the selected word: Sokki notices the count changed, types no
    // fix, and puts the word back with the key where the cursor was
    assert_eq!(got, vec![Outcome::Skipped("typing on, word put back")]);
    assert_eq!(f.text.borrow().as_str(), "Ich hbae k");
    assert_eq!(f.sel.get(), (u16_len("Ich hbae k"), 0));
}

#[test]
fn leaves_apple_apps_terminals_and_editors_alone() {
    for app in [
        "com.apple.mail",
        "com.apple.Terminal",
        "com.googlecode.iterm2",
        "com.microsoft.VSCode",
        "com.1password.1password",
    ] {
        assert!(!sokki_autocorrect::allowed(app, &[]), "{app}");
    }
    for app in [
        "com.google.Chrome",
        "com.tinyspeck.slackmacgap",
        "com.anthropic.claudefordesktop",
        "notion.id",
    ] {
        assert!(sokki_autocorrect::allowed(app, &[]), "{app}");
    }
    assert!(!sokki_autocorrect::allowed(
        "notion.id",
        &["Notion.id".into()]
    ));
}
