using Microsoft.AspNetCore.Http;

namespace Insight.Identity.Api.Auth;

/// <summary>
/// Phase 1.5 stub. Once the api-gateway issues the cookie/JWT pair the
/// service will receive a forwarded principal bearing an
/// <c>insight_tenant_id</c> claim; this resolver hooks that path in
/// without rewiring the composite chain.
/// </summary>
public sealed class JwtTenantContext : ITenantContext
{
    public Guid? Resolve(HttpContext context)
    {
        ArgumentNullException.ThrowIfNull(context);
        var raw = context.User.FindFirst("insight_tenant_id")?.Value;
        return Guid.TryParse(raw, out var tenantId) ? tenantId : null;
    }
}
