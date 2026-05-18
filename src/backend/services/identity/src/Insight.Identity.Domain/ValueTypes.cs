namespace Insight.Identity.Domain;

/// <summary>
/// Canonical <c>value_type</c> taxonomy stored in the <c>persons</c> table.
/// </summary>
/// <remarks>
/// The DB column is a free-form <c>VARCHAR(50)</c> and is intentionally
/// extensible — these constants enumerate the subset this service knows how
/// to project into the <c>Person</c> response. Unknown <c>value_type</c>s
/// are read but not surfaced.
/// </remarks>
public static class ValueTypes
{
    public const string Email = "email";
    public const string DisplayName = "display_name";
    public const string FirstName = "first_name";
    public const string LastName = "last_name";
    public const string Department = "department";
    public const string Division = "division";
    public const string JobTitle = "job_title";
    public const string Status = "status";
    public const string EmployeeId = "employee_id";
    public const string Username = "username";

    /// <summary>Source-native supervisor email (BambooHR <c>supervisorEmail</c>).</summary>
    public const string ParentEmail = "parent_email";

    /// <summary>Source-native supervisor identifier (BambooHR <c>supervisorEId</c>).</summary>
    public const string ParentId = "parent_id";

    /// <summary>
    /// Resolved Insight <c>person_id</c> of the supervisor, written by the
    /// reconciliation service. Used as the sole edge for the org tree in
    /// Phase 2 lookups.
    /// </summary>
    public const string ParentPersonId = "parent_person_id";
}
