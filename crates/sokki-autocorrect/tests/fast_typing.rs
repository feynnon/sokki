//! Fast typing while Sokki swaps a word (Robin, 27 Nov 2026: "I type, it corrects, but I've
//! already typed the next letter, and then it goes wrong").
//!
//! A simulated app on a simulated clock, as Chrome and Electron apps behave on the Mac:
//! - a key the user presses is counted by Sokki's hook at once and reaches the app about half
//!   a millisecond later;
//! - the app handles keys, Sokki's typed fix and Sokki's Accessibility selection one after the
//!   other, in the order they arrive;
//! - what Accessibility shows lags behind the app by a few milliseconds (Chrome, measured in
//!   `docs/research/mac-autocorrect-2026-11.md`), and every Accessibility call takes time.
//!
//! Sokki's thread looks every 15 ms, as `runner.rs` does. After the typing, the text must be
//! exactly what was typed, each typo either fixed or left as typed: never a letter lost or in
//! the wrong place. Made-up sentences only.
use std::cell::{Cell, RefCell};
use std::path::Path;

use sokki_autocorrect::{u16_len, Autocorrect, Field, Held, Outcome};

fn typos() -> sokki_typos::Typos {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    sokki_typos::Typos::load(
        &root.join("mobile/keyboard/src/main/assets/sokki"),
        &root.join("crates/sokki-typos/data"),
    )
    .unwrap()
}

#[derive(Clone, Debug)]
enum Ev {
    /// a key the user typed
    Key(char),
    /// Sokki's typed text (one posted key event)
    Text(String),
    /// Sokki's Accessibility selection
    Select(usize, usize),
}

#[derive(Clone)]
struct State {
    text: Vec<u16>,
    sel: (usize, usize),
}

impl State {
    fn put(&mut self, s: &str) {
        let (at, len) = self.sel;
        let ins: Vec<u16> = s.encode_utf16().collect();
        self.text.splice(at..at + len, ins.iter().copied());
        self.sel = (at + ins.len(), 0);
    }
    fn apply(&mut self, e: &Ev) {
        match e {
            Ev::Key('\u{8}') => {
                let (at, len) = self.sel;
                if len == 0 && at > 0 {
                    self.sel = (at - 1, 1);
                }
                self.put("");
            }
            Ev::Key(c) => self.put(&c.to_string()),
            Ev::Text(s) => self.put(s),
            Ev::Select(a, l) => {
                let n = self.text.len();
                self.sel = ((*a).min(n), (*l).min(n - (*a).min(n)));
            }
        }
    }
}

/// How the simulated app behaves.
#[derive(Clone, Copy)]
struct App {
    /// how far behind the app Accessibility shows the field, ms
    lag: f64,
    /// one Accessibility read, ms
    read: f64,
}

struct Sim {
    app: App,
    now: Cell<f64>,
    /// the user's keys: (time pressed, key)
    script: Vec<(f64, char)>,
    /// keys of the script already handed on (to the app, or held)
    sent: Cell<usize>,
    /// events on their way to the app: (time it arrives, event)
    queue: RefCell<Vec<(f64, Ev)>>,
    state: RefCell<State>,
    /// what Accessibility can show: (time, state), newest last
    history: RefCell<Vec<(f64, State)>>,
    /// Sokki holds the user's keys (the hook keeps them until the swap is done)
    holding: Cell<bool>,
    held: RefCell<Vec<char>>,
    /// longest time keys were held, ms
    longest_hold: Cell<f64>,
    hold_since: Cell<f64>,
    /// the hook can hold keys (Sokki before 27 Nov 2026 couldn't)
    can_hold: bool,
}

const HID: f64 = 0.5;

impl Sim {
    fn new(app: App, script: Vec<(f64, char)>) -> Sim {
        let s = State {
            text: Vec::new(),
            sel: (0, 0),
        };
        Sim {
            app,
            now: Cell::new(0.0),
            script,
            sent: Cell::new(0),
            queue: RefCell::new(Vec::new()),
            state: RefCell::new(s.clone()),
            history: RefCell::new(vec![(f64::MIN, s)]),
            holding: Cell::new(false),
            held: RefCell::new(Vec::new()),
            longest_hold: Cell::new(0.0),
            hold_since: Cell::new(0.0),
            can_hold: std::env::var_os("SOKKI_TEST_NO_HOLD").is_none(),
        }
    }

