-- If starting fresh...
-- DROP TABLE IF EXISTS Strands;
-- DROP TABLE IF EXISTS Tixels;

CREATE TABLE IF NOT EXISTS Strands (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  -- Cid bytes (2x varint (9) + 512bit hash (64)) = 18 + 64 = 82
  cid BINARY(82) UNIQUE NOT NULL,
  spec TEXT NOT NULL,
  data BLOB NOT NULL,
  details JSON DEFAULT '{}'
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
