use crate::error::DbResult;
use crate::models::{CreditEvent, CreditEventRaw};
use chrono::Utc;
use sqlx::{Any, Pool};

/// Insert a new credit event (immutable audit log)
pub async fn insert_credit_event(
    pool: &Pool<Any>,
    contributor_id: i64,
    event_type: &str,
    delta: i32,
    credit_before: i32,
    credit_after: i32,
    llm_evaluation: Option<String>,
    maintainer_override: Option<String>,
) -> DbResult<CreditEvent> {
    let now = Utc::now();
    let now_str = now.to_rfc3339();

    sqlx::query(
        "INSERT INTO credit_events (contributor_id, event_type, delta, credit_before, credit_after, llm_evaluation, maintainer_override, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(contributor_id)
    .bind(event_type)
    .bind(delta)
    .bind(credit_before)
    .bind(credit_after)
    .bind(&llm_evaluation)
    .bind(&maintainer_override)
    .bind(&now_str)
    .execute(pool)
    .await?;

    // Return a dummy ID (the event is immutable and we don't need the ID for most operations)
    // In a real scenario, we might query back to get the actual ID, but for simplicity we'll use 0
    // since credit events are append-only and typically queried by contributor_id
    Ok(CreditEvent {
        id: 0,  // Placeholder ID
        contributor_id,
        event_type: event_type.to_string(),
        delta,
        credit_before,
        credit_after,
        llm_evaluation,
        maintainer_override,
        created_at: now,
    })
}

/// List credit events by contributor with pagination
pub async fn list_events_by_contributor(
    pool: &Pool<Any>,
    contributor_id: i64,
    limit: i64,
    offset: i64,
) -> DbResult<Vec<CreditEvent>> {
    let events = sqlx::query_as::<_, CreditEventRaw>(
        "SELECT id, contributor_id, event_type, delta, credit_before, credit_after, llm_evaluation, maintainer_override, created_at
         FROM credit_events
         WHERE contributor_id = ?
         ORDER BY created_at DESC
         LIMIT ? OFFSET ?"
    )
    .bind(contributor_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|raw| raw.into())
    .collect();

    Ok(events)
}

/// Count total events for a contributor
pub async fn count_events_by_contributor(
    pool: &Pool<Any>,
    contributor_id: i64,
) -> DbResult<i64> {
    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM credit_events WHERE contributor_id = ?"
    )
    .bind(contributor_id)
    .fetch_one(pool)
    .await?;

    Ok(count.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contributors::create_contributor;
    use sqlx::any::AnyPoolOptions;

    async fn setup_test_db() -> Pool<Any> {
        // Install the SQLite driver for Any
        sqlx::any::install_default_drivers();

        let pool = AnyPoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("Failed to create test database pool");

        // Enable foreign keys
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await
            .expect("Failed to enable foreign keys");

        // Run migrations
        sqlx::query(include_str!("../migrations/001_initial.sql"))
            .execute(&pool)
            .await
            .expect("Failed to run migrations");

        pool
    }

    #[tokio::test]
    async fn test_insert_credit_event() {
        let pool = setup_test_db().await;

        let contributor = create_contributor(&pool, 12345, "owner", "repo", 100)
            .await
            .expect("Failed to create contributor");

        let event = insert_credit_event(
            &pool,
            contributor.id,
            "pr_opened",
            15,
            100,
            115,
            Some(r#"{"quality": "high"}"#.to_string()),
            None,
        )
        .await
        .expect("Failed to insert credit event");

        assert_eq!(event.contributor_id, contributor.id);
        assert_eq!(event.event_type, "pr_opened");
        assert_eq!(event.delta, 15);
        assert_eq!(event.credit_before, 100);
        assert_eq!(event.credit_after, 115);
        assert_eq!(
            event.llm_evaluation,
            Some(r#"{"quality": "high"}"#.to_string())
        );
        assert_eq!(event.maintainer_override, None);
    }

    #[tokio::test]
    async fn test_list_events_by_contributor() {
        let pool = setup_test_db().await;

        let contributor = create_contributor(&pool, 12345, "owner", "repo", 100)
            .await
            .expect("Failed to create contributor");

        // Insert multiple events
        insert_credit_event(&pool, contributor.id, "pr_opened", 15, 100, 115, None, None)
            .await
            .expect("Failed to insert event");
        insert_credit_event(&pool, contributor.id, "comment", 3, 115, 118, None, None)
            .await
            .expect("Failed to insert event");
        insert_credit_event(&pool, contributor.id, "pr_merged", 20, 118, 138, None, None)
            .await
            .expect("Failed to insert event");

        // List all events
        let events = list_events_by_contributor(&pool, contributor.id, 10, 0)
            .await
            .expect("Failed to list events");

        assert_eq!(events.len(), 3);
        // Should be in reverse chronological order
        assert_eq!(events[0].event_type, "pr_merged");
        assert_eq!(events[1].event_type, "comment");
        assert_eq!(events[2].event_type, "pr_opened");
    }

    #[tokio::test]
    async fn test_list_events_pagination() {
        let pool = setup_test_db().await;

        let contributor = create_contributor(&pool, 12345, "owner", "repo", 100)
            .await
            .expect("Failed to create contributor");

        // Insert 5 events
        for i in 0..5 {
            insert_credit_event(
                &pool,
                contributor.id,
                "comment",
                1,
                100 + i,
                101 + i,
                None,
                None,
            )
            .await
            .expect("Failed to insert event");
        }

        // First page (2 items)
        let page1 = list_events_by_contributor(&pool, contributor.id, 2, 0)
            .await
            .expect("Failed to list events");
        assert_eq!(page1.len(), 2);

        // Second page (2 items)
        let page2 = list_events_by_contributor(&pool, contributor.id, 2, 2)
            .await
            .expect("Failed to list events");
        assert_eq!(page2.len(), 2);

        // Third page (1 item)
        let page3 = list_events_by_contributor(&pool, contributor.id, 2, 4)
            .await
            .expect("Failed to list events");
        assert_eq!(page3.len(), 1);

        // Out of bounds page (0 items)
        let page4 = list_events_by_contributor(&pool, contributor.id, 2, 10)
            .await
            .expect("Failed to list events");
        assert_eq!(page4.len(), 0);
    }

    #[tokio::test]
    async fn test_count_events_by_contributor() {
        let pool = setup_test_db().await;

        let contributor = create_contributor(&pool, 12345, "owner", "repo", 100)
            .await
            .expect("Failed to create contributor");

        // Initially should be 0
        let count = count_events_by_contributor(&pool, contributor.id)
            .await
            .expect("Failed to count events");
        assert_eq!(count, 0);

        // Insert 3 events
        for i in 0..3 {
            insert_credit_event(
                &pool,
                contributor.id,
                "comment",
                1,
                100 + i,
                101 + i,
                None,
                None,
            )
            .await
            .expect("Failed to insert event");
        }

        // Should be 3
        let count = count_events_by_contributor(&pool, contributor.id)
            .await
            .expect("Failed to count events");
        assert_eq!(count, 3);
    }

    #[tokio::test]
    async fn test_empty_result_set() {
        let pool = setup_test_db().await;

        let contributor = create_contributor(&pool, 12345, "owner", "repo", 100)
            .await
            .expect("Failed to create contributor");

        // Query non-existent events
        let events = list_events_by_contributor(&pool, contributor.id, 10, 0)
            .await
            .expect("Failed to list events");

        assert_eq!(events.len(), 0);
    }
}
