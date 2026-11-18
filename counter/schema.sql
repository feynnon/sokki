-- One row per counted install (either choice on): a random id, never a name.
CREATE TABLE IF NOT EXISTS installs (
  id TEXT PRIMARY KEY,
  clips INTEGER NOT NULL,
  seconds TEXT NOT NULL,   -- JSON: {"de": 3600.5, "en": 120}
  updated INTEGER NOT NULL, -- unix seconds
  donate INTEGER NOT NULL DEFAULT 0 -- 1: supports Sokki & Open Source
);

-- The total, computed at most every 10 minutes (so /total reads one row).
CREATE TABLE IF NOT EXISTS cache (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL,
  updated INTEGER NOT NULL
);

-- "Help improve Sokki": one report per install and day (UTC).
CREATE TABLE IF NOT EXISTS usage (
  id TEXT NOT NULL,
  day TEXT NOT NULL,       -- "2026-11-18"
  report TEXT NOT NULL,    -- JSON, see the app's voice_data.rs usage_report
  received INTEGER NOT NULL,
  PRIMARY KEY (id, day)
);

-- "I'm interested" answers to a buyer message.
CREATE TABLE IF NOT EXISTS interest (
  id TEXT NOT NULL,
  notice INTEGER NOT NULL, -- the message's notice_id
  language TEXT NOT NULL,
  hours INTEGER NOT NULL,
  received INTEGER NOT NULL,
  PRIMARY KEY (id, notice)
);

-- "Support Sokki & Open Source": one row per uploaded recording; the audio
-- itself is in Workers KV under <id>/<clip>.flac.
CREATE TABLE IF NOT EXISTS donations (
  id TEXT NOT NULL,
  clip TEXT NOT NULL,      -- "dictation-20261118-174633123-ab12"
  language TEXT NOT NULL,
  source TEXT NOT NULL,    -- "dictation" or "meeting"
  seconds REAL NOT NULL,
  bytes INTEGER NOT NULL,  -- FLAC size
  text TEXT NOT NULL,      -- the transcript (no names or numbers)
  received INTEGER NOT NULL,
  PRIMARY KEY (id, clip)
);

-- "Tell me about paid recording tasks": who ticked it, and the languages
-- of their own recordings (a task could be offered in those).
CREATE TABLE IF NOT EXISTS tasks (
  id TEXT PRIMARY KEY,
  languages TEXT NOT NULL, -- JSON: ["de", "en"]
  updated INTEGER NOT NULL
);
