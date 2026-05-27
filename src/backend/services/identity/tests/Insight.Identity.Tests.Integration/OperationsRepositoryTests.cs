using FluentAssertions;
using Insight.Identity.Domain;
using Insight.Identity.Domain.Services;
using Insight.Identity.Infrastructure.MariaDb;
using Xunit;

namespace Insight.Identity.Tests.Integration;

/// <summary>
/// Direct tests for <see cref="OperationsRepository"/> against the
/// Testcontainers MariaDB: enqueue → start → complete/fail lifecycle,
/// tenant-scoped read, list filtering, and the startup zombie sweep.
/// </summary>
[Collection(MariaDbCollection.Name)]
public sealed class OperationsRepositoryTests : IAsyncLifetime
{
    private static readonly Guid TenantA = Guid.Parse("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa");
    private static readonly Guid TenantB = Guid.Parse("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb");
    private static readonly Guid Author  = Guid.Parse("cccccccc-cccc-cccc-cccc-cccccccccccc");

    private readonly MariaDbFixture _fixture;
    private OperationsRepository _repo = null!;

    public OperationsRepositoryTests(MariaDbFixture fixture) => _fixture = fixture;

    public async Task InitializeAsync()
    {
        await _fixture.ResetAsync().ConfigureAwait(false);
        _repo = new OperationsRepository(new MariaDbConnectionFactory(
            Microsoft.Extensions.Options.Options.Create(new MariaDbOptions { ConnectionString = _fixture.ConnectionString })));
    }

    public Task DisposeAsync() => Task.CompletedTask;

    [Fact]
    public async Task Enqueue_then_get_returns_queued_row()
    {
        var id = await _repo.EnqueueAsync(OperationTypes.PersonsSeed, TenantA, Author, "{\"mode\":\"link-by-email\"}", default);

        var op = await _repo.GetByIdAsync(TenantA, id, default);

        op.Should().NotBeNull();
        op!.Status.Should().Be(OperationStatus.Queued);
        op.OperationType.Should().Be(OperationTypes.PersonsSeed);
        op.AuthorPersonId.Should().Be(Author);
        op.RequestJson.Should().Contain("link-by-email");
        op.CompletedAt.Should().BeNull();
    }

    [Fact]
    public async Task GetById_is_tenant_scoped()
    {
        var id = await _repo.EnqueueAsync(OperationTypes.PersonsSeed, TenantA, Author, null, default);

        var fromOtherTenant = await _repo.GetByIdAsync(TenantB, id, default);

        fromOtherTenant.Should().BeNull();
    }

    [Fact]
    public async Task TryStart_flips_queued_to_running_once()
    {
        var id = await _repo.EnqueueAsync(OperationTypes.PersonsSeed, TenantA, Author, null, default);

        var first = await _repo.TryStartAsync(id, default);
        var second = await _repo.TryStartAsync(id, default);

        first.Should().BeTrue();
        second.Should().BeFalse("a row already running cannot be started again");
        (await _repo.GetByIdAsync(TenantA, id, default))!.Status.Should().Be(OperationStatus.Running);
    }

    [Fact]
    public async Task Complete_sets_summary_and_completed_at()
    {
        var id = await _repo.EnqueueAsync(OperationTypes.PersonsSeed, TenantA, Author, null, default);
        await _repo.TryStartAsync(id, default);

        await _repo.CompleteAsync(id, "{\"accounts_read\":5}", default);

        var op = await _repo.GetByIdAsync(TenantA, id, default);
        op!.Status.Should().Be(OperationStatus.Completed);
        op.SummaryJson.Should().Contain("accounts_read");
        op.CompletedAt.Should().NotBeNull();
    }

    [Fact]
    public async Task Fail_sets_error_and_completed_at()
    {
        var id = await _repo.EnqueueAsync(OperationTypes.PersonsSeed, TenantA, Author, null, default);
        await _repo.TryStartAsync(id, default);

        await _repo.FailAsync(id, "boom", default);

        var op = await _repo.GetByIdAsync(TenantA, id, default);
        op!.Status.Should().Be(OperationStatus.Failed);
        op.ErrorMessage.Should().Be("boom");
        op.CompletedAt.Should().NotBeNull();
    }

    [Fact]
    public async Task List_filters_by_status()
    {
        var done = await _repo.EnqueueAsync(OperationTypes.PersonsSeed, TenantA, Author, null, default);
        await _repo.TryStartAsync(done, default);
        await _repo.CompleteAsync(done, "{}", default);
        await _repo.EnqueueAsync(OperationTypes.PersonsSeed, TenantA, Author, null, default); // stays queued

        var completed = await _repo.ListAsync(TenantA, OperationTypes.PersonsSeed, OperationStatus.Completed, new PageRequest(50), default);
        var queued = await _repo.ListAsync(TenantA, OperationTypes.PersonsSeed, OperationStatus.Queued, new PageRequest(50), default);

        completed.Items.Should().ContainSingle().Which.OperationId.Should().Be(done);
        queued.Items.Should().ContainSingle();
    }

    [Fact]
    public async Task SweepZombies_fails_stale_queued_and_running_rows()
    {
        var stale = await _repo.EnqueueAsync(OperationTypes.PersonsSeed, TenantA, Author, null, default);
        await _repo.TryStartAsync(stale, default);

        // Cutoff in the future → the just-created row counts as "older than".
        var swept = await _repo.SweepZombiesAsync(DateTime.UtcNow.AddHours(1), default);

        swept.Should().BeGreaterThanOrEqualTo(1);
        (await _repo.GetByIdAsync(TenantA, stale, default))!.Status.Should().Be(OperationStatus.Failed);
    }
}