    /// Keys pressed so far, as the hook counts them.
    fn keys(&self) -> u64 {
        let now = self.now.get();
        self.script.iter().take_while(|(t, _)| *t <= now).count() as u64
    }

    fn post(&self, e: Ev) {
        self.queue.borrow_mut().push((self.now.get() + HID, e));
    }

    /// Lets `ms` pass: keys pressed in that time go to the app (or are held), and the app
    /// handles what has arrived.
    fn pass(&self, ms: f64) {
        let end = self.now.get() + ms;
        loop {
            let next_key = self.script.get(self.sent.get()).map(|(t, _)| *t);
            let next_ev = self
                .queue
                .borrow()
                .iter()
                .map(|(t, _)| *t)
                .fold(f64::INFINITY, f64::min);
            let t = next_key.unwrap_or(f64::INFINITY).min(next_ev);
            if t > end {
                break;
            }
            self.now.set(t.max(self.now.get()));
            if next_key == Some(t) {
                let (_, c) = self.script[self.sent.get()];
                self.sent.set(self.sent.get() + 1);
                if self.holding.get() {
                    if std::env::var_os("SIM_TRACE").is_some() {
                        eprintln!("{t:8.1} held {c:?}");
                    }
                    self.held.borrow_mut().push(c);
                } else {
                    self.post(Ev::Key(c));
                }
                continue;
            }
            let i = {
                let q = self.queue.borrow();
                q.iter()
                    .enumerate()
                    .filter(|(_, (at, _))| *at == t)
                    .map(|(i, _)| i)
                    .next()
                    .unwrap()
            };
            let (_, e) = self.queue.borrow_mut().remove(i);
            self.state.borrow_mut().apply(&e);
            if std::env::var_os("SIM_TRACE").is_some() {
                let st = self.state.borrow();
                eprintln!(
                    "{t:8.1} {e:?} -> {:?} sel {:?}",
                    String::from_utf16_lossy(&st.text),
                    st.sel
                );
            }
            let s = self.state.borrow().clone();
            self.history.borrow_mut().push((t, s));
        }
        self.now.set(end);
    }

    /// What Accessibility shows now.
    fn seen(&self) -> State {
        let at = self.now.get() - self.app.lag;
        let h = self.history.borrow();
        h.iter().rev().find(|(t, _)| *t <= at).unwrap().1.clone()
    }

    fn done(&self) -> bool {
        self.sent.get() == self.script.len() && self.queue.borrow().is_empty()
    }

    fn text(&self) -> String {
        String::from_utf16(&self.state.borrow().text).unwrap()
    }
}

impl Field for Sim {
    fn app(&self) -> &str {
        "com.google.Chrome"
    }
    fn value(&self) -> Option<String> {
        self.pass(self.app.read);
        Some(String::from_utf16(&self.seen().text).unwrap())
    }
    fn selection(&self) -> Option<(usize, usize)> {
        self.pass(self.app.read / 2.0);
        Some(self.seen().sel)
    }
    fn replaceable(&self) -> bool {
        true
    }
    fn select(&self, start: usize, len: usize) -> bool {
        self.post(Ev::Select(start, len));
        self.pass(self.app.read);
        // as mac.rs: wait until Accessibility shows the selection, at most 250 ms
        let until = self.now.get() + 250.0;
        loop {
            if self.selection() == Some((start, len)) {
                return true;
            }
            if self.now.get() > until {
                return false;
            }
            self.pass(3.0);
        }
    }
    fn insert(&self, text: &str) -> bool {
        self.post(Ev::Text(text.to_string()));
        self.pass(0.1);
        true
    }
    fn settle_within(&self, ms: u64, ok: &dyn Fn(&str) -> bool) -> bool {
        let until = self.now.get() + ms as f64;
        loop {
            if self.value().is_some_and(|v| ok(&v)) {
                return true;
            }
            if self.now.get() > until {
                return false;
            }
            self.pass(3.0);
        }
    }
    fn hold_keys(&self) -> Option<Held> {
        if !self.can_hold {
            return None;
        }
        self.holding.set(true);
        self.hold_since.set(self.now.get());
        if std::env::var_os("SIM_TRACE").is_some() {
            eprintln!(
                "{:8.1} hold at {} keys, seen {:?}",
                self.now.get(),
                self.keys(),
                String::from_utf16_lossy(&self.seen().text)
            );
        }
        let now = self.now.get();
        let last = self
            .script
            .iter()
            .take_while(|(t, _)| *t <= now)
            .last()
            .map_or(f64::MIN, |(t, _)| *t);
        Some(Held {
            keys: self.keys(),
            since_last_key_ms: (now - last).clamp(0.0, 1e9) as u64,
        })
    }
    fn release_keys(&self) {
        if !self.holding.get() {
            return;
        }
        self.holding.set(false);
        if std::env::var_os("SIM_TRACE").is_some() {
            eprintln!("{:8.1} release {:?}", self.now.get(), self.held.borrow());
        }
        let took = self.now.get() - self.hold_since.get();
        if took > self.longest_hold.get() {
            self.longest_hold.set(took);
        }
        for c in self.held.borrow_mut().drain(..) {
            self.post(Ev::Key(c));
        }
    }
}

