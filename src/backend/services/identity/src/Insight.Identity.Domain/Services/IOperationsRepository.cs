namespace Insight.Identity.Domain.Services;

/// <summary>
/// Persistence port over the <c>operations</c> audit table. Used by
/// the POST <c>/v1/persons-seed</c> handler to enqueue a row, the
/// background worker to update status + summary, the GET handlers to
/// surface state, and the startup zombie-cleanup hook.
/// </summary>
public interface IOperationsRepository
{
    /// <summary>
    /// Insert a fresh operation row with <see cref="OperationStatus.Queued"/>
    /// status. <paramref name="requestJson"/> stores the caller-posted
    /// body verbatim. Returns the minted <c>operation_id</c>.
    /// </summary>
    Task<Guid> EnqueueAsync(
        string operationType,
        Guid tenantId,
        Guid authorPersonId,
        string? requestJson,
        CancellationToken cancellationToken);

    /// <summary>
    /// Flip status to <see cref="OperationStatus.Running"/>. No-op
    /// (returns <c>false</c>) when the row is no longer <c>queued</c>
    /// — protects against a double-pickup by two worker instances.
    /// </summary>
    Task<bool> TryStartAsync(Guid operationId, CancellationToken cancellationToken);

    /// <summary>
    /// Flip status to <see cref="OperationStatus.Completed"/>, store
    /// <paramref name="summaryJson"/>, stamp <c>completed_at</c>.
    /// </summary>
    Task CompleteAsync(Guid operationId, string summaryJson, CancellationToken cancellationToken);

    /// <summary>
    /// Flip status to <see cref="OperationStatus.Failed"/>, store
    /// <paramref name="errorMessage"/>, stamp <c>completed_at</c>.
    /// </summary>
    Task FailAsync(Guid operationId, string errorMessage, CancellationToken cancellationToken);

    /// <summary>
    /// One operation by id within the tenant, or <c>null</c>. Tenant
    /// scoping is part of the predicate so a caller in tenant A
    /// cannot read a row from tenant B by id.
    /// </summary>
    Task<Operation?> GetByIdAsync(Guid tenantId, Guid operationId, CancellationToken cancellationToken);

    /// <summary>
    /// Paged list of operations in a tenant, newest first. Filters on
    /// <paramref name="operationType"/> / <paramref name="status"/>
    /// are optional; pass <c>null</c> to leave a filter off.
    /// </summary>
    Task<PagedResult<Operation>> ListAsync(
        Guid tenantId,
        string? operationType,
        OperationStatus? status,
        PageRequest page,
        CancellationToken cancellationToken);

    /// <summary>
    /// On service startup, flip every operation that was left in
    /// <c>queued</c> or <c>running</c> from a prior process to
    /// <see cref="OperationStatus.Failed"/> with an "aborted by pod
    /// restart" error message. The cutoff <paramref name="olderThan"/>
    /// guards against racing the current pod's own freshly-queued
    /// rows. Returns the count of rows flipped.
    /// </summary>
    Task<int> SweepZombiesAsync(DateTime olderThan, CancellationToken cancellationToken);
}
