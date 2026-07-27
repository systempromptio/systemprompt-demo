//! `record_mcp_access` writes an attributed `mcp_access` audit row, shaping
//! `entity_type`/`entity_name` by action and stamping the calling session.

use systemprompt::identifiers::{SessionId, UserId};
use systemprompt_mcp_shared::{McpAccess, record_mcp_access};

use crate::common::TempDb;

#[tokio::test]
async fn used_action_records_tool_scoped_row() {
    let Some(db) = TempDb::create().await else {
        eprintln!("skipping: no test database configured");
        return;
    };
    db.insert_user("user-1", "dev@example.com").await;

    record_mcp_access(
        &db.pool,
        &McpAccess {
            user_id: &UserId::new("user-1"),
            session_id: &SessionId::new("sess-a"),
            server: "systemprompt",
            tool: "list_skills",
            action: "used",
        },
    )
    .await;

    let rows = db.mcp_rows("list_skills").await;
    assert_eq!(rows.len(), 1, "exactly one activity row expected");
    let (user_id, action, entity_type, description, session_id) = &rows[0];
    assert_eq!(user_id, "user-1");
    assert_eq!(action, "used");
    assert_eq!(entity_type.as_deref(), Some("tool"));
    assert_eq!(description, "Executed 'list_skills' on systemprompt");
    assert_eq!(
        session_id.as_deref(),
        Some("sess-a"),
        "the session must be stamped, or per-session tool-fire queries find nothing"
    );

    db.cleanup().await;
}

#[tokio::test]
async fn authenticated_action_records_server_scoped_row() {
    let Some(db) = TempDb::create().await else {
        eprintln!("skipping: no test database configured");
        return;
    };
    db.insert_user("user-2", "dev2@example.com").await;

    record_mcp_access(
        &db.pool,
        &McpAccess {
            user_id: &UserId::new("user-2"),
            session_id: &SessionId::new("sess-b"),
            server: "systemprompt",
            tool: "list_skills",
            action: "authenticated",
        },
    )
    .await;

    // For non-"used" actions the row is attributed to the server, not the tool.
    let rows = db.mcp_rows("systemprompt").await;
    assert_eq!(rows.len(), 1);
    let (user_id, action, entity_type, description, session_id) = &rows[0];
    assert_eq!(user_id, "user-2");
    assert_eq!(action, "authenticated");
    assert_eq!(entity_type.as_deref(), Some("mcp_server"));
    assert_eq!(
        description,
        "Authenticated to systemprompt for 'list_skills'"
    );
    assert_eq!(session_id.as_deref(), Some("sess-b"));

    db.cleanup().await;
}

/// One user, two sessions: the rows must partition by session.
///
/// This is the regression behind `governance_stats` reporting a lifetime total
/// where it claimed to report one session. Scoping by `user_id` alone passes
/// every other test in this file and still gets this wrong.
#[tokio::test]
async fn rows_partition_by_session_for_one_user() {
    let Some(db) = TempDb::create().await else {
        eprintln!("skipping: no test database configured");
        return;
    };
    db.insert_user("user-3", "dev3@example.com").await;

    for session in ["sess-one", "sess-one", "sess-two"] {
        record_mcp_access(
            &db.pool,
            &McpAccess {
                user_id: &UserId::new("user-3"),
                session_id: &SessionId::new(session),
                server: "systemprompt",
                tool: "governance_stats",
                action: "used",
            },
        )
        .await;
    }

    let rows = db.mcp_rows("governance_stats").await;
    assert_eq!(rows.len(), 3, "all three rows belong to the same user");

    let in_first = rows
        .iter()
        .filter(|(_, _, _, _, s)| s.as_deref() == Some("sess-one"))
        .count();
    let in_second = rows
        .iter()
        .filter(|(_, _, _, _, s)| s.as_deref() == Some("sess-two"))
        .count();
    assert_eq!(in_first, 2);
    assert_eq!(in_second, 1);

    db.cleanup().await;
}
