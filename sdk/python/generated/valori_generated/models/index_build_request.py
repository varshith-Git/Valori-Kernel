from collections.abc import Mapping
from typing import (
    TYPE_CHECKING,
    Any,
    TypeVar,
    Union,
    cast,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.buildable_index_kind import BuildableIndexKind
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.index_build_parameters import IndexBuildParameters


T = TypeVar("T", bound="IndexBuildRequest")


@_attrs_define
class IndexBuildRequest:
    """The request body for `POST /v1/namespaces/{name}/index`.

    Attributes:
        parameters (Union[Unset, IndexBuildParameters]): The tuning knobs `POST /v1/namespaces/{name}/index` actually
            reads.

            Phase API-3.3: [`IndexBuildRequest::parameters`] is a `serde_json::Value`,
            which utoipa rendered as a schema with no `type` at all — `unknown` in
            TypeScript, `Any` in Python, and nothing whatsoever for a user to discover
            the knob names from. It was the only genuinely untyped field in the public
            surface.

            The runtime is not actually open-ended: both routers read exactly five
            keys, all unsigned integers — `m`, `ef_construction`, `ef_search` for HNSW
            (`server.rs` / `cluster_server.rs`, the `"hnsw"` arm) and `n_list`,
            `n_probe` for IVF (the `"ivf"` arm). This type names them.

            `additionalProperties` stays open because the documented behaviour is that
            unknown keys are *ignored*, not rejected — so a client sending one is not
            making an error, and the schema must not claim otherwise.
        type_ (Union[BuildableIndexKind, None, Unset]):
    """

    parameters: Union[Unset, "IndexBuildParameters"] = UNSET
    type_: Union[BuildableIndexKind, None, Unset] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        parameters: Union[Unset, dict[str, Any]] = UNSET
        if not isinstance(self.parameters, Unset):
            parameters = self.parameters.to_dict()

        type_: Union[None, Unset, str]
        if isinstance(self.type_, Unset):
            type_ = UNSET
        elif isinstance(self.type_, BuildableIndexKind):
            type_ = self.type_.value
        else:
            type_ = self.type_

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if parameters is not UNSET:
            field_dict["parameters"] = parameters
        if type_ is not UNSET:
            field_dict["type"] = type_

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.index_build_parameters import IndexBuildParameters

        d = dict(src_dict)
        _parameters = d.pop("parameters", UNSET)
        parameters: Union[Unset, IndexBuildParameters]
        if isinstance(_parameters, Unset):
            parameters = UNSET
        else:
            parameters = IndexBuildParameters.from_dict(_parameters)

        def _parse_type_(data: object) -> Union[BuildableIndexKind, None, Unset]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                type_type_1 = BuildableIndexKind(data)

                return type_type_1
            except:  # noqa: E722
                pass
            return cast(Union[BuildableIndexKind, None, Unset], data)

        type_ = _parse_type_(d.pop("type", UNSET))

        index_build_request = cls(
            parameters=parameters,
            type_=type_,
        )

        index_build_request.additional_properties = d
        return index_build_request

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
