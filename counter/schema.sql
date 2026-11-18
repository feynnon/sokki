-- One row per opted-in install: a random id, never a name.
CREATE TABLE IF NOT EXISTS installs (
  id TEXT PRIMARY KEY,
  clips INTEGER NOT NULL,
  seconds TEXT NOT NULL,   -- JSON: {"de": 3600.5, "en": 120}
  updated INTEGER NOT NULL -- unix seconds
);
