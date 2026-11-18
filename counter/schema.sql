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
