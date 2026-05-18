namespace Insight.Identity.Domain.Services;

/// <summary>
/// Repository abstraction the lookup service depends on. The infrastructure
/// project supplies a MariaDB-backed implementation; tests can stub the
/// interface directly.
/// </summary>
public interface IPersonsReader
{
    /// <summary>
    /// Resolve a single <c>person_id</c> from a lookup email. Returns
    /// <c>null</c> when no current observation in the tenant has
    /// <c>value_type='email'</c> = <paramref name="emailLowercase"/>.
    /// </summary>
    Task<Guid?> ResolvePersonIdByEmailAsync(
        Guid tenantId,
        string emailLowercase,
        CancellationToken cancellationToken);

    /// <summary>
    /// Latest-per-source observations for a single <c>person_id</c> within
    /// the tenant. Empty list when the person has no observations.
    /// </summary>
    Task<IReadOnlyList<PersonObservation>> GetLatestObservationsAsync(
        Guid tenantId,
        Guid personId,
        CancellationToken cancellationToken);

    /// <summary>
    /// Direct subordinates: <c>person_id</c>s whose latest
    /// <c>parent_person_id</c> observation across sources equals
    /// <paramref name="parentPersonId"/>. Reserved for Phase 2; Phase 1
    /// callers ignore the result.
    /// </summary>
    Task<IReadOnlyList<Guid>> GetDirectSubordinateIdsAsync(
        Guid tenantId,
        Guid parentPersonId,
        CancellationToken cancellationToken);
}
