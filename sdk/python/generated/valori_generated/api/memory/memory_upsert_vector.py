from http import HTTPStatus
from typing import Any, Optional, Union

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.api_error import ApiError
from ...models.memory_upsert_response import MemoryUpsertResponse
from ...models.memory_upsert_vector_request import MemoryUpsertVectorRequest
from ...types import Response


def _get_kwargs(
    *,
    body: MemoryUpsertVectorRequest,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/memory/upsert_vector",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: Union[AuthenticatedClient, Client], response: httpx.Response
) -> Optional[Union[ApiError, MemoryUpsertResponse]]:
    if response.status_code == 200:
        response_200 = MemoryUpsertResponse.from_dict(response.json())

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
) -> Response[Union[ApiError, MemoryUpsertResponse]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient,
    body: MemoryUpsertVectorRequest,
) -> Response[Union[ApiError, MemoryUpsertResponse]]:
    """Store an agent memory (SDK path)

     Identical to `POST /v1/memory/upsert`. This is the path the Python SDK has always used; both are
    supported and neither is deprecated.

    Args:
        body (MemoryUpsertVectorRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, MemoryUpsertResponse]]
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
    body: MemoryUpsertVectorRequest,
) -> Optional[Union[ApiError, MemoryUpsertResponse]]:
    """Store an agent memory (SDK path)

     Identical to `POST /v1/memory/upsert`. This is the path the Python SDK has always used; both are
    supported and neither is deprecated.

    Args:
        body (MemoryUpsertVectorRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, MemoryUpsertResponse]
    """

    return sync_detailed(
        client=client,
        body=body,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient,
    body: MemoryUpsertVectorRequest,
) -> Response[Union[ApiError, MemoryUpsertResponse]]:
    """Store an agent memory (SDK path)

     Identical to `POST /v1/memory/upsert`. This is the path the Python SDK has always used; both are
    supported and neither is deprecated.

    Args:
        body (MemoryUpsertVectorRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, MemoryUpsertResponse]]
    """

    kwargs = _get_kwargs(
        body=body,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient,
    body: MemoryUpsertVectorRequest,
) -> Optional[Union[ApiError, MemoryUpsertResponse]]:
    """Store an agent memory (SDK path)

     Identical to `POST /v1/memory/upsert`. This is the path the Python SDK has always used; both are
    supported and neither is deprecated.

    Args:
        body (MemoryUpsertVectorRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, MemoryUpsertResponse]
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
        )
    ).parsed
