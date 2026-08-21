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
    from ..models.hnsw_config_view import HnswConfigView


T = TypeVar("T", bound="IndexConfigResponse")


@_attrs_define
class IndexConfigResponse:
    """
    Attributes:
        index_type (str):
        hnsw (Union['HnswConfigView', None, Unset]):
    """

    index_type: str
    hnsw: Union["HnswConfigView", None, Unset] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.hnsw_config_view import HnswConfigView

        index_type = self.index_type

        hnsw: Union[None, Unset, dict[str, Any]]
        if isinstance(self.hnsw, Unset):
            hnsw = UNSET
        elif isinstance(self.hnsw, HnswConfigView):
            hnsw = self.hnsw.to_dict()
        else:
            hnsw = self.hnsw

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "index_type": index_type,
            }
        )
        if hnsw is not UNSET:
            field_dict["hnsw"] = hnsw

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.hnsw_config_view import HnswConfigView

        d = dict(src_dict)
        index_type = d.pop("index_type")

        def _parse_hnsw(data: object) -> Union["HnswConfigView", None, Unset]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                hnsw_type_1 = HnswConfigView.from_dict(data)

                return hnsw_type_1
            except:  # noqa: E722
                pass
            return cast(Union["HnswConfigView", None, Unset], data)

        hnsw = _parse_hnsw(d.pop("hnsw", UNSET))

        index_config_response = cls(
            index_type=index_type,
            hnsw=hnsw,
        )

        index_config_response.additional_properties = d
        return index_config_response

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
