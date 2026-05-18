namespace Insight.Identity.Domain.Services;

/// <summary>
/// Best-effort split of a <c>display_name</c> into first/last name when
/// dedicated <c>first_name</c> / <c>last_name</c> observations are missing.
/// Two formats are supported, in order of priority:
/// <list type="number">
///   <item><c>"Last, First"</c> — comma-separated → first=after-comma, last=before-comma.</item>
///   <item><c>"First Last"</c> — space-separated → first=first token, last=remaining tokens.</item>
/// </list>
/// Single-token names yield first=token, last="". Empty/whitespace yields ("", "").
/// </summary>
public static class DisplayNameSplitter
{
    public static (string FirstName, string LastName) Split(string? displayName)
    {
        if (string.IsNullOrWhiteSpace(displayName))
        {
            return (string.Empty, string.Empty);
        }

        var trimmed = displayName.Trim();

        if (SplitOn(trimmed, ',') is var (commaBefore, commaAfter) && commaBefore is not null)
        {
            // "Last, First" → first goes after the comma, last before.
            return (commaAfter!, commaBefore);
        }

        if (SplitOn(trimmed, ' ') is var (spaceBefore, spaceAfter) && spaceBefore is not null)
        {
            // "First Rest" → first goes before the space; rest keeps any middle names.
            return (spaceBefore, spaceAfter!);
        }

        return (trimmed, string.Empty);
    }

    /// <summary>
    /// Splits <paramref name="input"/> on the first occurrence of
    /// <paramref name="delimiter"/>, returning the trimmed left and right
    /// halves. Returns <c>(null, null)</c> when the delimiter is absent.
    /// </summary>
    private static (string? Left, string? Right) SplitOn(string input, char delimiter)
    {
        var idx = input.IndexOf(delimiter);
        return idx < 0
            ? (null, null)
            : (input[..idx].Trim(), input[(idx + 1)..].Trim());
    }
}
