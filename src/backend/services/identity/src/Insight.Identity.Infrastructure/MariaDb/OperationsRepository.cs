using System.Globalization;
using System.Text;
using Insight.Identity.Domain;
using Insight.Identity.Domain.Services;
using MySqlConnector;

namespace Insight.Identity.Infrastructure.MariaDb;

/// <summary>
/// MariaDB-backed <see cref="IOperationsRepository"/>. The lifecycle
/// transitions (<c>TryStart</c>/<c>Complete</c>/<c>Fail</c>) are
/// single-statement updates against the immutable primary key; read
/// paths filter by tenant.
/// </summary>
public sealed class OperationsRepository : IOperationsRepository
{
    private readonly MariaDbConnectionFactory _factory;

    public OperationsRepository(MariaDbConnectionFactory factory)
    {
        _factory = factory;
    }

    public async Task<Guid> EnqueueAsync(
        string operationType,
        Guid tenantId,
        Guid authorPersonId,
        string? requestJson,
        CancellationToken cancellationToken)
    {
        var operationId = Guid.NewGuid();
        await using var conn = await _factory.OpenAsync(cancellationToken).ConfigureAwait(false);
        await using var cmd = new MySqlCommand(SqlOperations.Insert, conn);
        cmd.Parameters.AddWithValue("@operation_id",     operationId.ToByteArray(bigEndian: true));
        cmd.Parameters.AddWithValue("@operation_type",   operationType);
        cmd.Parameters.AddWithValue("@tenant_id",        tenantId.ToByteArray(bigEndian: true));
        cmd.Parameters.AddWithValue("@author_person_id", authorPersonId.ToByteArray(bigEndian: true));
        cmd.Parameters.AddWithValue("@request_json",     requestJson is null ? (object)DBNull.Value : requestJson);
        await cmd.ExecuteNonQueryAsync(cancellationToken).ConfigureAwait(false);
        return operationId;
    }

    public async Task<bool> TryStartAsync(Guid operationId, CancellationToken cancellationToken)
    {
        await using var conn = await _factory.OpenAsync(cancellationToken).ConfigureAwait(false);
        await using var cmd = new MySqlCommand(SqlOperations.TryStart, conn);
        cmd.Parameters.AddWithValue("@operation_id", operationId.ToByteArray(bigEndian: true));
        var rows = await cmd.ExecuteNonQueryAsync(cancellationToken).ConfigureAwait(false);
        return rows == 1;
    }

    public async Task CompleteAsync(Guid operationId, string summaryJson, CancellationToken cancellationToken)
    {
        ArgumentNullException.ThrowIfNull(summaryJson);
        await using var conn = await _factory.OpenAsync(cancellationToken).ConfigureAwait(false);
        await using var cmd = new MySqlCommand(SqlOperations.Complete, conn);
        cmd.Parameters.AddWithValue("@operation_id", operationId.ToByteArray(bigEndian: true));
        cmd.Parameters.AddWithValue("@summary_json", summaryJson);
        await cmd.ExecuteNonQueryAsync(cancellationToken).ConfigureAwait(false);
    }

    public async Task FailAsync(Guid operationId, string errorMessage, CancellationToken cancellationToken)
    {
        ArgumentNullException.ThrowIfNull(errorMessage);
        await using var conn = await _factory.OpenAsync(cancellationToken).ConfigureAwait(false);
        await using var cmd = new MySqlCommand(SqlOperations.Fail, conn);
        cmd.Parameters.AddWithValue("@operation_id",  operationId.ToByteArray(bigEndian: true));
        cmd.Parameters.AddWithValue("@error_message", errorMessage);
        await cmd.ExecuteNonQueryAsync(cancellationToken).ConfigureAwait(false);
    }

    public async Task<Operation?> GetByIdAsync(Guid tenantId, Guid operationId, CancellationToken cancellationToken)
    {
        await using var conn = await _factory.OpenAsync(cancellationToken).ConfigureAwait(false);
        await using var cmd = new MySqlCommand(SqlOperations.GetById, conn);
        cmd.Parameters.AddWithValue("@tenant_id",    tenantId.ToByteArray(bigEndian: true));
        cmd.Parameters.AddWithValue("@operation_id", operationId.ToByteArray(bigEndian: true));
        await using var reader = await cmd.ExecuteReaderAsync(cancellationToken).ConfigureAwait(false);
        if (!await reader.ReadAsync(cancellationToken).ConfigureAwait(false))
        {
            return null;
        }
        return Read(reader);
    }

