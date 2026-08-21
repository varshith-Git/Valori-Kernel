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

T = TypeVar("T", bound="IndexStatusResponse")


@_attrs_define
class IndexStatusResponse:
    """Response to `POST /v1/namespaces/{name}/index` and
    `GET /v1/namespaces/{name}/index`.

    # Cluster vs standalone distinction

    In cluster mode, `desired_type` is always populated from the Raft-
    replicated desired spec (what the cluster wants), while `active_type`
    and `status` reflect this **node's local** build state. They may
    differ temporarily as builds propagate across replicas.

    Example during a transition:
    ```json
    { "desired_type": "ivf", "active_type": "hnsw", "status": "building",
      "building_generation": 2, "active_generation": 1 }
    ```

    In standalone mode, `desired_type` is always equal to `active_type` once
    a build completes (there's only one node).

        Attributes:
            active_type (str): The currently serving index type ("hnsw", "ivf", "bq", "none").
            collection (str):
            status (str): Current lifecycle status of the active or building generation.
            active_generation (Union[None, Unset, int]): The active generation number, if any.
            base_lsn (Union[None, Unset, int]): The base LSN of the building generation.
            build_started_at (Union[None, Unset, int]): Unix seconds when the current build started.
            building_generation (Union[None, Unset, int]): If a build is in progress, its generation number.
            desired_type (Union[None, Unset, str]): The type the user requested (may differ from active while building).
                In cluster mode, this comes from the Raft-replicated desired spec and
                is authoritative for the whole cluster, not just the responding node.
            error (Union[None, Unset, str]): Human-readable failure reason, if the last build failed.
    """

    active_type: str
    collection: str
    status: str
    active_generation: Union[None, Unset, int] = UNSET
    base_lsn: Union[None, Unset, int] = UNSET
    build_started_at: Union[None, Unset, int] = UNSET
    building_generation: Union[None, Unset, int] = UNSET
    desired_type: Union[None, Unset, str] = UNSET
    error: Union[None, Unset, str] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        active_type = self.active_type

        collection = self.collection

        status = self.status

        active_generation: Union[None, Unset, int]
        if isinstance(self.active_generation, Unset):
            active_generation = UNSET
        else:
            active_generation = self.active_generation

        base_lsn: Union[None, Unset, int]
        if isinstance(self.base_lsn, Unset):
            base_lsn = UNSET
        else:
            base_lsn = self.base_lsn

        build_started_at: Union[None, Unset, int]
        if isinstance(self.build_started_at, Unset):
            build_started_at = UNSET
        else:
            build_started_at = self.build_started_at

        building_generation: Union[None, Unset, int]
        if isinstance(self.building_generation, Unset):
            building_generation = UNSET
        else:
            building_generation = self.building_generation

        desired_type: Union[None, Unset, str]
        if isinstance(self.desired_type, Unset):
            desired_type = UNSET
        else:
            desired_type = self.desired_type

        error: Union[None, Unset, str]
        if isinstance(self.error, Unset):
            error = UNSET
        else:
            error = self.error

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "active_type": active_type,
                "collection": collection,
                "status": status,
            }
        )
        if active_generation is not UNSET:
            field_dict["active_generation"] = active_generation
        if base_lsn is not UNSET:
            field_dict["base_lsn"] = base_lsn
        if build_started_at is not UNSET:
            field_dict["build_started_at"] = build_started_at
        if building_generation is not UNSET:
            field_dict["building_generation"] = building_generation
        if desired_type is not UNSET:
            field_dict["desired_type"] = desired_type
        if error is not UNSET:
            field_dict["error"] = error

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        active_type = d.pop("active_type")

        collection = d.pop("collection")

        status = d.pop("status")

        def _parse_active_generation(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        active_generation = _parse_active_generation(d.pop("active_generation", UNSET))

        def _parse_base_lsn(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        base_lsn = _parse_base_lsn(d.pop("base_lsn", UNSET))

        def _parse_build_started_at(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        build_started_at = _parse_build_started_at(d.pop("build_started_at", UNSET))

        def _parse_building_generation(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        building_generation = _parse_building_generation(
            d.pop("building_generation", UNSET)
        )

        def _parse_desired_type(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        desired_type = _parse_desired_type(d.pop("desired_type", UNSET))

        def _parse_error(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        error = _parse_error(d.pop("error", UNSET))

        index_status_response = cls(
            active_type=active_type,
            collection=collection,
            status=status,
            active_generation=active_generation,
            base_lsn=base_lsn,
            build_started_at=build_started_at,
            building_generation=building_generation,
            desired_type=desired_type,
            error=error,
        )

        index_status_response.additional_properties = d
        return index_status_response

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
