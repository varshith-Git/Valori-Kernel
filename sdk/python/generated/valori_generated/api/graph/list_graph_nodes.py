from http import HTTPStatus
from typing import Any, Optional, Union

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.api_error import ApiError
from ...models.list_nodes_response import ListNodesResponse
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    collection: Union[Unset, str] = UNSET,
    kind: Union[Unset, int] = UNSET,
    offset: Union[Unset, int] = UNSET,
    limit: Union[Unset, int] = UNSET,
) -> dict[str, Any]:
    params: dict[str, Any] = {}

    params["collection"] = collection

    params["kind"] = kind

    params["offset"] = offset

    params["limit"] = limit

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/graph/nodes",
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: Union[AuthenticatedClient, Client], response: httpx.Response
) -> Optional[Union[ApiError, ListNodesResponse]]:
    if response.status_code == 200:
        response_200 = ListNodesResponse.from_dict(response.json())

        return response_200

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
) -> Response[Union[ApiError, ListNodesResponse]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient,
    collection: Union[Unset, str] = UNSET,
    kind: Union[Unset, int] = UNSET,
    offset: Union[Unset, int] = UNSET,
    limit: Union[Unset, int] = UNSET,
) -> Response[Union[ApiError, ListNodesResponse]]:
    """List graph nodes

     `count` is the size of the filtered set before pagination; `nodes` is the page. Omitting `limit`
    returns everything.

    Args:
        collection (Union[Unset, str]):
        kind (Union[Unset, int]):
        offset (Union[Unset, int]):
        limit (Union[Unset, int]):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, ListNodesResponse]]
    """

    kwargs = _get_kwargs(
        collection=collection,
        kind=kind,
        offset=offset,
        limit=limit,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient,
    collection: Union[Unset, str] = UNSET,
    kind: Union[Unset, int] = UNSET,
    offset: Union[Unset, int] = UNSET,
    limit: Union[Unset, int] = UNSET,
) -> Optional[Union[ApiError, ListNodesResponse]]:
    """List graph nodes

     `count` is the size of the filtered set before pagination; `nodes` is the page. Omitting `limit`
    returns everything.

    Args:
        collection (Union[Unset, str]):
        kind (Union[Unset, int]):
        offset (Union[Unset, int]):
        limit (Union[Unset, int]):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, ListNodesResponse]
    """

    return sync_detailed(
        client=client,
        collection=collection,
        kind=kind,
        offset=offset,
        limit=limit,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient,
    collection: Union[Unset, str] = UNSET,
    kind: Union[Unset, int] = UNSET,
    offset: Union[Unset, int] = UNSET,
    limit: Union[Unset, int] = UNSET,
) -> Response[Union[ApiError, ListNodesResponse]]:
    """List graph nodes

     `count` is the size of the filtered set before pagination; `nodes` is the page. Omitting `limit`
    returns everything.

    Args:
        collection (Union[Unset, str]):
        kind (Union[Unset, int]):
        offset (Union[Unset, int]):
        limit (Union[Unset, int]):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, ListNodesResponse]]
    """

    kwargs = _get_kwargs(
        collection=collection,
        kind=kind,
        offset=offset,
        limit=limit,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient,
    collection: Union[Unset, str] = UNSET,
    kind: Union[Unset, int] = UNSET,
    offset: Union[Unset, int] = UNSET,
    limit: Union[Unset, int] = UNSET,
) -> Optional[Union[ApiError, ListNodesResponse]]:
    """List graph nodes

     `count` is the size of the filtered set before pagination; `nodes` is the page. Omitting `limit`
    returns everything.

    Args:
        collection (Union[Unset, str]):
        kind (Union[Unset, int]):
        offset (Union[Unset, int]):
        limit (Union[Unset, int]):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, ListNodesResponse]
    """

    return (
        await asyncio_detailed(
            client=client,
            collection=collection,
            kind=kind,
            offset=offset,
            limit=limit,
        )
    ).parsed
