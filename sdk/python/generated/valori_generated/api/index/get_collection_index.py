from http import HTTPStatus
from typing import Any, Optional, Union

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.api_error import ApiError
from ...models.index_status_response import IndexStatusResponse
from ...types import Response


def _get_kwargs(
    name: str,
) -> dict[str, Any]:
    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/namespaces/{name}/index".format(
            name=name,
        ),
    }

    return _kwargs


def _parse_response(
    *, client: Union[AuthenticatedClient, Client], response: httpx.Response
) -> Optional[Union[ApiError, IndexStatusResponse]]:
    if response.status_code == 200:
        response_200 = IndexStatusResponse.from_dict(response.json())

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
) -> Response[Union[ApiError, IndexStatusResponse]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    name: str,
    *,
    client: AuthenticatedClient,
) -> Response[Union[ApiError, IndexStatusResponse]]:
    """Read one collection's index lifecycle state

     `desired_type` is what was asked for; `active_type` and `status` describe what this node is actually
    serving. In cluster mode `desired_type` comes from the Raft-replicated spec and is cluster-wide,
    while activation is node-local — the two differ while a build propagates.

    Args:
        name (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, IndexStatusResponse]]
    """

    kwargs = _get_kwargs(
        name=name,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    name: str,
    *,
    client: AuthenticatedClient,
) -> Optional[Union[ApiError, IndexStatusResponse]]:
    """Read one collection's index lifecycle state

     `desired_type` is what was asked for; `active_type` and `status` describe what this node is actually
    serving. In cluster mode `desired_type` comes from the Raft-replicated spec and is cluster-wide,
    while activation is node-local — the two differ while a build propagates.

    Args:
        name (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, IndexStatusResponse]
    """

    return sync_detailed(
        name=name,
        client=client,
    ).parsed


async def asyncio_detailed(
    name: str,
    *,
    client: AuthenticatedClient,
) -> Response[Union[ApiError, IndexStatusResponse]]:
    """Read one collection's index lifecycle state

     `desired_type` is what was asked for; `active_type` and `status` describe what this node is actually
    serving. In cluster mode `desired_type` comes from the Raft-replicated spec and is cluster-wide,
    while activation is node-local — the two differ while a build propagates.

    Args:
        name (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, IndexStatusResponse]]
    """

    kwargs = _get_kwargs(
        name=name,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    name: str,
    *,
    client: AuthenticatedClient,
) -> Optional[Union[ApiError, IndexStatusResponse]]:
    """Read one collection's index lifecycle state

     `desired_type` is what was asked for; `active_type` and `status` describe what this node is actually
    serving. In cluster mode `desired_type` comes from the Raft-replicated spec and is cluster-wide,
    while activation is node-local — the two differ while a build propagates.

    Args:
        name (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, IndexStatusResponse]
    """

    return (
        await asyncio_detailed(
            name=name,
            client=client,
        )
    ).parsed
