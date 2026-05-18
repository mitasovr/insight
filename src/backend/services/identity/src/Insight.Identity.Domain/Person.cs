namespace Insight.Identity.Domain;

/// <summary>
/// Person projection returned by the API. Field shape matches the legacy
/// Rust stub for drop-in replacement; <see cref="PersonId"/> is added so
/// downstream services can pivot off the resolved Insight UUID.
/// </summary>
public sealed record Person(
    Guid PersonId,
    string Email,
    string DisplayName,
    string FirstName,
    string LastName,
    string Department,
    string Division,
    string JobTitle,
    string Status,
    string? ParentEmail,
    string? ParentId,
    Guid? ParentPersonId,
    IReadOnlyList<Person> Subordinates);
