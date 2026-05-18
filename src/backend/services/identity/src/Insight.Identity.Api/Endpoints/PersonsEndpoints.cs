using Insight.Identity.Api.Auth;
using Insight.Identity.Api.Configuration;
using Insight.Identity.Api.Contracts;
using Insight.Identity.Domain.Services;
using Insight.Identity.Infrastructure.MariaDb;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Http;
using Microsoft.AspNetCore.Routing;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Options;

namespace Insight.Identity.Api.Endpoints;

public static class PersonsEndpoints
{
    public static IEndpointRouteBuilder MapPersonsEndpoints(this IEndpointRouteBuilder app)
    {
        ArgumentNullException.ThrowIfNull(app);

        app.MapGet("/v1/persons/{email}", async (
            string email,
            HttpContext http,
            ITenantContext tenants,
            PersonLookupService lookup,
            IOptions<AppOptions> options,
            CancellationToken cancellationToken) =>
        {
            var tenantId = tenants.Resolve(http);
            if (tenantId is null)
            {
                return Results.Json(new ProblemResponse(
                    Type: "urn:insight:error:tenant_unresolved",
                    Title: "Bad Request",
                    Status: StatusCodes.Status400BadRequest,
                    Detail: $"Tenant not provided. Send the {HeaderTenantContext.HeaderName} header or configure identity.tenant_default_id."),
                    statusCode: StatusCodes.Status400BadRequest);
            }

            var lookupOptions = options.Value.ExpandSubordinates
                ? new LookupOptions(ExpandSubordinates: true, MaxDepth: options.Value.MaxSubordinateDepth)
                : LookupOptions.Phase1;

            var person = await lookup.GetByEmailAsync(tenantId.Value, email, lookupOptions, cancellationToken)
                .ConfigureAwait(false);
            if (person is null)
            {
                return Results.Json(new ProblemResponse(
                    Type: "urn:insight:error:person_not_found",
                    Title: "Not Found",
                    Status: StatusCodes.Status404NotFound,
                    Detail: $"person with email '{email}' not found"),
                    statusCode: StatusCodes.Status404NotFound);
            }

            return Results.Ok(PersonResponse.From(person));
        });

        app.MapGet("/health", async (PersonsRepository repo, CancellationToken cancellationToken) =>
        {
            var ok = await repo.PingAsync(cancellationToken).ConfigureAwait(false);
            return ok
                ? Results.Ok(new { status = "healthy" })
                : Results.Json(new { status = "unhealthy" }, statusCode: StatusCodes.Status503ServiceUnavailable);
        });

        app.MapGet("/healthz", () => Results.Text("ok", "text/plain"));

        return app;
    }
}
