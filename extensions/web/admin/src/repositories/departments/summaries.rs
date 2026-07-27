//! Read-only rollups for the department dashboard: per-department usage
//! summaries, member lists, and top-tool breakdowns over the last 30 days.

use sqlx::PgPool;

use crate::types::departments::DepartmentSummary;

pub async fn list_departments(pool: &PgPool) -> Result<Vec<DepartmentSummary>, sqlx::Error> {
    sqlx::query_as!(
        DepartmentSummary,
        r#"
        SELECT
            d.id,
            d.name,
            d.description as "description!",
            COALESCE(mc.member_count, 0)::BIGINT  AS "member_count!",
            COALESCE(ac.assignment_count, 0)::BIGINT AS "assignment_count!",
            COALESCE(usg.input_tokens, 0)::BIGINT  AS "input_tokens!",
            COALESCE(usg.output_tokens, 0)::BIGINT AS "output_tokens!",
            COALESCE(usg.requests, 0)::BIGINT      AS "requests!",
            COALESCE(usg.cost_microdollars, 0)::BIGINT AS "cost_microdollars!",
            d.created_at,
            d.updated_at
        FROM departments d
        LEFT JOIN (
            SELECT department, COUNT(*)::BIGINT AS member_count
            FROM user_profile_ext
            WHERE department IS NOT NULL AND department <> ''
            GROUP BY department
        ) mc ON mc.department = d.name
        LEFT JOIN (
            SELECT rule_value, COUNT(*)::BIGINT AS assignment_count
            FROM access_control_rules
            WHERE rule_type = 'department'
            GROUP BY rule_value
        ) ac ON ac.rule_value = d.name
        LEFT JOIN (
            SELECT
                upe.department AS dept,
                COALESCE(SUM(ar.input_tokens), 0)::BIGINT  AS input_tokens,
                COALESCE(SUM(ar.output_tokens), 0)::BIGINT AS output_tokens,
                COUNT(ar.id)::BIGINT                       AS requests,
                COALESCE(SUM(ar.cost_microdollars), 0)::BIGINT AS cost_microdollars
            FROM ai_requests ar
            JOIN user_profile_ext upe ON upe.user_id = ar.user_id
            WHERE ar.created_at >= NOW() - INTERVAL '30 days'
            GROUP BY upe.department
        ) usg ON usg.dept = d.name
        ORDER BY d.name
        "#,
    )
    .fetch_all(pool)
    .await
}

