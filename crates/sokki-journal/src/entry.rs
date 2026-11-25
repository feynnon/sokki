//! One journal entry per piece of writing, and the bookkeeping that decides
//! when a piece is finished. The same rules and line format as the phone's
//! keyboard journal (`mobile/keyboard/.../dev/verbatim/sokki/Journal.kt`):
//! a field's text is finished when the field is emptied (a message sent) or
//! left (focus moved, another app, a password field).
//!
//! Which characters were typed: between two reads, text that grew by at
//! most one character per key press is typed. A bigger jump (a paste, a
//! dictation, autocomplete), Sokki's own dictations and whatever the field
//! held when it got focus are not. Word learning reads only typed words.

use serde::{Deserialize, Serialize};

/// A dictation Sokki pasted into the field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dictated {
    /// The speech model's text, before filler removal and word fixes.
    pub raw: String,
    /// What Sokki pasted, when it differs from `raw` (Mac only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pasted: Option<String>,
    /// When it was pasted, ms since 1970.
    pub at: u64,
}

/// One line of `journal-YYYY-MM-DD.jsonl`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Always "entry".
    pub t: String,
    /// When the field got focus, ms since 1970.
    pub opened: u64,
    /// When its text last changed.
    pub written: u64,
    /// The app: its bundle id on the Mac, its package on the phone.
    pub app: String,
    /// The field's placeholder or name, at most 80 characters.
    pub hint: String,
    /// The text the field held when it got focus.
    pub prefill: String,
    /// The text when it was sent or left.
    pub text: String,
    /// Dictations pasted into this field while it was being written.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dictated: Vec<Dictated>,
    /// The parts of `text` typed key by key, as [start, end) character
    /// ranges. Missing: not known (older or phone entries).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub typed: Option<Vec<[usize; 2]>>,
}

/// What a read of the focused field found. `K` tells fields apart.
#[derive(Debug, Clone)]
pub struct Observation<K> {
    pub key: K,
    pub app: String,
    pub hint: String,
    pub text: String,
}

struct Field<K> {
    key: K,
    app: String,
    hint: String,
    opened: u64,
    prefill: String,
    last: String,
    last_change: Option<u64>,
    dictated: Vec<Dictated>,
    /// Per character of `last`: typed key by key.
    typed: Vec<bool>,
    /// Key presses not yet matched by a change: a key is counted a moment
    /// before its character reaches the field.
    credit: u64,
}

/// [start, end) ranges of the `true` runs.
fn runs(flags: &[bool]) -> Vec<[usize; 2]> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, &f) in flags.iter().chain(std::iter::once(&false)).enumerate() {
        match (f, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                out.push([s, i]);
                start = None;
            }
            _ => {}
        }
    }
    out
}

impl<K> Field<K> {
    fn open(now: u64, o: Observation<K>) -> Self {
        Field {
            key: o.key,
            app: o.app,
            hint: o.hint.chars().take(80).collect(),
            opened: now,
            prefill: o.text.clone(),
            typed: vec![false; o.text.chars().count()],
            last: o.text,
            last_change: None,
            dictated: Vec::new(),
            credit: 0,
        }
    }

    /// The entry for what this field holds now, if anything was written.
    fn entry(&self) -> Option<Entry> {
        let written = self.last_change?;
        if self.last.trim().is_empty() || self.last == self.prefill {
            return None;
        }
        Some(Entry {
            t: "entry".into(),
            opened: self.opened,
            written,
            app: self.app.clone(),
            hint: self.hint.clone(),
            prefill: self.prefill.clone(),
            text: self.last.clone(),
            dictated: self.dictated.clone(),
            typed: Some(runs(&self.typed)),
        })
    }

