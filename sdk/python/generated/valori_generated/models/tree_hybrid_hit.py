from collections.abc import Mapping
from typing import (
    Any,
    TypeVar,
    Union,
    cast,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="TreeHybridHit")


@_attrs_define
class TreeHybridHit:
    """
    Attributes:
        score (float):
        source (str):
        breadcrumb (Union[None, Unset, str]):
        distance (Union[None, Unset, float]):
        lines (Union[None, Unset, list[int]]):
        node_id (Union[None, Unset, str]):
        record_id (Union[None, Unset, int]):
        text (Union[None, Unset, str]):
        title (Union[None, Unset, str]):
    """

    score: float
    source: str
    breadcrumb: Union[None, Unset, str] = UNSET
    distance: Union[None, Unset, float] = UNSET
    lines: Union[None, Unset, list[int]] = UNSET
    node_id: Union[None, Unset, str] = UNSET
    record_id: Union[None, Unset, int] = UNSET
    text: Union[None, Unset, str] = UNSET
    title: Union[None, Unset, str] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        score = self.score

        source = self.source

        breadcrumb: Union[None, Unset, str]
        if isinstance(self.breadcrumb, Unset):
            breadcrumb = UNSET
        else:
            breadcrumb = self.breadcrumb

        distance: Union[None, Unset, float]
        if isinstance(self.distance, Unset):
            distance = UNSET
        else:
            distance = self.distance

        lines: Union[None, Unset, list[int]]
        if isinstance(self.lines, Unset):
            lines = UNSET
        elif isinstance(self.lines, list):
            lines = self.lines

        else:
            lines = self.lines

        node_id: Union[None, Unset, str]
        if isinstance(self.node_id, Unset):
            node_id = UNSET
        else:
            node_id = self.node_id

        record_id: Union[None, Unset, int]
        if isinstance(self.record_id, Unset):
            record_id = UNSET
        else:
            record_id = self.record_id

        text: Union[None, Unset, str]
        if isinstance(self.text, Unset):
            text = UNSET
        else:
            text = self.text

        title: Union[None, Unset, str]
        if isinstance(self.title, Unset):
            title = UNSET
        else:
            title = self.title

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "score": score,
                "source": source,
            }
        )
        if breadcrumb is not UNSET:
            field_dict["breadcrumb"] = breadcrumb
        if distance is not UNSET:
            field_dict["distance"] = distance
        if lines is not UNSET:
            field_dict["lines"] = lines
        if node_id is not UNSET:
            field_dict["node_id"] = node_id
        if record_id is not UNSET:
            field_dict["record_id"] = record_id
        if text is not UNSET:
            field_dict["text"] = text
        if title is not UNSET:
            field_dict["title"] = title

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        score = d.pop("score")

        source = d.pop("source")

        def _parse_breadcrumb(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        breadcrumb = _parse_breadcrumb(d.pop("breadcrumb", UNSET))

        def _parse_distance(data: object) -> Union[None, Unset, float]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, float], data)

        distance = _parse_distance(d.pop("distance", UNSET))

        def _parse_lines(data: object) -> Union[None, Unset, list[int]]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, list):
                    raise TypeError()
                lines_type_0 = cast(list[int], data)

                return lines_type_0
            except:  # noqa: E722
                pass
            return cast(Union[None, Unset, list[int]], data)

        lines = _parse_lines(d.pop("lines", UNSET))

        def _parse_node_id(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        node_id = _parse_node_id(d.pop("node_id", UNSET))

        def _parse_record_id(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        record_id = _parse_record_id(d.pop("record_id", UNSET))

        def _parse_text(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        text = _parse_text(d.pop("text", UNSET))

        def _parse_title(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        title = _parse_title(d.pop("title", UNSET))

        tree_hybrid_hit = cls(
            score=score,
            source=source,
            breadcrumb=breadcrumb,
            distance=distance,
            lines=lines,
            node_id=node_id,
            record_id=record_id,
            text=text,
            title=title,
        )

        tree_hybrid_hit.additional_properties = d
        return tree_hybrid_hit

    @property
    def additional_keys(self) -> list[str]:
        return list(self.additional_properties.keys())

    def __getitem__(self, key: str) -> Any:
        return self.additional_properties[key]

    def __setitem__(self, key: str, value: Any) -> None:
        self.additional_properties[key] = value

    def __delitem__(self, key: str) -> None:
        del self.additional_properties[key]

    def __contains__(self, key: str) -> bool:
        return key in self.additional_properties
