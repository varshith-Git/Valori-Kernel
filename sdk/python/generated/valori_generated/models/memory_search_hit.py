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

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.memory_search_hit_metadata_type_0 import MemorySearchHitMetadataType0


T = TypeVar("T", bound="MemorySearchHit")


@_attrs_define
class MemorySearchHit:
    """
    Attributes:
        memory_id (str):
        record_id (int):
        score (float):
        age_secs (Union[None, Unset, int]): Phase C4.1 — record age in seconds; present only when decay is active.
        decay_factor (Union[None, Unset, float]): Phase C4.1 — applied decay factor in (0, 1]; present only when decay
            is active.
        metadata (Union['MemorySearchHitMetadataType0', None, Unset]):
    """

    memory_id: str
    record_id: int
    score: float
    age_secs: Union[None, Unset, int] = UNSET
    decay_factor: Union[None, Unset, float] = UNSET
    metadata: Union["MemorySearchHitMetadataType0", None, Unset] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.memory_search_hit_metadata_type_0 import (
            MemorySearchHitMetadataType0,
        )

        memory_id = self.memory_id

        record_id = self.record_id

        score = self.score

        age_secs: Union[None, Unset, int]
        if isinstance(self.age_secs, Unset):
            age_secs = UNSET
        else:
            age_secs = self.age_secs

        decay_factor: Union[None, Unset, float]
        if isinstance(self.decay_factor, Unset):
            decay_factor = UNSET
        else:
            decay_factor = self.decay_factor

        metadata: Union[None, Unset, dict[str, Any]]
        if isinstance(self.metadata, Unset):
            metadata = UNSET
        elif isinstance(self.metadata, MemorySearchHitMetadataType0):
            metadata = self.metadata.to_dict()
        else:
            metadata = self.metadata

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "memory_id": memory_id,
                "record_id": record_id,
                "score": score,
            }
        )
        if age_secs is not UNSET:
            field_dict["age_secs"] = age_secs
        if decay_factor is not UNSET:
            field_dict["decay_factor"] = decay_factor
        if metadata is not UNSET:
            field_dict["metadata"] = metadata

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.memory_search_hit_metadata_type_0 import (
            MemorySearchHitMetadataType0,
        )

        d = dict(src_dict)
        memory_id = d.pop("memory_id")

        record_id = d.pop("record_id")

        score = d.pop("score")

        def _parse_age_secs(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        age_secs = _parse_age_secs(d.pop("age_secs", UNSET))

        def _parse_decay_factor(data: object) -> Union[None, Unset, float]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, float], data)

        decay_factor = _parse_decay_factor(d.pop("decay_factor", UNSET))

        def _parse_metadata(
            data: object,
        ) -> Union["MemorySearchHitMetadataType0", None, Unset]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                metadata_type_0 = MemorySearchHitMetadataType0.from_dict(data)

                return metadata_type_0
            except:  # noqa: E722
                pass
            return cast(Union["MemorySearchHitMetadataType0", None, Unset], data)

        metadata = _parse_metadata(d.pop("metadata", UNSET))

        memory_search_hit = cls(
            memory_id=memory_id,
            record_id=record_id,
            score=score,
            age_secs=age_secs,
            decay_factor=decay_factor,
            metadata=metadata,
        )

        memory_search_hit.additional_properties = d
        return memory_search_hit

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
