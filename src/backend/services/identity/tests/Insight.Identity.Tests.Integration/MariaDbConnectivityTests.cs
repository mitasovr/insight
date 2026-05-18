using System.Data;
using FluentAssertions;
using Insight.Identity.Api;
using Insight.Identity.Infrastructure.MariaDb;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Options;
using Xunit;

namespace Insight.Identity.Tests.Integration;

/// <summary>
/// Connectivity isolation tests. They walk the same code path the failing
/// endpoint tests exercise, but stop at successive intermediate points so
/// we can see exactly which boundary breaks the connection.
/// </summary>
[Collection(MariaDbCollection.Name)]
public sealed class MariaDbConnectivityTests
{
    private static readonly Guid TenantId = Guid.Parse("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa");

    private readonly MariaDbFixture _fixture;

    public MariaDbConnectivityTests(MariaDbFixture fixture) => _fixture = fixture;

    /// <summary>
    /// Reference: bare driver against the fixture's ConnectionString. If
    /// this fails the container or driver is broken — but seed already
    /// proves it works, so this should be green.
    /// </summary>
    [Fact]
    public async Task Reference_bare_mysqlconnection_opens()
    {
        await using var conn = new MySqlConnector.MySqlConnection(_fixture.ConnectionString);
        await conn.OpenAsync();
        conn.State.Should().Be(ConnectionState.Open);
    }

    /// <summary>
    /// Layer 1: MariaDbConnectionFactory wrapping the same connection
    /// string outside any DI/host. If this fails, the overlay
    /// (Pooling/MinPoolSize/timeouts/CharacterSet) breaks something.
    /// </summary>
    [Fact]
    public async Task Layer1_factory_with_options_opens_connection()
    {
        var options = Options.Create(new MariaDbOptions
        {
            ConnectionString = _fixture.ConnectionString,
        });
        var factory = new MariaDbConnectionFactory(options);
        await using var conn = await factory.OpenAsync(CancellationToken.None);
        conn.State.Should().Be(ConnectionState.Open);
    }

    /// <summary>
    /// Layer 2: factory resolved from the WebApplicationFactory's DI.
    /// If this fails, options binding inside the test host is broken.
    /// </summary>
    [Fact]
    public async Task Layer2_factory_from_app_di_opens_connection()
    {
        using var app = new TestApplicationFactory(_fixture.ConnectionString, TenantId);
        // Build the host without sending a request.
        _ = app.Services;
        using var scope = app.Services.CreateScope();
        var factory = scope.ServiceProvider.GetRequiredService<MariaDbConnectionFactory>();

        // Sanity: the factory inside the host must look at the same place.
        factory.Target.Should().EndWith("/identity");

        await using var conn = await factory.OpenAsync(CancellationToken.None);
        conn.State.Should().Be(ConnectionState.Open);
    }
}
