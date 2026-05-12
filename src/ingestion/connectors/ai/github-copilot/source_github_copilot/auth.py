"""GitHub Copilot authentication helpers.

Two distinct header sets per the connector's two-step fetch pattern:

  rest_headers(token)      — for api.github.com (Bearer auth + GitHub API headers)
  download_headers()       — for copilot-reports.github.com (NO auth — signed URLs are
                             pre-authenticated; sending Authorization to this domain
                             may cause request failures)

See cpt-insightspec-constraint-ghcopilot-no-auth-download in DESIGN §2.2.
"""


def rest_headers(token: str) -> dict:
    """Headers for authenticated requests to api.github.com."""
    return {
        "Authorization": f"Bearer {token}",
        "Accept": "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28",
        "User-Agent": "insight-github-copilot-connector/0.1",
    }


def download_headers() -> dict:
    """Headers for downloading NDJSON from signed URLs on copilot-reports.github.com.

    MUST NOT include Authorization. URLs are pre-authenticated; an Authorization
    header on this domain may be rejected.
    """
    return {
        "User-Agent": "insight-github-copilot-connector/0.1",
        "Accept": "application/x-ndjson, text/plain, */*",
    }
