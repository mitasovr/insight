namespace Insight.Identity.Domain;

/// <summary>
/// One row from the <c>persons</c> append-only observation log, projected
/// onto the fields this service uses. The full row exposes more columns
/// (insight_source_type, source_id, hashes, …) — only what assembly needs
/// is hydrated.
/// </summary>
/// <param name="PersonId">Stable Insight UUID for the person.</param>
/// <param name="InsightSourceType">Source system identifier, e.g. <c>bamboohr</c>.</param>
/// <param name="InsightSourceId">Connector-instance UUID.</param>
/// <param name="ValueType">One of <see cref="ValueTypes"/>; free-form upstream.</param>
/// <param name="ValueEffective">Coalesced value across the three storage columns.</param>
/// <param name="CreatedAt">Microsecond-precision UTC observation timestamp.</param>
public sealed record PersonObservation(
    Guid PersonId,
    string InsightSourceType,
    Guid InsightSourceId,
    string ValueType,
    string ValueEffective,
    DateTime CreatedAt);
