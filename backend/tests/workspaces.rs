use std::{
    collections::{BTreeSet, HashSet},
    sync::Arc,
};
use synquay_server::{
    application::{
        ports::{PortError, PortFuture, identity::PolicyEngine, workspaces::WorkspaceRepository},
        services::workspaces::WorkspacePolicy,
    },
    domain::{
        connector::ConnectionScope,
        identity::{Action, Identity, Resource},
        workspace::*,
    },
    infrastructure::storage::SqliteStore,
};
use uuid::Uuid;
fn id(v: u128) -> Uuid {
    Uuid::from_u128(v)
}
fn space(v: u128) -> Workspace {
    Workspace {
        id: id(v),
        name: format!("Space {v}"),
        state: WorkspaceState::Active,
        revision: 1,
    }
}
fn member(ws: u128, p: u128, role: WorkspaceRole) -> Membership {
    Membership {
        workspace_id: id(ws),
        principal_id: id(p),
        role,
        state: MemberState::Active,
    }
}
fn update(revision: u64, change: WorkspaceChange) -> WorkspaceUpdate {
    WorkspaceUpdate {
        expected_revision: revision,
        change,
    }
}
fn change(p: u128, role: WorkspaceRole, state: MemberState) -> WorkspaceChange {
    WorkspaceChange::SetMember {
        principal_id: id(p),
        role,
        state,
    }
}

#[tokio::test]
async fn bootstrap_is_idempotent_and_restarts_never_resurrect_members() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("spaces.db");
    let store = SqliteStore::open(&path).await.unwrap();
    store
        .bootstrap(
            space(1),
            id(10),
            vec![member(1, 11, WorkspaceRole::Operator)],
        )
        .await
        .unwrap();
    store
        .update(
            id(1),
            id(10),
            Uuid::new_v4(),
            update(1, change(11, WorkspaceRole::Operator, MemberState::Revoked)),
        )
        .await
        .unwrap();
    drop(store);
    let store = SqliteStore::open(&path).await.unwrap();
    store
        .bootstrap(
            space(1),
            id(10),
            vec![
                member(1, 11, WorkspaceRole::Operator),
                member(1, 12, WorkspaceRole::Operator),
            ],
        )
        .await
        .unwrap();
    assert!(!store.access(id(1), id(11)).await.unwrap().active());
    assert!(matches!(
        store.access(id(1), id(12)).await,
        Err(PortError::PermissionDenied)
    ));
    assert_eq!(
        store
            .access(id(1), id(10))
            .await
            .unwrap()
            .workspace
            .revision,
        2
    );
    assert!(matches!(
        store.access(id(2), id(10)).await,
        Err(PortError::PermissionDenied)
    ));
}

#[tokio::test]
async fn only_owner_changes_members_and_last_owner_cannot_be_revoked() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("spaces.db"))
        .await
        .unwrap();
    store
        .bootstrap(
            space(1),
            id(10),
            vec![
                member(1, 11, WorkspaceRole::Administrator),
                member(1, 12, WorkspaceRole::Operator),
            ],
        )
        .await
        .unwrap();
    assert!(matches!(
        store
            .update(
                id(1),
                id(10),
                Uuid::new_v4(),
                update(1, change(10, WorkspaceRole::Owner, MemberState::Revoked))
            )
            .await,
        Err(PortError::Conflict)
    ));
    assert!(matches!(
        store
            .update(
                id(1),
                id(11),
                Uuid::new_v4(),
                update(1, change(12, WorkspaceRole::Owner, MemberState::Active))
            )
            .await,
        Err(PortError::PermissionDenied)
    ));
    assert!(matches!(
        store
            .update(
                id(1),
                id(12),
                Uuid::new_v4(),
                update(
                    1,
                    WorkspaceChange::Rename {
                        name: "Forged".into()
                    }
                )
            )
            .await,
        Err(PortError::PermissionDenied)
    ));
    store
        .update(
            id(1),
            id(11),
            Uuid::new_v4(),
            update(
                1,
                WorkspaceChange::Rename {
                    name: "Renamed".into(),
                },
            ),
        )
        .await
        .unwrap();
    store
        .update(
            id(1),
            id(10),
            Uuid::new_v4(),
            update(2, change(12, WorkspaceRole::Owner, MemberState::Active)),
        )
        .await
        .unwrap();
    store
        .update(
            id(1),
            id(10),
            Uuid::new_v4(),
            update(3, change(10, WorkspaceRole::Owner, MemberState::Revoked)),
        )
        .await
        .unwrap();
    assert!(store.access(id(1), id(12)).await.unwrap().manages());
    assert!(!store.access(id(1), id(10)).await.unwrap().manages());
}

