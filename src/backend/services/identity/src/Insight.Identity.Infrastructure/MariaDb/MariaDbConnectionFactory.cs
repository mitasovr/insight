using System.Text.RegularExpressions;
using Microsoft.Extensions.Options;
using MySqlConnector;

namespace Insight.Identity.Infrastructure.MariaDb;

/// <summary>
/// Builds opened <see cref="MySqlConnection"/>s from
/// <see cref="MariaDbOptions"/>. When <see cref="MariaDbOptions.ConnectionString"/>
/// is set it is used verbatim (with overrides for pool/timeout knobs);
/// otherwise <see cref="MariaDbOptions.Url"/> is parsed via an explicit
/// regex (URLs use the <c>mysql://user:pass@host:port/db</c> form for
/// parity with the Rust service). <see cref="System.Uri"/> is intentionally
/// avoided because its generic-scheme parsing applies non-obvious rules
/// to <c>mysql://</c> that drop or rewrite user-info under some inputs.
/// </summary>
public sealed partial class MariaDbConnectionFactory
{
    private readonly string _connectionString;

    public MariaDbConnectionFactory(IOptions<MariaDbOptions> options)
    {
        ArgumentNullException.ThrowIfNull(options);
        _connectionString = BuildConnectionString(options.Value);
    }

    public string ConnectionString => _connectionString;

    /// <summary>Sanitised "server:port/db" for diagnostics — no creds.</summary>
    public string Target
    {
        get
        {
            var b = new MySqlConnectionStringBuilder(_connectionString);
            return $"{b.Server}:{b.Port}/{b.Database}";
        }
    }

    public async Task<MySqlConnection> OpenAsync(CancellationToken cancellationToken)
    {
        var connection = new MySqlConnection(_connectionString);
        try
        {
            await connection.OpenAsync(cancellationToken).ConfigureAwait(false);
            return connection;
        }
        catch
        {
            await connection.DisposeAsync().ConfigureAwait(false);
            throw;
        }
    }

    private static string BuildConnectionString(MariaDbOptions options)
    {
        MySqlConnectionStringBuilder builder;
        if (!string.IsNullOrWhiteSpace(options.ConnectionString))
        {
            builder = new MySqlConnectionStringBuilder(options.ConnectionString);
        }
        else if (!string.IsNullOrWhiteSpace(options.Url))
        {
            builder = ParseUrl(options.Url);
        }
        else
        {
            throw new ArgumentException(
                "mariadb options must set either 'url' or 'connection_string'",
                nameof(options));
        }

        // Only override values the operator explicitly set (non-zero).
        // MySqlConnector defaults are sane (CommandTimeout=30s,
        // CharacterSet auto-negotiated, Pooling=true, MaxPoolSize=100);
        // forcing them below the safe range — e.g. CommandTimeout=5
        // — interrupts the post-handshake `SET NAMES` round-trip and
        // reports as `Connect Timeout expired` from the pool layer.
        builder.Pooling = true;
        if (options.MinPoolSize > 0)
        {
            builder.MinimumPoolSize = (uint)options.MinPoolSize;
        }
        if (options.MaxPoolSize > 0)
        {
            builder.MaximumPoolSize = (uint)options.MaxPoolSize;
        }
        if (options.ConnectionTimeoutSeconds > 0)
        {
            builder.ConnectionTimeout = (uint)options.ConnectionTimeoutSeconds;
        }
        if (options.CommandTimeoutSeconds > 0)
        {
            builder.DefaultCommandTimeout = (uint)options.CommandTimeoutSeconds;
        }
        return builder.ConnectionString;
    }

    private static MySqlConnectionStringBuilder ParseUrl(string url)
    {
        var match = UrlRegex().Match(url);
        if (!match.Success)
        {
            // Don't echo the raw url — it can contain user:pass that
            // would otherwise leak into operator logs / exception output.
            throw new ArgumentException(
                "mariadb.url must match 'mysql://[user[:pass]@]host[:port]/db'");
        }

        var port = match.Groups["port"].Success
            ? uint.Parse(match.Groups["port"].Value, System.Globalization.CultureInfo.InvariantCulture)
            : 3306u;

        return new MySqlConnectionStringBuilder
        {
            Server = match.Groups["host"].Value,
            Port = port,
            UserID = Uri.UnescapeDataString(match.Groups["user"].Value),
            Password = Uri.UnescapeDataString(match.Groups["pass"].Value),
            Database = match.Groups["db"].Value,
        };
    }

    [GeneratedRegex(
        @"^(?:mysql|mariadb)://(?:(?<user>[^:@/]*)(?::(?<pass>[^@/]*))?@)?(?<host>[^:/?#]+)(?::(?<port>\d+))?/(?<db>[^?#]+)$",
        RegexOptions.IgnoreCase | RegexOptions.CultureInvariant)]
    private static partial Regex UrlRegex();
}
