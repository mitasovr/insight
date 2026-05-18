using Microsoft.AspNetCore.Http;

namespace Insight.Identity.Api.Auth;

/// <summary>
/// Walks the resolver chain (header → JWT stub → config default) and
/// returns the first non-null result. The order is what makes header
/// callers always win over the configured default — needed so a single
/// helm release can serve multiple tenants by per-request header.
/// </summary>
public sealed class CompositeTenantContext : ITenantContext
{
    private readonly IReadOnlyList<ITenantContext> _resolvers;

    public CompositeTenantContext(IEnumerable<ITenantContext> resolvers)
    {
        _resolvers = resolvers?.ToArray() ?? throw new ArgumentNullException(nameof(resolvers));
    }

    public Guid? Resolve(HttpContext context)
    {
        foreach (var resolver in _resolvers)
        {
            var tenantId = resolver.Resolve(context);
            if (tenantId is not null)
            {
                return tenantId;
            }
        }
        return null;
    }
}
