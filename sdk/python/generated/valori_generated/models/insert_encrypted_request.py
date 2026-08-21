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

T = TypeVar("T", bound="InsertEncryptedRequest")


@_attrs_define
class InsertEncryptedRequest:
    """
    Attributes:
        payload (str): Base64-encoded plaintext payload (will be encrypted by the vault).
        collection (Union[None, Unset, str]):
        key_id (Union[None, Unset, str]): Optional pre-chosen key_id (hex). If absent, a fresh key_id is generated.
        tag (Union[None, Unset, int]):
    """

    payload: str
    collection: Union[None, Unset, str] = UNSET
    key_id: Union[None, Unset, str] = UNSET
    tag: Union[None, Unset, int] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        payload = self.payload

        collection: Union[None, Unset, str]
        if isinstance(self.collection, Unset):
            collection = UNSET
        else:
            collection = self.collection

        key_id: Union[None, Unset, str]
        if isinstance(self.key_id, Unset):
            key_id = UNSET
        else:
            key_id = self.key_id

        tag: Union[None, Unset, int]
        if isinstance(self.tag, Unset):
            tag = UNSET
        else:
            tag = self.tag

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "payload": payload,
            }
        )
        if collection is not UNSET:
            field_dict["collection"] = collection
        if key_id is not UNSET:
            field_dict["key_id"] = key_id
        if tag is not UNSET:
            field_dict["tag"] = tag

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        payload = d.pop("payload")

        def _parse_collection(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        collection = _parse_collection(d.pop("collection", UNSET))

        def _parse_key_id(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        key_id = _parse_key_id(d.pop("key_id", UNSET))

        def _parse_tag(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        tag = _parse_tag(d.pop("tag", UNSET))

        insert_encrypted_request = cls(
            payload=payload,
            collection=collection,
            key_id=key_id,
            tag=tag,
        )

        insert_encrypted_request.additional_properties = d
        return insert_encrypted_request

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