/// A small, fixed random source (the runs are the same every time).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % 1_000_000) as f64 / 1_000_000.0
    }
}

/// The user types `s` at `per_s` keys a second, each gap between half and one and a half
/// times the mean.
fn script(s: &str, per_s: f64, rng: &mut Rng) -> Vec<(f64, char)> {
    let gap = 1000.0 / per_s;
    let mut t = 10.0;
    s.chars()
        .map(|c| {
            t += gap * (0.5 + rng.next());
            (t, c)
        })
        .collect()
}

struct Run {
    text: String,
    fixed: usize,
    longest_hold: f64,
    skipped: Vec<&'static str>,
}

/// Sokki's thread: looks every 15 ms when a key was pressed, or again soon when the field
/// hadn't caught up (as `runner.rs`).
fn run(app: App, s: &str, per_s: f64, seed: u64) -> Run {
    thread_local! {
        static AC: RefCell<Option<Autocorrect>> = const { RefCell::new(None) };
    }
    AC.with(|a| {
        let mut a = a.borrow_mut();
        let ac = a.get_or_insert_with(|| Autocorrect::new(typos(), None));
        ac.reset();
        run_with(ac, app, s, per_s, seed)
    })
}

fn run_with(ac: &mut Autocorrect, app: App, s: &str, per_s: f64, seed: u64) -> Run {
    let mut rng = Rng(seed);
    let sim = Sim::new(app, script(s, per_s, &mut rng));
    let mut seen = 0;
    let mut key_at = 0.0;
    let mut fixed = 0;
    let mut skipped = Vec::new();
    let mut idle = 0;
    while idle < 40 {
        sim.pass(15.0);
        let now = sim.keys();
        let catching_up = ac.stale() && sim.now.get() - key_at < 300.0;
        if now == seen && !catching_up {
            if sim.done() {
                idle += 1;
            }
            continue;
        }
        idle = 0;
        if now != seen {
            seen = now;
            key_at = sim.now.get();
        }
        match ac.on_key(&sim, &|| sim.keys()) {
            Outcome::Fixed { .. } => fixed += 1,
            Outcome::Skipped(why) => skipped.push(why),
            Outcome::Missed { why, .. } => skipped.push(why),
            _ => {}
        }
        assert!(!sim.holding.get(), "keys still held after a look");
    }
    Run {
        text: sim.text(),
        fixed,
        longest_hold: sim.longest_hold.get(),
        skipped,
    }
}

/// Typed with two typos; each may end up fixed or as typed, nothing else.
const TYPED: &str = "Das ist nihct gut und wroks nicht so ";
fn acceptable(text: &str) -> bool {
    [
        "Das ist nicht gut und works nicht so ",
        "Das ist nihct gut und works nicht so ",
        "Das ist nicht gut und wroks nicht so ",
        "Das ist nihct gut und wroks nicht so ",
    ]
    .contains(&text)
}

#[test]
fn trace_one() {
    if let Ok(v) = std::env::var("SIM_TRACE") {
        let mut it = v.split(',').map(|x| x.parse::<f64>().unwrap());
        let (per_s, lag, seed) = (
            it.next().unwrap(),
            it.next().unwrap(),
            it.next().unwrap() as u64,
        );
        let r = run(App { lag, read: 2.0 }, TYPED, per_s, seed * 7919);
        eprintln!("{:?} {:?}", r.text, r.skipped);
    }
}

#[test]
fn slow_typing_in_an_app_that_shows_keys_at_once_is_fixed() {
    let r = run(
        App {
            lag: 0.0,
            read: 1.0,
        },
        TYPED,
        5.0,
        1,
    );
    assert_eq!(r.text, "Das ist nicht gut und works nicht so ");
    assert_eq!(r.fixed, 2);
}

