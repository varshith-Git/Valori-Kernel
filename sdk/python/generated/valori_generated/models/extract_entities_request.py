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

T = TypeVar("T", bound="ExtractEntitiesRequest")


@_attrs_define
class ExtractEntitiesRequest:
    """
    Attributes:
        text (str):
        entity_types (Union[Unset, list[str]]):
        model (Union[None, Unset, str]):
        namespace (Union[None, Unset, str]):
    """

    text: str
    entity_types: Union[Unset, list[str]] = UNSET
    model: Union[None, Unset, str] = UNSET
    namespace: Union[None, Unset, str] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        text = self.text

        entity_types: Union[Unset, list[str]] = UNSET
        if not isinstance(self.entity_types, Unset):
            entity_types = self.entity_types

        model: Union[None, Unset, str]
        if isinstance(self.model, Unset):
            model = UNSET
        else:
            model = self.model

        namespace: Union[None, Unset, str]
        if isinstance(self.namespace, Unset):
            namespace = UNSET
        else:
            namespace = self.namespace

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "text": text,
            }
        )
        if entity_types is not UNSET:
            field_dict["entity_types"] = entity_types
        if model is not UNSET:
            field_dict["model"] = model
        if namespace is not UNSET:
            field_dict["namespace"] = namespace

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        text = d.pop("text")

        entity_types = cast(list[str], d.pop("entity_types", UNSET))

        def _parse_model(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        model = _parse_model(d.pop("model", UNSET))

        def _parse_namespace(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        namespace = _parse_namespace(d.pop("namespace", UNSET))

        extract_entities_request = cls(
            text=text,
            entity_types=entity_types,
            model=model,
            namespace=namespace,
        )

        extract_entities_request.additional_properties = d
        return extract_entities_request

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
