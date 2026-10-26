-- Every release has exactly one publication audit, including file activation.
-- This join index prevents a history page from scanning the growing audit table.
CREATE UNIQUE INDEX tool_publications_revision
 ON tool_publications(workspace,profile,revision);
PRAGMA user_version=7;
