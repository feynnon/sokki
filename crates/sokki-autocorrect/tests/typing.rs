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
    /// the app takes no keys right now: a typed fix never arrives
    deaf: bool,
    /// a key the user presses right after the fix was typed
    after_fix: RefCell<Option<char>>,
    /// a key that lands while Sokki reads the field: after it counted the keys, before it
    /// reads the text
    in_value_read: RefCell<Option<char>>,
    /// a key that lands after Sokki read the text, before it reads the cursor
    in_selection_read: RefCell<Option<char>>,
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
            deaf: false,
            after_fix: RefCell::new(None),
            in_value_read: RefCell::new(None),
            in_selection_read: RefCell::new(None),
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
        if let Some(c) = self.in_value_read.borrow_mut().take() {
            self.key(c);
        }
        Some(self.text.borrow().clone())
    }
    fn selection(&self) -> Option<(usize, usize)> {
        if let Some(c) = self.in_selection_read.borrow_mut().take() {
            self.key(c);
        }
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
        if !self.deaf {
            self.put(text);
        }
        if let Some(c) = self.after_fix.borrow_mut().take() {
            self.key(c);
        }
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
    // the pasted word wasn't just typed (only listed); the path isn't a word
    assert_eq!(
        got,
        vec![Outcome::Missed {
            typed: "nihct".into(),
            fix: "nicht".into(),
            why: "word not seen typed (a paste, dictation)"
        }]
    );
    assert_eq!(
        f.text.borrow().as_str(),
        "Ich habe das nihct see src/maion "
    );
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

#[test]
fn a_fix_that_never_arrives_leaves_the_word_and_no_selection() {
    let mut ac = Autocorrect::new(typos(), None);
    let mut f = Fake::new("com.google.Chrome");
    f.deaf = true;
    let got = type_in(&mut ac, &f, "Ich hbae ");
    assert_eq!(got, vec![Outcome::Skipped("the fix didn't arrive")]);
    // the next key goes after the space, not over the word
    f.deaf = false;
    type_in(&mut ac, &f, "es");
    assert_eq!(f.text.borrow().as_str(), "Ich hbae es");
}

#[test]
fn a_key_right_after_the_fix_lands_after_it() {
    let mut ac = Autocorrect::new(typos(), None);
    let f = Fake::new("com.google.Chrome");
    type_in(&mut ac, &f, "Ich hbae");
    *f.after_fix.borrow_mut() = Some('e');
    let got = type_in(&mut ac, &f, " ");
    assert_eq!(
        got,
        vec![Outcome::Fixed {
            typed: "hbae".into(),
            fix: "habe".into(),
            exact: true
        }]
    );
    type_in(&mut ac, &f, "s ");
    assert_eq!(f.text.borrow().as_str(), "Ich habe es ");
    assert_eq!(f.sel.get(), (u16_len("Ich habe es "), 0));
}

/// A rich editor in a browser (Slack, Claude, Gmail): a space at the end is a no-break space
/// until the next character comes.
#[test]
fn fixes_in_a_rich_editor_with_no_break_spaces() {
    let mut ac = Autocorrect::new(typos(), None);
    let f = Fake::new("com.tinyspeck.slackmacgap");
    let mut got = Vec::new();
    for c in "Ich habe das nihct gesehen ".chars() {
        // the space before the cursor turns plain when the next character comes
        let t = f.text.borrow().replace('\u{a0}', " ");
        *f.text.borrow_mut() = t;
        f.key(if c == ' ' { '\u{a0}' } else { c });
        let o = ac.on_key(&f, &|| f.keys.get());
        if o != Outcome::Nothing {
            got.push(o);
        }
    }
    assert_eq!(
        got,
        vec![Outcome::Fixed {
            typed: "nihct".into(),
            fix: "nicht".into(),
            exact: true
        }]
    );
    assert!(f.text.borrow().starts_with("Ich habe das nicht"));
}

#[test]
fn a_word_the_model_gives_back_unchanged_is_left_alone() {
    // the model answers "weiß" with "weiß": Sokki must not select and retype it
    let mut ac = Autocorrect::new(typos(), None);
    let f = Fake::new("com.google.Chrome");
    let got = type_in(&mut ac, &f, "Das weiß ich schon ");
    assert_eq!(got, vec![]);
    assert_eq!(f.text.borrow().as_str(), "Das weiß ich schon ");
}

#[test]
fn a_typo_as_the_first_word_of_an_empty_field_is_fixed() {
    let mut ac = Autocorrect::new(typos(), None);
    let f = Fake::new("com.anthropic.claudefordesktop");
    let got = type_in(&mut ac, &f, "nihct ");
    assert_eq!(
        got,
        vec![Outcome::Fixed {
            typed: "nihct".into(),
            fix: "nicht".into(),
            exact: true
        }]
    );
    // the message is sent: Enter, the field empties, the next one starts
    type_in(&mut ac, &f, "\n");
    *f.text.borrow_mut() = String::new();
    f.sel.set((0, 0));
    let got = type_in(&mut ac, &f, "nihct ");
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(f.text.borrow().as_str(), "nicht ");
}

#[test]
fn a_word_typed_after_a_click_into_the_text_is_fixed() {
    let mut ac = Autocorrect::new(typos(), None);
    let f = Fake::new("com.google.Chrome");
    type_in(&mut ac, &f, "Ich das gesehen");
    // a click puts the cursor after "Ich " (no key, no read)
    f.sel.set((4, 0));
    let got = type_in(&mut ac, &f, "hbae ");
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(f.text.borrow().as_str(), "Ich habe das gesehen");
}

#[test]
fn a_word_corrected_with_backspace_while_typing_is_still_fixed() {
    let mut ac = Autocorrect::new(typos(), None);
    let f = Fake::new("com.anthropic.claudefordesktop");
    let got = type_in(&mut ac, &f, "Das ist nihx\u{8}ct ");
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(f.text.borrow().as_str(), "Das ist nicht ");
}

#[test]
fn a_backspace_and_a_letter_between_two_looks_keep_the_word_typed() {
    let mut ac = Autocorrect::new(typos(), None);
    let f = Fake::new("com.anthropic.claudefordesktop");
    type_in(&mut ac, &f, "Das ist nihx");
    // fast typing: Backspace and the next letter land before Sokki looks again
    f.key('\u{8}');
    f.key('c');
    assert_eq!(ac.on_key(&f, &|| f.keys.get()), Outcome::Nothing);
    let got = type_in(&mut ac, &f, "t ");
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(f.text.borrow().as_str(), "Das ist nicht ");
}

/// Types `before`, then the first letter of `rest` with `race` set to the next one, then the
/// rest: a fast key landing while Sokki reads the field.
fn type_with_race(
    ac: &mut Autocorrect,
    f: &Fake,
    before: &str,
    rest: &str,
    race: &RefCell<Option<char>>,
) -> Vec<Outcome> {
    let mut out = type_in(ac, f, before);
    let mut cs = rest.chars();
    let (a, b) = (cs.next().unwrap(), cs.next().unwrap());
    f.key(a);
    *race.borrow_mut() = Some(b);
    let o = ac.on_key(f, &|| f.keys.get());
    if o != Outcome::Nothing {
        out.push(o);
    }
    out.extend(type_in(ac, f, cs.as_str()));
    out
}

#[test]
fn a_key_landing_while_the_text_is_read_keeps_the_word_typed() {
    let mut ac = Autocorrect::new(typos(), None);
    let f = Fake::new("com.t3tools.t3code");
    let got = type_with_race(
        &mut ac,
        &f,
        "Ich habe das n",
        "ihct gesehen",
        &f.in_value_read,
    );
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(f.text.borrow().as_str(), "Ich habe das nicht gesehen");
}

#[test]
fn a_key_landing_between_text_and_cursor_reads_keeps_the_word_typed() {
    let mut ac = Autocorrect::new(typos(), None);
    let f = Fake::new("com.t3tools.t3code");
    let got = type_with_race(
        &mut ac,
        &f,
        "Ich habe das n",
        "ihct gesehen",
        &f.in_selection_read,
    );
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(f.text.borrow().as_str(), "Ich habe das nicht gesehen");
}

#[test]
fn the_users_own_words_are_loaded_word_by_word() {
    let mut ac = Autocorrect::new(typos(), None);
    let n = ac.set_user_words(&[
        "Meta Ads".into(),
        "Velorama".into(),
        "Velorama".into(),
        "X".into(),
    ]);
    assert_eq!(n, 3); // Meta, Ads, Velorama
}

#[test]
fn a_typo_of_the_users_own_word_is_fixed_to_it() {
    let mut ac = Autocorrect::new(typos(), None);
    ac.set_keyboard(&["qwertyuiop".into(), "asdfghjkl".into(), "zxcvbnm".into()]);
    ac.set_user_words(&["Velorama".into()]);
    let f = Fake::new("com.google.Chrome");
    type_in(&mut ac, &f, "Morgen bin ich bei Veloarma ");
    assert_eq!(f.text.borrow().as_str(), "Morgen bin ich bei Velorama ");
}

#[test]
fn a_word_that_changed_without_a_key_is_listed_as_missed() {
    let mut ac = Autocorrect::new(typos(), None);
    let f = Fake::new("com.anthropic.claudefordesktop");
    type_in(&mut ac, &f, "Das ist ");
    // dictation puts text in without a key
    f.put("nihct ");
    let got = ac.on_key(&f, &|| f.keys.get());
    assert!(matches!(got, Outcome::Missed { .. }), "{got:?}");
    // looking again without a change lists nothing more
    assert_eq!(ac.on_key(&f, &|| f.keys.get()), Outcome::Nothing);
    assert_eq!(f.text.borrow().as_str(), "Das ist nihct ");
}

#[test]
fn a_line_break_the_app_adds_after_the_cursor_keeps_the_word_typed() {
    // rich editors (Claude's composer) may show or drop an empty last line while one types
    let mut ac = Autocorrect::new(typos(), None);
    let f = Fake::new("com.anthropic.claudefordesktop");
    type_in(&mut ac, &f, "Das ist ni");
    let at = f.sel.get();
    f.text.borrow_mut().push('\n');
    f.sel.set(at);
    let got = type_in(&mut ac, &f, "hct ");
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(f.text.borrow().as_str(), "Das ist nicht \n");
}