#[tokio::test]
async fn suspend_retains_recovery_authority_and_cas_serializes_changes() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("spaces.db"))
        .await
        .unwrap();
    store.bootstrap(space(1), id(10), vec![]).await.unwrap();
    let (a, b) = tokio::join!(
        store.update(
            id(1),
            id(10),
            Uuid::new_v4(),
            update(
                1,
                WorkspaceChange::SetState {
                    state: WorkspaceState::Suspended
                }
            )
        ),
        store.update(
            id(1),
            id(10),
            Uuid::new_v4(),
            update(
                1,
                WorkspaceChange::Rename {
                    name: "Race".into()
                }
            )
        )
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert!(matches!(a, Err(PortError::Conflict)) || matches!(b, Err(PortError::Conflict)));
    store
        .update(
            id(1),
            id(10),
            Uuid::new_v4(),
            update(
                2,
                WorkspaceChange::SetState {
                    state: WorkspaceState::Suspended,
                },
            ),
        )
        .await
        .unwrap();
    let access = store.access(id(1), id(10)).await.unwrap();
    assert!(access.manages());
    assert!(!access.active());
    store
        .update(
            id(1),
            id(10),
            Uuid::new_v4(),
            update(
                3,
                WorkspaceChange::SetState {
                    state: WorkspaceState::Active,
                },
            ),
        )
        .await
        .unwrap();
    assert!(store.access(id(1), id(10)).await.unwrap().active());
}

struct InnerPolicy {
    allow: bool,
}
impl PolicyEngine for InnerPolicy {
    fn authorize(&self, _: Identity, _: Action, _: Resource) -> PortFuture<'_, ()> {
        Box::pin(async move {
            if self.allow {
                Ok(())
            } else {
                Err(PortError::PermissionDenied)
            }
        })
    }
}
#[tokio::test]
async fn membership_roles_and_resource_grants_are_intersected_and_rechecked() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(
        SqliteStore::open(dir.path().join("spaces.db"))
            .await
            .unwrap(),
    );
    store
        .bootstrap(space(1), id(10), vec![member(1, 11, WorkspaceRole::Viewer)])
        .await
        .unwrap();
    let scope = ConnectionScope {
        workspace_id: id(1),
        connection_id: id(2),
    };
    let principal = Identity {
        mcp_endpoint: None,
        workspace_id: id(1),
        principal_id: id(11),
        client_id: None,
        scopes: BTreeSet::new(),
        expires_at: None,
    };
    let tool = Resource::Tool {
        scope,
        tool_id: "read".into(),
    };
    let mut policy = WorkspacePolicy {
        repository: store.clone(),
        inner: Arc::new(InnerPolicy { allow: true }),
        read_only_tools: Arc::new(std::sync::RwLock::new(HashSet::from([(
            scope,
            "read".into(),
        )]))),
    };
    policy
        .authorize(principal.clone(), Action::InvokeTool, tool.clone())
        .await
        .unwrap();
    assert!(matches!(
        policy
            .authorize(
                principal.clone(),
                Action::InvokeTool,
                Resource::Tool {
                    scope,
                    tool_id: "write".into()
                }
            )
            .await,
        Err(PortError::PermissionDenied)
    ));
    policy.inner = Arc::new(InnerPolicy { allow: false });
    assert!(matches!(
        policy
            .authorize(principal.clone(), Action::InvokeTool, tool.clone())
            .await,
        Err(PortError::PermissionDenied)
    ));
    policy.inner = Arc::new(InnerPolicy { allow: true });
    store
        .update(
            id(1),
            id(10),
            Uuid::new_v4(),
            update(1, change(11, WorkspaceRole::Viewer, MemberState::Revoked)),
        )
        .await
        .unwrap();
    assert!(matches!(
        policy
            .authorize(principal.clone(), Action::InvokeTool, tool.clone())
            .await,
        Err(PortError::PermissionDenied)
    ));
    store
        .update(
            id(1),
            id(10),
            Uuid::new_v4(),
            update(2, change(11, WorkspaceRole::Operator, MemberState::Active)),
        )
        .await
        .unwrap();
    policy
        .authorize(principal.clone(), Action::InvokeTool, tool.clone())
        .await
        .unwrap();
    store
        .update(
            id(1),
            id(10),
            Uuid::new_v4(),
            update(
                3,
                WorkspaceChange::SetState {
                    state: WorkspaceState::Suspended,
                },
            ),
        )
        .await
        .unwrap();
    assert!(matches!(
        policy.authorize(principal, Action::InvokeTool, tool).await,
        Err(PortError::PermissionDenied)
    ));
}

