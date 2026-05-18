using FluentAssertions;
using Insight.Identity.Infrastructure.MariaDb;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.Options;
using MySqlConnector;
using Xunit;

namespace Insight.Identity.Tests.Unit;

/// <summary>
/// Verifies the configuration → options → connection-string pipeline
/// without touching a real MariaDB. If any of these fail, the integration
/// tests cannot pass — they exercise the same path.
/// </summary>
public sealed class MariaDbConnectionFactoryTests
{
    private const string TcStyleConnectionString =
        "Server=127.0.0.1;Port=49153;Database=identity;Uid=insight;Pwd=insight-pass";

    [Fact]
    public void Binding_populates_connection_string_from_snake_case_key()
    {
        var config = new ConfigurationBuilder()
            .AddInMemoryCollection(new Dictionary<string, string?>
            {
                ["mariadb:connection_string"] = TcStyleConnectionString,
            })
            .Build();

        var options = new MariaDbOptions { Url = null };
        config.GetSection(MariaDbOptions.SectionName).Bind(options);

        options.ConnectionString.Should().Be(TcStyleConnectionString);
        options.Url.Should().BeNull();
    }

    [Fact]
    public void Factory_uses_connection_string_verbatim_when_set()
    {
        var options = Options.Create(new MariaDbOptions
        {
            ConnectionString = TcStyleConnectionString,
            Url = null,
        });

        var factory = new MariaDbConnectionFactory(options);
        var built = new MySqlConnectionStringBuilder(factory.ConnectionString);

        built.Server.Should().Be("127.0.0.1");
        built.Port.Should().Be(49153);
        built.Database.Should().Be("identity");
        built.UserID.Should().Be("insight");
        built.Password.Should().Be("insight-pass");
    }

    [Fact]
    public void Factory_keeps_driver_defaults_when_options_zero()
    {
        var options = Options.Create(new MariaDbOptions
        {
            ConnectionString = TcStyleConnectionString,
            Url = null,
        });

        var built = new MySqlConnectionStringBuilder(new MariaDbConnectionFactory(options).ConnectionString);

        // MySqlConnector defaults must survive the overlay so the
        // post-handshake `SET NAMES` round-trip has its full 30s.
        built.ConnectionTimeout.Should().Be(15u);
        built.DefaultCommandTimeout.Should().Be(30u);
        built.Pooling.Should().BeTrue();
    }

    [Fact]
    public void Factory_applies_explicit_overrides_when_options_nonzero()
    {
        var options = Options.Create(new MariaDbOptions
        {
            ConnectionString = TcStyleConnectionString,
            Url = null,
            ConnectionTimeoutSeconds = 7,
            CommandTimeoutSeconds = 25,
            MaxPoolSize = 8,
        });

        var built = new MySqlConnectionStringBuilder(new MariaDbConnectionFactory(options).ConnectionString);

        built.ConnectionTimeout.Should().Be(7u);
        built.DefaultCommandTimeout.Should().Be(25u);
        built.MaximumPoolSize.Should().Be(8u);
    }

    [Fact]
    public void Factory_falls_back_to_url_when_connection_string_absent()
    {
        var options = Options.Create(new MariaDbOptions
        {
            Url = "mysql://insight:insight-pass@127.0.0.1:49153/identity",
            ConnectionString = null,
        });

        var built = new MySqlConnectionStringBuilder(new MariaDbConnectionFactory(options).ConnectionString);

        built.Server.Should().Be("127.0.0.1");
        built.Port.Should().Be(49153u);
        built.UserID.Should().Be("insight");
        built.Password.Should().Be("insight-pass");
        built.Database.Should().Be("identity");
    }

    [Fact]
    public void Factory_throws_when_neither_form_set()
    {
        var options = Options.Create(new MariaDbOptions
        {
            Url = null,
            ConnectionString = null,
        });

        Action act = () => _ = new MariaDbConnectionFactory(options);
        act.Should().Throw<ArgumentException>();
    }
}
