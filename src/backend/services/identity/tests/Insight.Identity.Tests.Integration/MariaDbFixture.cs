using MySqlConnector;
using Testcontainers.MariaDb;
using Xunit;

namespace Insight.Identity.Tests.Integration;

/// <summary>
/// Spins up a single MariaDB container per xUnit collection and applies
/// the persons schema. Mirrors the canonical DDL from
/// <c>src/backend/services/identity/src/migration/m20260421_000001_persons.rs</c>.
/// </summary>
public sealed class MariaDbFixture : IAsyncLifetime
{
    private readonly MariaDbContainer _container = new MariaDbBuilder("mariadb:11.4")
        .WithDatabase("identity")
        .WithUsername("insight")
        .WithPassword("insight-pass")
        .Build();

    public string ConnectionString => _container.GetConnectionString();

    public string ConnectionUrl
    {
        get
        {
            var b = new MySqlConnectionStringBuilder(ConnectionString);
            return $"mysql://{Uri.EscapeDataString(b.UserID)}:{Uri.EscapeDataString(b.Password)}@{b.Server}:{b.Port}/{b.Database}";
        }
    }

    public async Task InitializeAsync()
    {
        await _container.StartAsync().ConfigureAwait(false);
        await ApplySchemaAsync().ConfigureAwait(false);
    }

    public Task DisposeAsync() => _container.DisposeAsync().AsTask();

    public async Task ResetAsync()
    {
        await using var conn = new MySqlConnection(ConnectionString);
        await conn.OpenAsync().ConfigureAwait(false);
        await using var cmd = new MySqlCommand("DELETE FROM persons", conn);
        await cmd.ExecuteNonQueryAsync().ConfigureAwait(false);
    }

    private async Task ApplySchemaAsync()
    {
        await using var conn = new MySqlConnection(ConnectionString);
        await conn.OpenAsync().ConfigureAwait(false);
        await using var cmd = new MySqlCommand(PersonsDdl, conn);
        await cmd.ExecuteNonQueryAsync().ConfigureAwait(false);
    }

    private const string PersonsDdl = """
        CREATE TABLE IF NOT EXISTS persons (
            id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
            value_type VARCHAR(50) NOT NULL,
            insight_source_type VARCHAR(100) NOT NULL,
            insight_source_id BINARY(16) NOT NULL,
            insight_tenant_id BINARY(16) NOT NULL,
            value_id VARCHAR(320) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NULL,
            value_full_text VARCHAR(512) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NULL,
            value TEXT NULL,
            value_effective TEXT
                GENERATED ALWAYS AS (COALESCE(value_id, value_full_text, value)) STORED,
            value_hash CHAR(64) CHARACTER SET ascii COLLATE ascii_bin
                GENERATED ALWAYS AS (SHA2(COALESCE(value_id, value_full_text, value), 256)) STORED,
            person_id BINARY(16) NOT NULL,
            author_person_id BINARY(16) NOT NULL,
            reason TEXT NOT NULL DEFAULT '',
            created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
            UNIQUE KEY uq_person_observation (
                insight_tenant_id, person_id, insight_source_type, insight_source_id,
                value_type, value_hash
            ),
            INDEX idx_value_id (insight_tenant_id, value_type, value_id),
            INDEX idx_value_full_text (insight_tenant_id, value_type, value_full_text),
            INDEX idx_person_id (person_id),
            INDEX idx_tenant_person (insight_tenant_id, person_id),
            INDEX idx_source (insight_source_type, insight_source_id)
        ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci
        """;
}

[CollectionDefinition(MariaDbCollection.Name)]
public sealed class MariaDbCollection : ICollectionFixture<MariaDbFixture>
{
    public const string Name = "MariaDB";
}
