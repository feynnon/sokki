-- One row per opted-in install: a random id, never a name.
CREATE TABLE IF NOT EXISTS installs (
  id TEXT PRIMARY KEY,
  clips INTEGER NOT NULL,
  seconds TEXT NOT NULL,   -- JSON: {"de": 3600.5, "en": 120}
  updated INTEGER NOT NULL, -- unix seconds
  donate INTEGER NOT NULL DEFAULT 0 -- 1: data given away to keep Verbatim free
);

-- The total, computed at most every 10 minutes (so /total reads one row).
CREATE TABLE IF NOT EXISTS cache (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL,
  updated INTEGER NOT NULL
);
