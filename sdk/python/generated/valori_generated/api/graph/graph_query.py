from http import HTTPStatus
from typing import Any, Optional, Union

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.api_error import ApiError
from ...models.graph_query_response import GraphQueryResponse
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    start: int,
    direction: Union[Unset, str] = UNSET,
    edge_kind: Union[Unset, int] = UNSET,
    node_kind: Union[Unset, int] = UNSET,
    depth: Union[Unset, int] = UNSET,
    limit: Union[Unset, int] = UNSET,
    collection: Union[Unset, str] = UNSET,
) -> dict[str, Any]:
    params: dict[str, Any] = {}

    params["start"] = start

    params["direction"] = direction

    params["edge_kind"] = edge_kind

    params["node_kind"] = node_kind

    params["depth"] = depth

    params["limit"] = limit

    params["collection"] = collection

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/graph/query",
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: Union[AuthenticatedClient, Client], response: httpx.Response
) -> Optional[Union[ApiError, GraphQueryResponse]]:
    if response.status_code == 200:
        response_200 = GraphQueryResponse.from_dict(response.json())

        return response_200

    if response.status_code == 400:
        response_400 = ApiError.from_dict(response.json())

        return response_400

    if response.status_code == 401:
        response_401 = ApiError.from_dict(response.json())

        return response_401

    if response.status_code == 403:
        response_403 = ApiError.from_dict(response.json())

        return response_403

    if response.status_code == 404:
        response_404 = ApiError.from_dict(response.json())

        return response_404

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: Union[AuthenticatedClient, Client], response: httpx.Response
) -> Response[Union[ApiError, GraphQueryResponse]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient,
    start: int,
    direction: Union[Unset, str] = UNSET,
    edge_kind: Union[Unset, int] = UNSET,
    node_kind: Union[Unset, int] = UNSET,
    depth: Union[Unset, int] = UNSET,
    limit: Union[Unset, int] = UNSET,
    collection: Union[Unset, str] = UNSET,
) -> Response[Union[ApiError, GraphQueryResponse]]:
    """Deterministic bounded graph traversal

     Walks the graph from `start` with optional edge-kind and node-kind filters. Result order is
    deterministic for a given kernel state.

    Args:
        start (int):
        direction (Union[Unset, str]):
        edge_kind (Union[Unset, int]):
        node_kind (Union[Unset, int]):
        depth (Union[Unset, int]):
        limit (Union[Unset, int]):
        collection (Union[Unset, str]):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, GraphQueryResponse]]
    """

    kwargs = _get_kwargs(
        start=start,
        direction=direction,
        edge_kind=edge_kind,
        node_kind=node_kind,
        depth=depth,
        limit=limit,
        collection=collection,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient,
    start: int,
    direction: Union[Unset, str] = UNSET,
    edge_kind: Union[Unset, int] = UNSET,
    node_kind: Union[Unset, int] = UNSET,
    depth: Union[Unset, int] = UNSET,
    limit: Union[Unset, int] = UNSET,
    collection: Union[Unset, str] = UNSET,
) -> Optional[Union[ApiError, GraphQueryResponse]]:
    """Deterministic bounded graph traversal

     Walks the graph from `start` with optional edge-kind and node-kind filters. Result order is
    deterministic for a given kernel state.

    Args:
        start (int):
        direction (Union[Unset, str]):
        edge_kind (Union[Unset, int]):
        node_kind (Union[Unset, int]):
        depth (Union[Unset, int]):
        limit (Union[Unset, int]):
        collection (Union[Unset, str]):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, GraphQueryResponse]
    """

    return sync_detailed(
        client=client,
        start=start,
        direction=direction,
        edge_kind=edge_kind,
        node_kind=node_kind,
        depth=depth,
        limit=limit,
        collection=collection,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient,
    start: int,
    direction: Union[Unset, str] = UNSET,
    edge_kind: Union[Unset, int] = UNSET,
    node_kind: Union[Unset, int] = UNSET,
    depth: Union[Unset, int] = UNSET,
    limit: Union[Unset, int] = UNSET,
    collection: Union[Unset, str] = UNSET,
) -> Response[Union[ApiError, GraphQueryResponse]]:
    """Deterministic bounded graph traversal

     Walks the graph from `start` with optional edge-kind and node-kind filters. Result order is
    deterministic for a given kernel state.

    Args:
        start (int):
        direction (Union[Unset, str]):
        edge_kind (Union[Unset, int]):
        node_kind (Union[Unset, int]):
        depth (Union[Unset, int]):
        limit (Union[Unset, int]):
        collection (Union[Unset, str]):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, GraphQueryResponse]]
    """

    kwargs = _get_kwargs(
        start=start,
        direction=direction,
        edge_kind=edge_kind,
        node_kind=node_kind,
        depth=depth,
        limit=limit,
        collection=collection,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient,
    start: int,
    direction: Union[Unset, str] = UNSET,
    edge_kind: Union[Unset, int] = UNSET,
    node_kind: Union[Unset, int] = UNSET,
    depth: Union[Unset, int] = UNSET,
    limit: Union[Unset, int] = UNSET,
    collection: Union[Unset, str] = UNSET,
) -> Optional[Union[ApiError, GraphQueryResponse]]:
    """Deterministic bounded graph traversal

     Walks the graph from `start` with optional edge-kind and node-kind filters. Result order is
    deterministic for a given kernel state.

    Args:
        start (int):
        direction (Union[Unset, str]):
        edge_kind (Union[Unset, int]):
        node_kind (Union[Unset, int]):
        depth (Union[Unset, int]):
        limit (Union[Unset, int]):
        collection (Union[Unset, str]):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, GraphQueryResponse]
    """

    return (
        await asyncio_detailed(
            client=client,
            start=start,
            direction=direction,
            edge_kind=edge_kind,
            node_kind=node_kind,
            depth=depth,
            limit=limit,
            collection=collection,
        )
    ).parsed
