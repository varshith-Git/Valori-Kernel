from http import HTTPStatus
from typing import Any, Optional, Union

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.api_error import ApiError
from ...models.subgraph_response import SubgraphResponse
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    root: int,
    depth: Union[Unset, int] = UNSET,
    collection: Union[Unset, str] = UNSET,
) -> dict[str, Any]:
    params: dict[str, Any] = {}

    params["root"] = root

    params["depth"] = depth

    params["collection"] = collection

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/graph/subgraph",
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: Union[AuthenticatedClient, Client], response: httpx.Response
) -> Optional[Union[ApiError, SubgraphResponse]]:
    if response.status_code == 200:
        response_200 = SubgraphResponse.from_dict(response.json())

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
) -> Response[Union[ApiError, SubgraphResponse]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient,
    root: int,
    depth: Union[Unset, int] = UNSET,
    collection: Union[Unset, str] = UNSET,
) -> Response[Union[ApiError, SubgraphResponse]]:
    """Expand a subgraph around a root node

     Breadth-first expansion bounded by `depth`. Traversal never crosses a collection boundary.

    Args:
        root (int):
        depth (Union[Unset, int]):
        collection (Union[Unset, str]):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, SubgraphResponse]]
    """

    kwargs = _get_kwargs(
        root=root,
        depth=depth,
        collection=collection,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient,
    root: int,
    depth: Union[Unset, int] = UNSET,
    collection: Union[Unset, str] = UNSET,
) -> Optional[Union[ApiError, SubgraphResponse]]:
    """Expand a subgraph around a root node

     Breadth-first expansion bounded by `depth`. Traversal never crosses a collection boundary.

    Args:
        root (int):
        depth (Union[Unset, int]):
        collection (Union[Unset, str]):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, SubgraphResponse]
    """

    return sync_detailed(
        client=client,
        root=root,
        depth=depth,
        collection=collection,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient,
    root: int,
    depth: Union[Unset, int] = UNSET,
    collection: Union[Unset, str] = UNSET,
) -> Response[Union[ApiError, SubgraphResponse]]:
    """Expand a subgraph around a root node

     Breadth-first expansion bounded by `depth`. Traversal never crosses a collection boundary.

    Args:
        root (int):
        depth (Union[Unset, int]):
        collection (Union[Unset, str]):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, SubgraphResponse]]
    """

    kwargs = _get_kwargs(
        root=root,
        depth=depth,
        collection=collection,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient,
    root: int,
    depth: Union[Unset, int] = UNSET,
    collection: Union[Unset, str] = UNSET,
) -> Optional[Union[ApiError, SubgraphResponse]]:
    """Expand a subgraph around a root node

     Breadth-first expansion bounded by `depth`. Traversal never crosses a collection boundary.

    Args:
        root (int):
        depth (Union[Unset, int]):
        collection (Union[Unset, str]):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, SubgraphResponse]
    """

    return (
        await asyncio_detailed(
            client=client,
            root=root,
            depth=depth,
            collection=collection,
        )
    ).parsed
