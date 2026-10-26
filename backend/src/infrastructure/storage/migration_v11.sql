CREATE TABLE agent_binding(singleton INTEGER PRIMARY KEY CHECK(singleton=1), record TEXT NOT NULL, job_count INTEGER NOT NULL DEFAULT 0);
CREATE TABLE agent_jobs(
 workspace TEXT NOT NULL, job_id TEXT NOT NULL, source TEXT NOT NULL, profile TEXT NOT NULL,
 state TEXT NOT NULL CHECK(state IN ('queued','claimed','completed','failed','expired')),
 attempts INTEGER NOT NULL DEFAULT 0, due_ms INTEGER NOT NULL, deadline_ms INTEGER NOT NULL,
 owner TEXT, client TEXT, claim_id TEXT, lease_until_ms INTEGER,
 result_hash TEXT, delivery_id TEXT,
 PRIMARY KEY(workspace,job_id)
);
-- Queue, lease concurrency, expiry sweep and seek pagination have separate access paths.
CREATE INDEX agent_jobs_due ON agent_jobs(workspace,profile,state,due_ms,job_id);
CREATE INDEX agent_jobs_lease ON agent_jobs(workspace,profile,state,lease_until_ms);
CREATE INDEX agent_jobs_deadline ON agent_jobs(workspace,state,deadline_ms);
CREATE INDEX agent_jobs_page ON agent_jobs(workspace,profile,job_id);
CREATE TRIGGER agent_jobs_count AFTER INSERT ON agent_jobs BEGIN
 UPDATE agent_binding SET job_count=job_count+1 WHERE singleton=1;
END;
CREATE TABLE agent_claims(
 workspace TEXT NOT NULL, profile TEXT NOT NULL, principal TEXT NOT NULL, client TEXT NOT NULL,
 claim_id TEXT NOT NULL, job_id TEXT NOT NULL, generation INTEGER NOT NULL,
 PRIMARY KEY(workspace,profile,principal,client,claim_id)
);
PRAGMA user_version=11;
