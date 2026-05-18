using System.Text.Json.Serialization;

namespace Insight.Identity.Api.Contracts;

/// <summary>
/// RFC 7807 problem-details body. Field shape matches the Rust stub for
/// consumer compatibility (<c>type</c>, <c>title</c>, <c>status</c>,
/// <c>detail</c>).
/// </summary>
public sealed record ProblemResponse(
    [property: JsonPropertyName("type")] string Type,
    [property: JsonPropertyName("title")] string Title,
    [property: JsonPropertyName("status")] int Status,
    [property: JsonPropertyName("detail")] string Detail);
