//! `SeaORM` entity definitions for `MariaDB` tables.

pub mod metrics {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "metrics")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub insight_tenant_id: Uuid,
        pub name: String,
        pub description: Option<String>,
        pub query_ref: String,
        pub is_enabled: bool,
        pub created_at: ChronoDateTimeUtc,
        pub updated_at: ChronoDateTimeUtc,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod thresholds {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "thresholds")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub insight_tenant_id: Uuid,
        pub metric_id: Uuid,
        pub field_name: String,
        pub operator: String,
        #[sea_orm(column_type = "Decimal(Some((20, 6)))")]
        pub value: f64,
        pub level: String,
        pub created_at: ChronoDateTimeUtc,
        pub updated_at: ChronoDateTimeUtc,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod metric_catalog {
    //! `metric_catalog` entity (Refs #519 schema, #521 validator reads/writes).
    //!
    //! The validator only reads/writes a narrow column set; the rest of the
    //! catalog row exists but is owned by other components (seed-migration for
    //! product columns, admin-crud for read joins). Typed access is exposed for
    //! that narrow set; the no-`updated_at` write path uses raw SQL with bound
    //! parameters from `domain::schema_validator::repository` so we can pin
    //! `updated_at = updated_at` and bypass MariaDB's `ON UPDATE CURRENT_TIMESTAMP`.
    //!
    //! The typed entity is intentionally provided ahead of its first SeaORM
    //! consumer — admin-crud (#525) and catalog-reader (#524) join against
    //! this table; defining the entity here keeps the schema↔code coupling
    //! in one place and lets downstream PRs add columns to the `Model` shape
    //! without re-deciding the table-name binding.
    #![allow(dead_code)]

    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "metric_catalog")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false, column_type = "Binary(16)")]
        pub id: Uuid,
        pub metric_key: String,
        /// One of `ok` / `error` / `unchecked` (DB-side ENUM + CHECK).
        pub schema_status: String,
        pub schema_checked_at: Option<ChronoDateTimeUtc>,
        pub schema_error_code: Option<String>,
        pub updated_at: ChronoDateTimeUtc,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod table_columns {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "table_columns")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub insight_tenant_id: Option<Uuid>,
        pub clickhouse_table: String,
        pub field_name: String,
        pub field_description: Option<String>,
        pub created_at: ChronoDateTimeUtc,
        pub updated_at: ChronoDateTimeUtc,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