    /// `keys`: key presses since the last read.
    fn update(&mut self, now: u64, keys: u64, text: String) -> Option<Entry> {
        if text == self.last {
            // Keep them for the change they are about to make (at most a few).
            self.credit = (self.credit + keys).min(8);
            return None;
        }
        let keys = keys + std::mem::take(&mut self.credit);
        if text.trim().is_empty() && !self.last.trim().is_empty() {
            // Emptied: the message was sent. The field stays focused for the next one.
            let sent = self.entry();
            self.prefill = String::new();
            self.typed = vec![false; text.chars().count()];
            self.last = text;
            self.last_change = None;
            self.dictated.clear();
            return sent;
        }
        // What changed: the part between the common start and the common end.
        let old: Vec<char> = self.last.chars().collect();
        let new: Vec<char> = text.chars().collect();
        let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
        let room = old.len().min(new.len()) - prefix;
        let suffix = old
            .iter()
            .rev()
            .zip(new.iter().rev())
            .take(room)
            .take_while(|(a, b)| a == b)
            .count();
        let inserted: String = new[prefix..new.len() - suffix].iter().collect();
        let count = inserted.chars().count() as u64;
        let from_dictation = count >= 3
            && self.dictated.iter().any(|d| {
                d.raw.contains(inserted.trim())
                    || d.pasted
                        .as_deref()
                        .is_some_and(|p| p.contains(inserted.trim()))
            });
        let typed = count > 0 && count <= keys.saturating_add(1) && keys > 0 && !from_dictation;
        let mut flags = self.typed[..prefix.min(self.typed.len())].to_vec();
        flags.extend(std::iter::repeat_n(typed, count as usize));
        let keep_from = old.len().saturating_sub(suffix).min(self.typed.len());
        flags.extend_from_slice(&self.typed[keep_from..]);
        flags.resize(new.len(), false);
        self.typed = flags;
        self.last = text;
        self.last_change = Some(now);
        None
    }
}

/// How long a dictation waits for a field to attach to.
const DICTATION_WAIT_MS: u64 = 5_000;

/// Follows the focused field read by read and hands out finished entries.
pub struct Tracker<K> {
    current: Option<Field<K>>,
    pending: Vec<Dictated>,
}

impl<K: PartialEq> Default for Tracker<K> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: PartialEq> Tracker<K> {
    pub fn new() -> Self {
        Tracker {
            current: None,
            pending: Vec::new(),
        }
    }

    /// One read at `now` (ms since 1970), `keys` key presses after the
    /// previous one. `None`: no field Sokki may read (none focused, a
    /// password field, secure input, an excluded app).
    pub fn observe(&mut self, now: u64, keys: u64, read: Option<Observation<K>>) -> Vec<Entry> {
        let mut done = Vec::new();
        // A dictation pasted since the last read belongs to the focused field
        // before its text is compared, so its words don't count as typed.
        if let Some(field) = self.current.as_mut() {
            field.dictated.append(&mut self.pending);
        }
        match read {
            None => done.extend(self.leave()),
            Some(o) => match self.current.as_mut() {
                Some(field) if field.key == o.key => done.extend(field.update(now, keys, o.text)),
                _ => {
                    done.extend(self.leave());
                    self.current = Some(Field::open(now, o));
                }
            },
        }
        // A dictation belongs to the field that is focused once it was pasted.
        if let Some(field) = self.current.as_mut() {
            field.dictated.append(&mut self.pending);
        } else {
            self.pending
                .retain(|d| now.saturating_sub(d.at) < DICTATION_WAIT_MS);
        }
        done
    }

    /// Sokki pasted a dictation; the next read attaches it to its field.
    pub fn dictated(&mut self, d: Dictated) {
        self.pending.push(d);
    }

