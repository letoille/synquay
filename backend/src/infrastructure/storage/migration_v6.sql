CREATE TABLE tool_drafts (
 workspace TEXT NOT NULL, profile TEXT NOT NULL, generation INTEGER NOT NULL CHECK(generation>0),
 base_revision INTEGER NOT NULL, exposed_tools TEXT NOT NULL, actor TEXT NOT NULL, updated_ms INTEGER NOT NULL,
 PRIMARY KEY(workspace,profile),
 FOREIGN KEY(workspace,profile,base_revision) REFERENCES tool_releases(workspace,profile,revision)
);
CREATE TABLE tool_draft_audit (
 workspace TEXT NOT NULL,profile TEXT NOT NULL,generation INTEGER NOT NULL,
 base_revision INTEGER NOT NULL,exposed_tools TEXT NOT NULL,actor TEXT NOT NULL,occurred_ms INTEGER NOT NULL,
 PRIMARY KEY(workspace,profile,generation)
);
CREATE TABLE tool_publish_operations (
 workspace TEXT NOT NULL,profile TEXT NOT NULL,operation TEXT NOT NULL,
 actor TEXT NOT NULL,expected_revision INTEGER NOT NULL,draft_generation INTEGER NOT NULL,
 revision INTEGER NOT NULL,occurred_ms INTEGER NOT NULL,
 PRIMARY KEY(workspace,profile,operation),
 FOREIGN KEY(workspace,profile,revision) REFERENCES tool_releases(workspace,profile,revision)
);
-- Existing composite primary keys cover draft lookup, generation CAS, operation
-- replay lookup and revision history. No redundant secondary indexes.
PRAGMA user_version=6;