#[tokio::test]
async fn audits_and_membership_mutations_commit_or_roll_back_together() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("spaces.db");
    let store = SqliteStore::open(&path).await.unwrap();
    store
        .bootstrap(
            space(1),
            id(10),
            vec![member(1, 11, WorkspaceRole::Operator)],
        )
        .await
        .unwrap();
    let sql = rusqlite::Connection::open(&path).unwrap();
    sql.execute_batch("CREATE TRIGGER fail_workspace_audit BEFORE INSERT ON audit BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
    assert!(
        store
            .update(
                id(1),
                id(10),
                Uuid::new_v4(),
                update(1, change(11, WorkspaceRole::Operator, MemberState::Revoked))
            )
            .await
            .is_err()
    );
    assert!(store.access(id(1), id(11)).await.unwrap().active());
    assert_eq!(
        store
            .access(id(1), id(10))
            .await
            .unwrap()
            .workspace
            .revision,
        1
    );
    assert_eq!(
        sql.query_row("SELECT COUNT(*) FROM workspace_changes", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        1
    );
    sql.execute_batch("DROP TRIGGER fail_workspace_audit")
        .unwrap();
    store
        .update(
            id(1),
            id(10),
            Uuid::new_v4(),
            update(1, change(11, WorkspaceRole::Operator, MemberState::Revoked)),
        )
        .await
        .unwrap();
    let record: String = sql
        .query_row(
            "SELECT change_record FROM workspace_changes WHERE revision=2",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(record.contains("revoked"));
    assert!(record.contains(&id(11).to_string()));
}

#[tokio::test]
async fn member_pages_and_owner_checks_use_scoped_indexes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("spaces.db");
    let store = SqliteStore::open(&path).await.unwrap();
    store
        .bootstrap(
            space(1),
            id(10),
            (11..16)
                .map(|p| member(1, p, WorkspaceRole::Operator))
                .collect(),
        )
        .await
        .unwrap();
    store
        .bootstrap(
            space(2),
            id(20),
            vec![member(2, 21, WorkspaceRole::Operator)],
        )
        .await
        .unwrap();
    let page = store.members(id(1), id(10), None, 2).await.unwrap();
    assert_eq!(page.len(), 2);
    let next = store
        .members(id(1), id(10), Some(page[1].principal_id), 2)
        .await
        .unwrap();
    assert!(next[0].principal_id > page[1].principal_id);
    assert!(next.iter().all(|m| m.workspace_id == id(1)));
    assert!(store.members(id(1), id(11), None, 2).await.is_err());
    assert!(store.members(id(1), id(10), None, 101).await.is_err());
    let sql = rusqlite::Connection::open(path).unwrap();
    for query in [
        "SELECT principal,role,state FROM workspace_members WHERE workspace=?1 AND principal>?2 ORDER BY principal LIMIT 50",
        "SELECT 1 FROM workspace_members WHERE workspace=?1 AND principal<>?2 AND role='owner' AND state='active' LIMIT 1",
    ] {
        let plan = sql
            .prepare(&format!("EXPLAIN QUERY PLAN {query}"))
            .unwrap()
            .query_map([id(1).to_string(), id(10).to_string()], |r| {
                r.get::<_, String>(3)
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .join(" ");
        assert!(plan.contains("SEARCH"), "{plan}");
        assert!(!plan.contains("TEMP B-TREE"), "{plan}");
        if query.contains("role='owner'") {
            assert!(plan.contains("workspace_active_owners"), "{plan}");
        }
    }
}

#[tokio::test]
async fn v8_failure_keeps_v7_and_does_not_leave_partial_workspace_tables() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("spaces.db");
    let sql = rusqlite::Connection::open(&path).unwrap();
    for script in [
        include_str!("../src/infrastructure/storage/schema.sql"),
        include_str!("../src/infrastructure/storage/migration_v2.sql"),
        include_str!("../src/infrastructure/storage/migration_v3.sql"),
        include_str!("../src/infrastructure/storage/migration_v4.sql"),
        include_str!("../src/infrastructure/storage/migration_v5.sql"),
        include_str!("../src/infrastructure/storage/migration_v6.sql"),
        include_str!("../src/infrastructure/storage/migration_v7.sql"),
    ] {
        sql.execute_batch(script).unwrap();
    }
    sql.execute_batch("CREATE TABLE workspace_changes(incompatible INTEGER)")
        .unwrap();
    assert!(SqliteStore::open(&path).await.is_err());
    assert_eq!(
        sql.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        7
    );
    assert_eq!(
        sql.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name='workspaces'",
            [],
            |r| r.get::<_, u32>(0)
        )
        .unwrap(),
        0
    );
    sql.execute_batch("DROP TABLE workspace_changes").unwrap();
    SqliteStore::open(&path).await.unwrap();
    assert_eq!(
        sql.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        21
    );
}
