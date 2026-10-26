-- Existing source identity UNIQUE index also covers fingerprint lookups; no extra index needed.
ALTER TABLE events ADD COLUMN fingerprint TEXT;
-- Immutable catalog releases; reverse PK scan serves the latest version lookup.
CREATE TABLE mcp_catalog_releases (
 workspace TEXT NOT NULL, profile TEXT NOT NULL, revision INTEGER NOT NULL,
 record TEXT NOT NULL, actor TEXT NOT NULL, created_ms INTEGER NOT NULL,
 PRIMARY KEY(workspace,profile,revision)
);
PRAGMA user_version=10;
