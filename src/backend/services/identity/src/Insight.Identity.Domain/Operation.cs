namespace Insight.Identity.Domain;

/// <summary>
/// One row of the <c>operations</c> audit table — a record of an
/// admin-triggered async operation (e.g. a <c>persons-seed</c> run).
/// <see cref="Status"/> moves <c>queued</c> → <c>running</c> →
/// <c>completed</c>/<c>failed</c>. <see cref="RequestJson"/> stores
/// the body the caller posted (for replay / forensics);
/// <see cref="SummaryJson"/> stores the per-operation result shape
/// (counters / ids / etc) on success; <see cref="ErrorMessage"/>
/// stores the failure reason on failure. Both JSON columns are
/// opaque to the Domain layer — callers serialise / deserialise
/// against their own per-operation contract.
/// </summary>
public sealed record Operation(
    Guid OperationId,
    string OperationType,
    OperationStatus Status,
    Guid InsightTenantId,
    Guid AuthorPersonId,
    string? RequestJson,
    string? SummaryJson,
    string? ErrorMessage,
    DateTime StartedAt,
    DateTime? CompletedAt);

/// <summary>Lifecycle phase of an <see cref="Operation"/>.</summary>
public enum OperationStatus
{
    /// <summary>Row was inserted by the POST handler; worker has not picked it up yet.</summary>
    Queued,
    /// <summary>Worker is executing the operation right now.</summary>
    Running,
    /// <summary>Operation finished successfully; <c>summary_json</c> populated, <c>completed_at</c> set.</summary>
    Completed,
    /// <summary>Operation threw or was aborted; <c>error_message</c> populated, <c>completed_at</c> set.</summary>
    Failed,
}

/// <summary>
/// Canonical operation-type strings (the value of the
/// <c>operation_type</c> column). New entries here are the only
/// place an operation type is named — keep DB writes and route
/// handlers reading from these constants.
/// </summary>
public static class OperationTypes
{
    /// <summary>Bulk re-seed of MariaDB persons/account_person_map/org_chart from ClickHouse identity_inputs.</summary>
    public const string PersonsSeed = "persons-seed";
}
