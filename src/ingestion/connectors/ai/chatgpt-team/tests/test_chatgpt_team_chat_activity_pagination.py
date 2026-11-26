"""The chat activity walk must reach the end of a day and then stop.

`user_list` pages with a `cursor` and the `query_id` of the first answer. It
ignores a parameter it does not know and answers page one again, with a new
`next_cursor` each time. A walk that sends the wrong parameter never ends, and
nothing in one answer shows that it is a repeat. The mock answers a follow-up
page only when the request carries both values, so a wrong parameter fails the
read instead of looping.
"""

from __future__ import annotations

import json

import pytest
import yaml
from config import ACCOUNT_ID, PROXY_URL, ChatGptTeamConfigBuilder
from connector_tests import HttpMocker, HttpRequest, HttpResponse, connector_dir, load_fixture, read_stream
from freezegun import freeze_time

_CONNECTOR = "ai/chatgpt-team"
_STREAM = "chatgpt_team_chat_activity"
_URL = f"{PROXY_URL}/api/accounts/{ACCOUNT_ID}/analytics/user_list"
_DAY = "2026-08-19"
_NOW = f"{_DAY}T11:30:00Z"
_QUERY_ID = "query-a"
_NEXT_CURSOR = "cursor-b"


def _page_size() -> int:
    manifest = yaml.safe_load((connector_dir(_CONNECTOR) / "connector.yaml").read_text())
    stream = next(s for s in manifest["streams"] if s.get("name") == _STREAM)
    return int(stream["retriever"]["paginator"]["pagination_strategy"]["page_size"])


def _emails(prefix: str, count: int) -> list[str]:
    return [f"{prefix}-{i:03d}@example.com" for i in range(count)]


def _page(emails: list[str], next_cursor: str | None, query_id: str | None) -> HttpResponse:
    body = load_fixture(__file__, "chat_activity_page.json", next_cursor=next_cursor, query_id=query_id)
    body["users"] = [load_fixture(__file__, "chat_activity_user.json", email=e) for e in emails]
    return HttpResponse(body=json.dumps(body), status_code=200)


def _params(page_size: int, **follow_up: str) -> dict[str, str]:
    return {
        "page_size": str(page_size),
        "order_by_field": "messages",
        "order_by_direction": "DESC",
        "start_date": _DAY,
        "end_date": _DAY,
        **follow_up,
    }


def _read(http_mocker: HttpMocker) -> list[str]:
    config = ChatGptTeamConfigBuilder().with_start_date(_DAY).build()
    with freeze_time(_NOW):
        output = read_stream(_CONNECTOR, _STREAM, config)
    assert not output.errors, f"the read failed: {output.errors}"
    return [r.record.data["email"] for r in output.records]


def test_a_follow_up_page_sends_the_cursor_with_the_query_id(http_mocker: HttpMocker) -> None:
    page_size = _page_size()
    first = _emails("first", page_size)
    second = _emails("second", 3)

    http_mocker.get(HttpRequest(_URL, query_params=_params(page_size)), _page(first, _NEXT_CURSOR, _QUERY_ID))
    http_mocker.get(
        HttpRequest(_URL, query_params=_params(page_size, cursor=_NEXT_CURSOR, query_id=_QUERY_ID)),
        _page(second, None, _QUERY_ID),
    )

    emails = _read(http_mocker)

    assert sorted(emails) == sorted(first + second), "the walk did not read each person of the day once"


@pytest.mark.parametrize(
    ("case", "count", "next_cursor", "query_id"),
    [
        ("a short page with a next cursor", 3, _NEXT_CURSOR, _QUERY_ID),
        ("a full page without a query id", None, _NEXT_CURSOR, None),
    ],
)
def test_the_walk_stops_where_no_valid_follow_up_exists(
    http_mocker: HttpMocker, case: str, count: int | None, next_cursor: str, query_id: str | None
) -> None:
    page_size = _page_size()
    only = _emails("only", count if count is not None else page_size)

    http_mocker.get(HttpRequest(_URL, query_params=_params(page_size)), _page(only, next_cursor, query_id))

    emails = _read(http_mocker)

    assert sorted(emails) == sorted(only), f"should stop after one page: {case}"