/// The sweep behind the numbers in `docs/research/autocorrect-typing-race-2026-11.md`:
/// 8 to 20 keys a second, Accessibility 0 to 30 ms behind, 40 runs each.
#[test]
fn fast_typing_never_loses_or_moves_a_key() {
    let mut wrong = Vec::new();
    let (mut runs, mut fixes, mut longest) = (0, 0, 0.0f64);
    let mut skipped: std::collections::BTreeMap<&str, usize> = Default::default();
    // per lag: wrong runs, typos fixed, longest hold
    let mut by_lag: std::collections::BTreeMap<u32, (usize, usize, f64)> = Default::default();
    for per_s in [8.0, 12.0, 15.0, 20.0] {
        for lag in [0.0, 5.0, 10.0, 20.0, 30.0] {
            for seed in 1..=40u64 {
                let r = run(App { lag, read: 2.0 }, TYPED, per_s, seed * 7919);
                runs += 1;
                fixes += r.fixed;
                let cell = by_lag.entry(lag as u32).or_default();
                cell.0 += usize::from(!acceptable(&r.text));
                cell.1 += r.fixed;
                cell.2 = cell.2.max(r.longest_hold);
                longest = longest.max(r.longest_hold);
                for s in r.skipped {
                    *skipped.entry(s).or_default() += 1;
                }
                if !acceptable(&r.text) {
                    wrong.push(format!(
                        "{per_s}/s, {lag} ms behind, seed {seed}: {:?}",
                        r.text
                    ));
                }
            }
        }
    }
    eprintln!(
        "fast typing: {runs} runs, {} wrong, {fixes} of {} typos fixed, keys held at most {longest:.1} ms, skipped {skipped:?}",
        wrong.len(),
        runs * 2
    );
    for (lag, (w, f, h)) in &by_lag {
        eprintln!("  {lag} ms behind: {w} of 160 runs wrong, {f} of 320 typos fixed, keys held at most {h:.0} ms");
    }
    for w in wrong.iter().take(8) {
        eprintln!("  {w}");
    }
    assert!(
        wrong.is_empty(),
        "{} of {runs} runs ended wrong",
        wrong.len()
    );
}

#[test]
fn a_key_typed_while_the_word_is_swapped_lands_after_it() {
    // the next word's first letter comes 30 ms after the space, the app shows keys 10 ms late
    let typed = "it wroks fine ";
    let mut t = 0.0;
    let script: Vec<(f64, char)> = typed
        .chars()
        .map(|c| {
            t += if c == 'f' { 30.0 } else { 120.0 };
            (t, c)
        })
        .collect();
    let sim = Sim::new(
        App {
            lag: 10.0,
            read: 2.0,
        },
        script,
    );
    let mut ac = Autocorrect::new(typos(), None);
    let mut seen = 0;
    for _ in 0..200 {
        sim.pass(15.0);
        if sim.keys() != seen || ac.stale() {
            seen = sim.keys();
            ac.on_key(&sim, &|| sim.keys());
        }
    }
    let text = sim.text();
    assert!(
        text == "it works fine " || text == "it wroks fine ",
        "{text:?}"
    );
    assert_eq!(u16_len(&text), sim.state.borrow().sel.0);
}

#[test]
fn backspace_right_after_a_fix_still_brings_the_word_back_while_keys_are_held() {
    let typed = "it wroks \u{8}";
    let mut t = 0.0;
    let script: Vec<(f64, char)> = typed
        .chars()
        .map(|c| {
            t += 150.0;
            (t, c)
        })
        .collect();
    let sim = Sim::new(
        App {
            lag: 10.0,
            read: 2.0,
        },
        script,
    );
    let mut ac = Autocorrect::new(typos(), None);
    let (mut seen, mut got) = (0, Vec::new());
    for _ in 0..200 {
        sim.pass(15.0);
        if sim.keys() != seen || ac.stale() {
            seen = sim.keys();
            let o = ac.on_key(&sim, &|| sim.keys());
            if o != Outcome::Nothing {
                got.push(o);
            }
        }
    }
    assert_eq!(sim.text(), "it wroks");
    assert!(
        matches!(got.last(), Some(Outcome::Undone { .. })),
        "{got:?}"
    );
}
