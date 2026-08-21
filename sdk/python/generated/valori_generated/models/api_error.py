from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.error_code import ErrorCode

T = TypeVar("T", bound="ApiError")


@_attrs_define
class ApiError:
    """The canonical error body, as a schema-bearing DTO.

    [`valori_engine::EngineError`] produces this shape at runtime but lives in
    a crate that does not depend on `utoipa`, so the schema is declared here —
    the translation layer §36 asks for, rather than a re-export of an internal
    type.

        Attributes:
            code (ErrorCode): Mirror of [`valori_engine::ErrorCode`] for schema generation.

                `tests/api_contract.rs` diffs the runtime enum against the committed YAML;
                this type exists so the generated document carries the same closed set.
            error (str): Human-readable message. Do not parse.
    """

    code: ErrorCode
    error: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        code = self.code.value

        error = self.error

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "code": code,
                "error": error,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        code = ErrorCode(d.pop("code"))

        error = d.pop("error")

        api_error = cls(
            code=code,
            error=error,
        )

        api_error.additional_properties = d
        return api_error

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
