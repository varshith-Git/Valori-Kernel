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

T = TypeVar("T", bound="IndexBuildParameters")


@_attrs_define
class IndexBuildParameters:
    """The tuning knobs `POST /v1/namespaces/{name}/index` actually reads.

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

        Attributes:
            ef_construction (Union[None, Unset, int]): HNSW: candidate-list size during construction.
            ef_search (Union[None, Unset, int]): HNSW: candidate-list size during search.
            m (Union[None, Unset, int]): HNSW: neighbours per node. `m_max0` is derived as `2 * m`.
            n_list (Union[None, Unset, int]): IVF: centroid count. Omit to auto-scale to `max(16, sqrt(N))`.
            n_probe (Union[None, Unset, int]): IVF: probe count. Omit to auto-scale to `max(1, sqrt(n_list))`.
    """

    ef_construction: Union[None, Unset, int] = UNSET
    ef_search: Union[None, Unset, int] = UNSET
    m: Union[None, Unset, int] = UNSET
    n_list: Union[None, Unset, int] = UNSET
    n_probe: Union[None, Unset, int] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        ef_construction: Union[None, Unset, int]
        if isinstance(self.ef_construction, Unset):
            ef_construction = UNSET
        else:
            ef_construction = self.ef_construction

        ef_search: Union[None, Unset, int]
        if isinstance(self.ef_search, Unset):
            ef_search = UNSET
        else:
            ef_search = self.ef_search

        m: Union[None, Unset, int]
        if isinstance(self.m, Unset):
            m = UNSET
        else:
            m = self.m

        n_list: Union[None, Unset, int]
        if isinstance(self.n_list, Unset):
            n_list = UNSET
        else:
            n_list = self.n_list

        n_probe: Union[None, Unset, int]
        if isinstance(self.n_probe, Unset):
            n_probe = UNSET
        else:
            n_probe = self.n_probe

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if ef_construction is not UNSET:
            field_dict["ef_construction"] = ef_construction
        if ef_search is not UNSET:
            field_dict["ef_search"] = ef_search
        if m is not UNSET:
            field_dict["m"] = m
        if n_list is not UNSET:
            field_dict["n_list"] = n_list
        if n_probe is not UNSET:
            field_dict["n_probe"] = n_probe

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)

        def _parse_ef_construction(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        ef_construction = _parse_ef_construction(d.pop("ef_construction", UNSET))

        def _parse_ef_search(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        ef_search = _parse_ef_search(d.pop("ef_search", UNSET))

        def _parse_m(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        m = _parse_m(d.pop("m", UNSET))

        def _parse_n_list(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        n_list = _parse_n_list(d.pop("n_list", UNSET))

        def _parse_n_probe(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        n_probe = _parse_n_probe(d.pop("n_probe", UNSET))

        index_build_parameters = cls(
            ef_construction=ef_construction,
            ef_search=ef_search,
            m=m,
            n_list=n_list,
            n_probe=n_probe,
        )

        index_build_parameters.additional_properties = d
        return index_build_parameters

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
