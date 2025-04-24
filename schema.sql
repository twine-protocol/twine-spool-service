-- Command to run: wrangler d1 execute spool-dev --local --file=./schema.sql
-- If starting fresh...
-- DROP TABLE IF EXISTS Tixels;
-- DROP TABLE IF EXISTS Strands;
-- DROP TABLE IF EXISTS Registrations;
-- DROP TABLE IF EXISTS ApiKeys;

CREATE TABLE IF NOT EXISTS Strands (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  -- Cid bytes (2x varint (9) + 512bit hash (64)) = 18 + 64 = 82
  cid BINARY(82) UNIQUE NOT NULL,
  spec TEXT NOT NULL,
  data BLOB NOT NULL,
  details JSON DEFAULT '{}',
  writable BOOLEAN DEFAULT 1 NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_strands_cid ON Strands (cid);

CREATE TABLE IF NOT EXISTS Tixels (
  cid BINARY(82) UNIQUE NOT NULL,
  strand INTEGER NOT NULL,
  idx INTEGER NOT NULL,
  data BLOB NOT NULL,

  -- Keys
  PRIMARY KEY (strand, idx),
  FOREIGN KEY (strand) REFERENCES Strands(id)
);

CREATE INDEX IF NOT EXISTS idx_tixels_cid ON Tixels (cid);

CREATE TABLE IF NOT EXISTS Registrations (
  uuid TEXT PRIMARY KEY,
  email TEXT NOT NULL,
  status TEXT NOT NULL,
  strand_cid BINARY(82) UNIQUE NOT NULL,
  strand BLOB
);

-- Api Keys
CREATE TABLE IF NOT EXISTS ApiKeys (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  description TEXT NOT NULL,
  hashed_key TEXT UNIQUE NOT NULL,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  last_used_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  expires_at TIMESTAMP
);
