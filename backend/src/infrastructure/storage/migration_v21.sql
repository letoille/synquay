-- Display names are scoped by the existing conversation primary key.
ALTER TABLE connection_conversations ADD COLUMN display_name TEXT;
PRAGMA user_version=21;
