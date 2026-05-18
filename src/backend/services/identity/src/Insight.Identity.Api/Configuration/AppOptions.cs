using Microsoft.Extensions.Configuration;

namespace Insight.Identity.Api.Configuration;

/// <summary>
/// Top-level service options. Mirrors the Rust service's
/// <c>IDENTITY__*</c> snake_case env layout: <c>IDENTITY__bind_addr</c>,
/// <c>IDENTITY__database_url</c>, <c>IDENTITY__tenant_default_id</c>.
/// Configuration providers wired in <c>Program.cs</c> normalize the
/// double-underscore prefix and case; <see cref="ConfigurationKeyNameAttribute"/>
/// bridges the snake_case keys to PascalCase properties because the default
/// binder only does case-insensitive matching, not separator translation.
/// </summary>
public sealed class AppOptions
{
    public const string SectionName = "identity";

    [ConfigurationKeyName("bind_addr")]
    public string BindAddr { get; init; } = "0.0.0.0:8082";

    /// <summary>
    /// Default tenant UUID used when no <c>X-Insight-Tenant-Id</c> header
    /// arrives and JWT auth is not yet wired. Mirrors the Phase 1 flow
    /// the Rust stub used for local development.
    /// </summary>
    [ConfigurationKeyName("tenant_default_id")]
    public Guid? TenantDefaultId { get; init; }

    /// <summary>
    /// Phase 2 toggle. Phase 1 deployments leave this <c>false</c> so the
    /// API returns a single person without recursive subordinate
    /// expansion.
    /// </summary>
    [ConfigurationKeyName("expand_subordinates")]
    public bool ExpandSubordinates { get; init; }

    [ConfigurationKeyName("max_subordinate_depth")]
    public int MaxSubordinateDepth { get; init; } = 5;
}
