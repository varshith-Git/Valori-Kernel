from http import HTTPStatus
from typing import Any, Optional, Union

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.api_error import ApiError
from ...models.delete_node_response import DeleteNodeResponse
from ...types import UNSET, Response, Unset


def _get_kwargs(
    id: int,
    *,
    collection: Union[Unset, str] = UNSET,
) -> dict[str, Any]:
    params: dict[str, Any] = {}

    params["collection"] = collection

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "delete",
        "url": "/v1/graph/node/{id}".format(
            id=id,
        ),
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: Union[AuthenticatedClient, Client], response: httpx.Response
) -> Optional[Union[ApiError, DeleteNodeResponse]]:
    if response.status_code == 200:
        response_200 = DeleteNodeResponse.from_dict(response.json())

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
) -> Response[Union[ApiError, DeleteNodeResponse]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    id: int,
    *,
    client: AuthenticatedClient,
    collection: Union[Unset, str] = UNSET,
) -> Response[Union[ApiError, DeleteNodeResponse]]:
    """Delete a graph node

     Cascades to every edge incident on the node. Committed to the audit chain.

    Args:
        id (int):
        collection (Union[Unset, str]):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, DeleteNodeResponse]]
    """

    kwargs = _get_kwargs(
        id=id,
        collection=collection,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    id: int,
    *,
    client: AuthenticatedClient,
    collection: Union[Unset, str] = UNSET,
) -> Optional[Union[ApiError, DeleteNodeResponse]]:
    """Delete a graph node

     Cascades to every edge incident on the node. Committed to the audit chain.

    Args:
        id (int):
        collection (Union[Unset, str]):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, DeleteNodeResponse]
    """

    return sync_detailed(
        id=id,
        client=client,
        collection=collection,
    ).parsed


async def asyncio_detailed(
    id: int,
    *,
    client: AuthenticatedClient,
    collection: Union[Unset, str] = UNSET,
) -> Response[Union[ApiError, DeleteNodeResponse]]:
    """Delete a graph node

     Cascades to every edge incident on the node. Committed to the audit chain.

    Args:
        id (int):
        collection (Union[Unset, str]):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, DeleteNodeResponse]]
    """

    kwargs = _get_kwargs(
        id=id,
        collection=collection,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    id: int,
    *,
    client: AuthenticatedClient,
    collection: Union[Unset, str] = UNSET,
) -> Optional[Union[ApiError, DeleteNodeResponse]]:
    """Delete a graph node

     Cascades to every edge incident on the node. Committed to the audit chain.

    Args:
        id (int):
        collection (Union[Unset, str]):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, DeleteNodeResponse]
    """

    return (
        await asyncio_detailed(
            id=id,
            client=client,
            collection=collection,
        )
    ).parsed
