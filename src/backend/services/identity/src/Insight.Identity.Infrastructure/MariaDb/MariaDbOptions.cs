using Microsoft.Extensions.Configuration;

namespace Insight.Identity.Infrastructure.MariaDb;

/// <summary>
/// MariaDB connection options. Bound from configuration under the
/// <c>mariadb</c> section. Either <see cref="Url"/> (Rust-parity URL form)
/// or <see cref="ConnectionString"/> (raw MySqlConnector KV form) must be
/// set; <see cref="ConnectionString"/> wins when both are present so test
/// harnesses can hand over a Testcontainers-supplied connection string
/// without lossy URL round-tripping. <see cref="ConfigurationKeyNameAttribute"/>
/// maps snake_case keys onto PascalCase properties — the default binder
/// only matches case insensitively and does not translate underscores.
/// </summary>
public sealed class MariaDbOptions
{
    public const string SectionName = "mariadb";

    /// <summary>
    /// <c>mysql://user:pass@host:port/db</c> URL form. Used in production
    /// for parity with the Rust service's <c>IDENTITY__mariadb__url</c>
    /// env var.
    /// </summary>
    public string? Url { get; init; }

    /// <summary>
    /// Raw MySqlConnector connection string in <c>Server=…;Port=…;Uid=…</c>
    /// form. Bypasses URL parsing entirely; intended for tests and for
    /// operators who need to pass MariaDB-specific options
    /// (<c>SslMode</c>, <c>AllowPublicKeyRetrieval</c>, …) that the URL
    /// shape cannot express.
    /// </summary>
    [ConfigurationKeyName("connection_string")]
    public string? ConnectionString { get; init; }

    /// <summary>
    /// Pool/timeout overrides. Zero means "use MySqlConnector default"
    /// (Pooling=true, MinPoolSize=0, MaxPoolSize=100, ConnectionTimeout=15s,
    /// CommandTimeout=30s). Non-zero values are written into the connection
    /// string verbatim — set them only when you have a measured reason.
    /// </summary>
    [ConfigurationKeyName("min_pool_size")]
    public int MinPoolSize { get; init; }

    [ConfigurationKeyName("max_pool_size")]
    public int MaxPoolSize { get; init; }

    [ConfigurationKeyName("connection_timeout_seconds")]
    public int ConnectionTimeoutSeconds { get; init; }

    [ConfigurationKeyName("command_timeout_seconds")]
    public int CommandTimeoutSeconds { get; init; }
}
