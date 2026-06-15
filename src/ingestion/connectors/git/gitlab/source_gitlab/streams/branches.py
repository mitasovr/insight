from __future__ import annotations

from collections.abc import Mapping
from typing import Any

from source_gitlab.streams.base import GitlabSubstream


class BranchesStream(GitlabSubstream):
    name = "branches"

    def _path(self, *, stream_slice: Mapping[str, Any] | None) -> str:
        project_id = self._parent_value(stream_slice, "id")
        return f"projects/{project_id}/repository/branches"

    def _initial_params(
        self, stream_slice: Mapping[str, Any] | None
    ) -> Mapping[str, Any]:
        return {"per_page": self.page_size}

    def _record_key(
        self, record: Mapping[str, Any], stream_slice: Mapping[str, Any] | None
    ) -> list[str]:
        project_id = (stream_slice or {})["parent"]["id"]
        return [str(project_id), str(record["name"])]

    def _project(
        self, record: Mapping[str, Any], stream_slice: Mapping[str, Any] | None
    ) -> Mapping[str, Any]:
        project_id = (stream_slice or {})["parent"]["id"]
        commit = record.get("commit") or {}
        return {
            "project_id": project_id,
            "name": record.get("name"),
            "commit_sha": commit.get("id"),
            "default": record.get("default"),
            "protected": record.get("protected"),
            "merged": record.get("merged"),
        }