    public async Task<PagedResult<Operation>> ListAsync(
        Guid tenantId,
        string? operationType,
        OperationStatus? status,
        PageRequest page,
        CancellationToken cancellationToken)
    {
        ArgumentNullException.ThrowIfNull(page);
        var clamped = page.WithClampedLimit();

        var sb = new StringBuilder(SqlOperations.ListBase);
        if (operationType is not null) sb.Append(" AND operation_type = @operation_type");
        if (status is not null)        sb.Append(" AND status = @status");
        sb.Append(" ORDER BY started_at DESC, operation_id DESC LIMIT @limit");

        await using var conn = await _factory.OpenAsync(cancellationToken).ConfigureAwait(false);
        await using var cmd = new MySqlCommand(sb.ToString(), conn);
        cmd.Parameters.AddWithValue("@tenant_id", tenantId.ToByteArray(bigEndian: true));
        if (operationType is not null) cmd.Parameters.AddWithValue("@operation_type", operationType);
        if (status is { } s)           cmd.Parameters.AddWithValue("@status", StatusToString(s));
        cmd.Parameters.AddWithValue("@limit", clamped.Limit);

        await using var reader = await cmd.ExecuteReaderAsync(cancellationToken).ConfigureAwait(false);
        var items = new List<Operation>();
        while (await reader.ReadAsync(cancellationToken).ConfigureAwait(false))
        {
            items.Add(Read(reader));
        }
        // Cursor pagination is a future bookmark — for now next-cursor is always null.
        return new PagedResult<Operation>(items, NextCursor: null);
    }

    public async Task<int> SweepZombiesAsync(DateTime olderThan, CancellationToken cancellationToken)
    {
        await using var conn = await _factory.OpenAsync(cancellationToken).ConfigureAwait(false);
        await using var cmd = new MySqlCommand(SqlOperations.SweepZombies, conn);
        cmd.Parameters.AddWithValue(
            "@older_than",
            olderThan.ToUniversalTime().ToString("yyyy-MM-dd HH:mm:ss.ffffff", CultureInfo.InvariantCulture));
        return await cmd.ExecuteNonQueryAsync(cancellationToken).ConfigureAwait(false);
    }

    private static Operation Read(MySqlDataReader reader)
    {
        return new Operation(
            OperationId:     new Guid((byte[])reader["operation_id"], bigEndian: true),
            OperationType:   reader.GetString("operation_type"),
            Status:          ParseStatus(reader.GetString("status")),
            InsightTenantId: new Guid((byte[])reader["insight_tenant_id"], bigEndian: true),
            AuthorPersonId:  new Guid((byte[])reader["author_person_id"], bigEndian: true),
            RequestJson:     reader["request_json"]  is DBNull ? null : reader.GetString("request_json"),
            SummaryJson:     reader["summary_json"]  is DBNull ? null : reader.GetString("summary_json"),
            ErrorMessage:    reader["error_message"] is DBNull ? null : reader.GetString("error_message"),
            StartedAt:       reader.GetDateTime("started_at"),
            CompletedAt:     reader["completed_at"] is DBNull ? null : reader.GetDateTime("completed_at"));
    }

    private static string StatusToString(OperationStatus status) => status switch
    {
        OperationStatus.Queued    => "queued",
        OperationStatus.Running   => "running",
        OperationStatus.Completed => "completed",
        OperationStatus.Failed    => "failed",
        _ => throw new ArgumentOutOfRangeException(nameof(status), status, "unknown operation status"),
    };

    private static OperationStatus ParseStatus(string raw) => raw switch
    {
        "queued"    => OperationStatus.Queued,
        "running"   => OperationStatus.Running,
        "completed" => OperationStatus.Completed,
        "failed"    => OperationStatus.Failed,
        _ => throw new InvalidOperationException($"unknown operations.status '{raw}'"),
    };
}