    /// Finishes the field being written (the journal was switched off, the
    /// app quits). Returns its entry, if anything was written.
    pub fn leave(&mut self) -> Option<Entry> {
        self.current.take().and_then(|f| f.entry())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(key: u32, text: &str) -> Option<Observation<u32>> {
        Some(Observation {
            key,
            app: "com.example.chat".into(),
            hint: "Message".into(),
            text: text.into(),
        })
    }

    #[test]
    fn a_sent_message_is_one_entry() {
        let mut t = Tracker::new();
        assert!(t.observe(1, 100, read(1, "")).is_empty());
        assert!(t.observe(2, 100, read(1, "Hallo Jorinde")).is_empty());
        assert!(t
            .observe(3, 100, read(1, "Hallo Jorinde, bis morgen"))
            .is_empty());
        let sent = t.observe(4, 100, read(1, ""));
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].text, "Hallo Jorinde, bis morgen");
        assert_eq!((sent[0].opened, sent[0].written), (1, 3));
        assert_eq!(sent[0].prefill, "");
        // The next message in the same field is its own entry.
        t.observe(5, 100, read(1, "Noch was"));
        let next = t.observe(6, 100, read(1, ""));
        assert_eq!(next[0].text, "Noch was");
        assert_eq!(next[0].opened, 1);
    }

    #[test]
    fn leaving_a_field_finishes_it() {
        let mut t = Tracker::new();
        t.observe(1, 100, read(1, "Entwurf"));
        t.observe(2, 100, read(1, "Entwurf für Brannwick"));
        let left = t.observe(3, 100, read(2, "anderes Feld"));
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].prefill, "Entwurf");
        assert_eq!(left[0].text, "Entwurf für Brannwick");
        // A password field or another app: the field is left too.
        t.observe(4, 100, read(2, "anderes Feld, geändert"));
        assert_eq!(t.observe(5, 100, None)[0].text, "anderes Feld, geändert");
    }

    #[test]
    fn nothing_written_is_no_entry() {
        let mut t = Tracker::new();
        t.observe(1, 100, read(1, "already there"));
        assert!(t.observe(2, 100, read(2, "")).is_empty());
        // Changed and changed back to what it held.
        t.observe(3, 100, read(3, "a"));
        t.observe(4, 100, read(3, "ab"));
        t.observe(5, 100, read(3, "a"));
        assert!(t.observe(6, 100, None).is_empty());
    }

    #[test]
    fn a_dictation_attaches_to_its_field() {
        let mut t = Tracker::new();
        t.observe(1, 100, read(1, ""));
        t.dictated(Dictated {
            raw: "das läuft auf Fassel".into(),
            pasted: None,
            at: 2,
        });
        t.observe(3, 100, read(1, "das läuft auf Fassel"));
        t.observe(4, 100, read(1, "das läuft auf Vercel"));
        let sent = t.observe(5, 100, read(1, ""));
        assert_eq!(sent[0].dictated.len(), 1);
        assert_eq!(sent[0].dictated[0].raw, "das läuft auf Fassel");
        assert_eq!(sent[0].text, "das läuft auf Vercel");
        // The next message starts without it.
        t.observe(6, 100, read(1, "ok"));
        assert!(t.observe(7, 100, read(1, ""))[0].dictated.is_empty());
    }

    #[test]
    fn a_dictation_without_a_field_is_dropped_after_a_while() {
        let mut t: Tracker<u32> = Tracker::new();
        t.dictated(Dictated {
            raw: "x".into(),
            pasted: None,
            at: 0,
        });
        t.observe(DICTATION_WAIT_MS + 1, 100, None);
        t.observe(DICTATION_WAIT_MS + 2, 100, read(1, ""));
        t.observe(DICTATION_WAIT_MS + 3, 100, read(1, "y"));
        assert!(t.observe(DICTATION_WAIT_MS + 4, 100, None)[0]
            .dictated
            .is_empty());
    }

    #[test]
    fn typed_pasted_and_dictated_are_told_apart() {
        let mut t = Tracker::new();
        t.observe(1, 0, read(1, "Re: Angebot"));
        // Typed one or two keys at a time.
        t.observe(2, 1, read(1, "Re: Angebot H"));
        t.observe(3, 2, read(1, "Re: Angebot Hal"));
        t.observe(4, 2, read(1, "Re: Angebot Hallo"));
        t.observe(4, 1, read(1, "Re: Angebot Hallo"));
        t.observe(4, 0, read(1, "Re: Angebot Hallo!"));
        t.observe(4, 0, read(1, "Re: Angebot Hallo"));
        // A key counted before its character arrives still counts.
        t.observe(4, 1, read(1, "Re: Angebot Hallo"));
        // A paste: many characters on one key (⌘V).
        t.observe(5, 1, read(1, "Re: Angebot Hallo Kelvaro Mendtz"));
        // A dictation by Sokki.
        t.dictated(Dictated {
            raw: "läuft auf Fassel".into(),
            pasted: None,
            at: 5,
        });
        t.observe(
            6,
            0,
            read(1, "Re: Angebot Hallo Kelvaro Mendtz läuft auf Fassel"),
        );
        // A typed fix in the middle: "Fassel" → "Vercel", key by key.
        t.observe(7, 6, read(1, "Re: Angebot Hallo Kelvaro Mendtz läuft auf "));
        t.observe(
            8,
            6,
            read(1, "Re: Angebot Hallo Kelvaro Mendtz läuft auf Vercel"),
        );
        let e = t.observe(9, 1, None).remove(0);
        let typed: Vec<String> = e
            .typed
            .clone()
            .unwrap()
            .iter()
            .map(|[a, b]| e.text.chars().skip(*a).take(b - a).collect())
            .collect();
        assert_eq!(typed, [" Hallo", "Vercel"], "{e:?}");
    }

    #[test]
    fn the_line_format_matches_the_phone() {
        let e = Entry {
            t: "entry".into(),
            opened: 1,
            written: 2,
            app: "com.example".into(),
            hint: "".into(),
            prefill: "".into(),
            text: "hi".into(),
            dictated: vec![],
            typed: None,
        };
        assert_eq!(
            serde_json::to_string(&e).unwrap(),
            r#"{"t":"entry","opened":1,"written":2,"app":"com.example","hint":"","prefill":"","text":"hi"}"#
        );
    }
}
