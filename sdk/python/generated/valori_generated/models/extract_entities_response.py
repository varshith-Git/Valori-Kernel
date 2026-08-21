from collections.abc import Mapping
from typing import (
    TYPE_CHECKING,
    Any,
    TypeVar,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.inserted_entity import InsertedEntity
    from ..models.inserted_relationship import InsertedRelationship


T = TypeVar("T", bound="ExtractEntitiesResponse")


@_attrs_define
class ExtractEntitiesResponse:
    """
    Attributes:
        entities (list['InsertedEntity']):
        entity_count (int):
        relationship_count (int):
        relationships (list['InsertedRelationship']):
        skipped_relationships (int):
    """

    entities: list["InsertedEntity"]
    entity_count: int
    relationship_count: int
    relationships: list["InsertedRelationship"]
    skipped_relationships: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        entities = []
        for entities_item_data in self.entities:
            entities_item = entities_item_data.to_dict()
            entities.append(entities_item)

        entity_count = self.entity_count

        relationship_count = self.relationship_count

        relationships = []
        for relationships_item_data in self.relationships:
            relationships_item = relationships_item_data.to_dict()
            relationships.append(relationships_item)

        skipped_relationships = self.skipped_relationships

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "entities": entities,
                "entity_count": entity_count,
                "relationship_count": relationship_count,
                "relationships": relationships,
                "skipped_relationships": skipped_relationships,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.inserted_entity import InsertedEntity
        from ..models.inserted_relationship import InsertedRelationship

        d = dict(src_dict)
        entities = []
        _entities = d.pop("entities")
        for entities_item_data in _entities:
            entities_item = InsertedEntity.from_dict(entities_item_data)

            entities.append(entities_item)

        entity_count = d.pop("entity_count")

        relationship_count = d.pop("relationship_count")

        relationships = []
        _relationships = d.pop("relationships")
        for relationships_item_data in _relationships:
            relationships_item = InsertedRelationship.from_dict(relationships_item_data)

            relationships.append(relationships_item)

        skipped_relationships = d.pop("skipped_relationships")

        extract_entities_response = cls(
            entities=entities,
            entity_count=entity_count,
            relationship_count=relationship_count,
            relationships=relationships,
            skipped_relationships=skipped_relationships,
        )

        extract_entities_response.additional_properties = d
        return extract_entities_response

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
