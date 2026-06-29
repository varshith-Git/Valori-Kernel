# Backward-compat shim — moved to valoricore.integrations
from valoricore.integrations.llamaindex import ValoricoreLlamaIndex
from valoricore.integrations.llamaindex import ValoricoreLlamaIndex as ValoricoreVectorStore

__all__ = ["ValoricoreLlamaIndex", "ValoricoreVectorStore"]
