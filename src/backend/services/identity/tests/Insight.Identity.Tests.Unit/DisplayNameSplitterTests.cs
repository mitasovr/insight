using FluentAssertions;
using Insight.Identity.Domain.Services;
using Xunit;

namespace Insight.Identity.Tests.Unit;

public sealed class DisplayNameSplitterTests
{
    [Theory]
    [InlineData("Smith, Alice", "Alice", "Smith")]
    [InlineData("Alice Smith", "Alice", "Smith")]
    [InlineData("Alice Mary Smith", "Alice", "Mary Smith")]
    [InlineData("Alice", "Alice", "")]
    [InlineData("", "", "")]
    [InlineData("   ", "", "")]
    [InlineData("Smith,Alice", "Alice", "Smith")]
    [InlineData("  Smith ,  Alice  ", "Alice", "Smith")]
    public void Splits_known_formats(string input, string expectedFirst, string expectedLast)
    {
        var (first, last) = DisplayNameSplitter.Split(input);
        first.Should().Be(expectedFirst);
        last.Should().Be(expectedLast);
    }
}
