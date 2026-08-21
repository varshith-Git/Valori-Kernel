from http import HTTPStatus
from typing import Any, Optional, Union

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.api_error import ApiError
from ...models.memory_search_response import MemorySearchResponse
from ...models.memory_search_vector_request import MemorySearchVectorRequest
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    body: MemorySearchVectorRequest,
    explain: Union[Unset, bool] = UNSET,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    params: dict[str, Any] = {}

    params["explain"] = explain

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/memory/search",
        "params": params,
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: Union[AuthenticatedClient, Client], response: httpx.Response
) -> Optional[Union[ApiError, MemorySearchResponse]]:
    if response.status_code == 200:
        response_200 = MemorySearchResponse.from_dict(response.json())

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
) -> Response[Union[ApiError, MemorySearchResponse]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient,
    body: MemorySearchVectorRequest,
    explain: Union[Unset, bool] = UNSET,
) -> Response[Union[ApiError, MemorySearchResponse]]:
    """Recall agent memories

     Vector recall with optional recency decay, metadata filtering, and hybrid term re-ranking. When
    `decay_half_life_secs` is set, each hit also carries `decay_factor` and `age_secs`; `score` remains
    the true distance. Add `?explain=true` for an `_execution` block describing the plan that ran.

    Args:
        explain (Union[Unset, bool]):
        body (MemorySearchVectorRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, MemorySearchResponse]]
    """

    kwargs = _get_kwargs(
        body=body,
        explain=explain,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient,
    body: MemorySearchVectorRequest,
    explain: Union[Unset, bool] = UNSET,
) -> Optional[Union[ApiError, MemorySearchResponse]]:
    """Recall agent memories

     Vector recall with optional recency decay, metadata filtering, and hybrid term re-ranking. When
    `decay_half_life_secs` is set, each hit also carries `decay_factor` and `age_secs`; `score` remains
    the true distance. Add `?explain=true` for an `_execution` block describing the plan that ran.

    Args:
        explain (Union[Unset, bool]):
        body (MemorySearchVectorRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, MemorySearchResponse]
    """

    return sync_detailed(
        client=client,
        body=body,
        explain=explain,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient,
    body: MemorySearchVectorRequest,
    explain: Union[Unset, bool] = UNSET,
) -> Response[Union[ApiError, MemorySearchResponse]]:
    """Recall agent memories

     Vector recall with optional recency decay, metadata filtering, and hybrid term re-ranking. When
    `decay_half_life_secs` is set, each hit also carries `decay_factor` and `age_secs`; `score` remains
    the true distance. Add `?explain=true` for an `_execution` block describing the plan that ran.

    Args:
        explain (Union[Unset, bool]):
        body (MemorySearchVectorRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, MemorySearchResponse]]
    """

    kwargs = _get_kwargs(
        body=body,
        explain=explain,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient,
    body: MemorySearchVectorRequest,
    explain: Union[Unset, bool] = UNSET,
) -> Optional[Union[ApiError, MemorySearchResponse]]:
    """Recall agent memories

     Vector recall with optional recency decay, metadata filtering, and hybrid term re-ranking. When
    `decay_half_life_secs` is set, each hit also carries `decay_factor` and `age_secs`; `score` remains
    the true distance. Add `?explain=true` for an `_execution` block describing the plan that ran.

    Args:
        explain (Union[Unset, bool]):
        body (MemorySearchVectorRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, MemorySearchResponse]
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
            explain=explain,
        )
    ).parsed
