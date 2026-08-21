from http import HTTPStatus
from typing import Any, Optional, Union

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.api_error import ApiError
from ...models.create_node_request import CreateNodeRequest
from ...models.create_node_response import CreateNodeResponse
from ...types import Response


def _get_kwargs(
    *,
    body: CreateNodeRequest,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/graph/node",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: Union[AuthenticatedClient, Client], response: httpx.Response
) -> Optional[Union[ApiError, CreateNodeResponse]]:
    if response.status_code == 200:
        response_200 = CreateNodeResponse.from_dict(response.json())

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

    if response.status_code == 500:
        response_500 = ApiError.from_dict(response.json())

        return response_500

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: Union[AuthenticatedClient, Client], response: httpx.Response
) -> Response[Union[ApiError, CreateNodeResponse]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient,
    body: CreateNodeRequest,
) -> Response[Union[ApiError, CreateNodeResponse]]:
    """Create a graph node

     `kind` is the numeric NodeKind discriminant (0=Document, 1=Chunk, 2=Concept, …). `record_id`
    optionally binds the node to a stored vector.

    Args:
        body (CreateNodeRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, CreateNodeResponse]]
    """

    kwargs = _get_kwargs(
        body=body,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient,
    body: CreateNodeRequest,
) -> Optional[Union[ApiError, CreateNodeResponse]]:
    """Create a graph node

     `kind` is the numeric NodeKind discriminant (0=Document, 1=Chunk, 2=Concept, …). `record_id`
    optionally binds the node to a stored vector.

    Args:
        body (CreateNodeRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, CreateNodeResponse]
    """

    return sync_detailed(
        client=client,
        body=body,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient,
    body: CreateNodeRequest,
) -> Response[Union[ApiError, CreateNodeResponse]]:
    """Create a graph node

     `kind` is the numeric NodeKind discriminant (0=Document, 1=Chunk, 2=Concept, …). `record_id`
    optionally binds the node to a stored vector.

    Args:
        body (CreateNodeRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, CreateNodeResponse]]
    """

    kwargs = _get_kwargs(
        body=body,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient,
    body: CreateNodeRequest,
) -> Optional[Union[ApiError, CreateNodeResponse]]:
    """Create a graph node

     `kind` is the numeric NodeKind discriminant (0=Document, 1=Chunk, 2=Concept, …). `record_id`
    optionally binds the node to a stored vector.

    Args:
        body (CreateNodeRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, CreateNodeResponse]
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
        )
    ).parsed
