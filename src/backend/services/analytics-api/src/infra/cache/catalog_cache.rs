//! Catalog cache layer (`cpt-metric-cat-component-cache-layer`).
//!
//! v1 ships a no-op stub. The real Redis-backed implementation lands with #524
//! (catalog-reader + threshold-resolver + cache-layer). The trait + no-op live
//! here in this PR so the seed migration's `cache_layer.flush_all()` step from
//! the DESIGN §3.6 seed-migration sequence has a real call site to invoke
//! today — the wiring activates the moment a non-stub `CatalogCache` impl is
//! plugged in.
//!
//! The flush semantics are **prefix purge of [`CACHE_KEY_PREFIX`]**
//! (`cat:v1:*`), explicitly NOT `FLUSHDB`. The Redis instance is shared with
//! Identity Resolution's `person_aliases:*` namespace and any other domain
//! that grows onto it; a `FLUSHDB` would clobber sibling tenants. See
//! DESIGN §3.6 "Seed Migration — New Metric Lands" and PRD §11.

use async_trait::async_trait;

/// Canonical Redis-key prefix for the catalog cache. Every catalog entry's
/// key is `cat:v1:{tenant_id}:{role_slug_or_empty}:{team_id_or_empty}` per
/// the #524 brief; this constant is the prefix the seed-migration flush and
/// admin-write invalidations both walk.
pub const CACHE_KEY_PREFIX: &str = "cat:v1:";

/// Catalog cache trait — abstracts the Redis (or future pub-sub) backend so
/// the seed and admin paths can call into it without depending on a concrete
/// client. #524 lands the real implementation behind this trait.
#[async_trait]
pub trait CatalogCache: Send + Sync {
    /// Purge every key under [`CACHE_KEY_PREFIX`].
    ///
    /// The seed migration calls this after every successful boot so newly
    /// seeded rows are visible on the next `POST /catalog/get_metrics` read
    /// without waiting for the per-key TTL.
    ///
    /// # Errors
    ///
    /// Implementation-defined. The no-op stub never errors; a Redis-backed
    /// impl surfaces connection / SCAN failures here.
    async fn flush_all(&self) -> anyhow::Result<()>;
}

/// No-op implementation. v1 default until #524 lands the Redis-backed one.
///
/// `flush_all` logs that it ran and returns `Ok(())` — so the call site in
/// `main.rs` exercises the same control-flow shape that the Redis impl will
/// (the seed-migration sequence diagram in DESIGN §3.6 wants `ack` back from
/// the cache layer; a panicking stub would mask wiring drift).
pub struct NoopCatalogCache;

#[async_trait]
impl CatalogCache for NoopCatalogCache {
    async fn flush_all(&self) -> anyhow::Result<()> {
        tracing::info!(
            prefix = CACHE_KEY_PREFIX,
            "catalog_cache: flush_all called on no-op stub \
             (real `{prefix}*` purge activates once #524 ships the Redis impl)",
            prefix = CACHE_KEY_PREFIX,
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_is_cat_v1_colon() {
        // Pinned shape because cross-component coordination (seed flush,
        // admin invalidate, #524 resolver) all derive their keys from this
        // constant. A typo here silently desyncs the catalog cache from the
        // rest of the catalog domain.
        assert_eq!(CACHE_KEY_PREFIX, "cat:v1:");
    }

    #[tokio::test]
    async fn noop_flush_is_ok() {
        let cache = NoopCatalogCache;
        // `expect_used` is denied workspace-wide — unwrap_or_else with a
        // panic message matches the repo convention (see
        // `domain::schema_validator::parse::tests::parse_or_panic`).
        cache
            .flush_all()
            .await
            .unwrap_or_else(|e| panic!("no-op must never error: {e}"));
    }
}
