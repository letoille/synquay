-- Bounded operator-defined devices. Scoped primary keys serve all read/update paths.
CREATE TABLE IF NOT EXISTS connection_devices (
 workspace TEXT NOT NULL, connection TEXT NOT NULL, record TEXT NOT NULL,
 PRIMARY KEY(workspace,connection)
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS resource_state (
 workspace TEXT NOT NULL, connection TEXT NOT NULL, resource TEXT NOT NULL, state_group TEXT NOT NULL, field TEXT NOT NULL,
 value TEXT NOT NULL, received_ms INTEGER NOT NULL, source_ms INTEGER, retained INTEGER NOT NULL,
 PRIMARY KEY(workspace,connection,resource,state_group,field)
) WITHOUT ROWID;
PRAGMA user_version=20;
