using FluentAssertions;
using Insight.Identity.Domain;
using Insight.Identity.Domain.Services;
using Xunit;

namespace Insight.Identity.Tests.Unit;

public sealed class PersonAssemblerTests
{
    private static readonly Guid PersonId = Guid.Parse("11111111-1111-1111-1111-111111111111");
    private static readonly Guid SourceId = Guid.Parse("22222222-2222-2222-2222-222222222222");

    private static PersonObservation Obs(string valueType, string value, DateTime? createdAt = null) =>
        new(PersonId, "bamboohr", SourceId, valueType, value, createdAt ?? new DateTime(2026, 1, 1, 0, 0, 0, DateTimeKind.Utc));

    [Fact]
    public void Returns_null_for_empty_observations()
    {
        var person = PersonAssembler.Assemble(PersonId, Array.Empty<PersonObservation>(), Array.Empty<Person>());
        person.Should().BeNull();
    }

    [Fact]
    public void Picks_latest_value_per_type()
    {
        var obs = new[]
        {
            Obs(ValueTypes.Email, "old@example.com", new DateTime(2025, 1, 1, 0, 0, 0, DateTimeKind.Utc)),
            Obs(ValueTypes.Email, "new@example.com", new DateTime(2026, 1, 1, 0, 0, 0, DateTimeKind.Utc)),
        };

        var person = PersonAssembler.Assemble(PersonId, obs, Array.Empty<Person>());

        person.Should().NotBeNull();
        person!.Email.Should().Be("new@example.com");
    }

    [Fact]
    public void Falls_back_to_display_name_when_first_last_absent()
    {
        var obs = new[]
        {
            Obs(ValueTypes.DisplayName, "Smith, Alice"),
            Obs(ValueTypes.Email, "alice@example.com"),
        };

        var person = PersonAssembler.Assemble(PersonId, obs, Array.Empty<Person>());

        person.Should().NotBeNull();
        person!.FirstName.Should().Be("Alice");
        person.LastName.Should().Be("Smith");
    }

    [Fact]
    public void Prefers_explicit_first_last_over_display_name_split()
    {
        var obs = new[]
        {
            Obs(ValueTypes.DisplayName, "Wrong, Person"),
            Obs(ValueTypes.FirstName, "Alice"),
            Obs(ValueTypes.LastName, "Smith"),
        };

        var person = PersonAssembler.Assemble(PersonId, obs, Array.Empty<Person>());

        person!.FirstName.Should().Be("Alice");
        person.LastName.Should().Be("Smith");
    }

    [Fact]
    public void Carries_through_org_chart_attributes()
    {
        var parentPerson = Guid.NewGuid();
        var obs = new[]
        {
            Obs(ValueTypes.Email, "alice@example.com"),
            Obs(ValueTypes.ParentEmail, "bob@example.com"),
            Obs(ValueTypes.ParentId, "BOB-7"),
            Obs(ValueTypes.ParentPersonId, parentPerson.ToString("D")),
        };

        var person = PersonAssembler.Assemble(PersonId, obs, Array.Empty<Person>());

        person!.ParentEmail.Should().Be("bob@example.com");
        person.ParentId.Should().Be("BOB-7");
        person.ParentPersonId.Should().Be(parentPerson);
    }

    [Fact]
    public void Empty_strings_for_missing_attributes()
    {
        var obs = new[] { Obs(ValueTypes.Email, "alice@example.com") };

        var person = PersonAssembler.Assemble(PersonId, obs, Array.Empty<Person>());

        person!.DisplayName.Should().BeEmpty();
        person.Department.Should().BeEmpty();
        person.JobTitle.Should().BeEmpty();
        person.Status.Should().BeEmpty();
        person.ParentEmail.Should().BeNull();
        person.ParentPersonId.Should().BeNull();
    }
}
