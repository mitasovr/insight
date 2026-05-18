using Microsoft.AspNetCore.Http;

namespace Insight.Identity.Api.Auth;

/// <summary>
/// Resolves the calling tenant for the current HTTP request. Backed by a
/// composite implementation that checks a header first and falls back to
/// configuration; future JWT-claim-backed tenants slot in by adding an
/// implementation higher up the chain.
/// </summary>
public interface ITenantContext
{
    /// <summary>
    /// Returns the tenant UUID for the current request, or <c>null</c>
    /// when no resolver could provide one (caller must respond 400).
    /// </summary>
    Guid? Resolve(HttpContext context);
}
