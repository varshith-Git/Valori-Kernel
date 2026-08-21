from http import HTTPStatus
from typing import Any, Optional, Union

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.api_error import ApiError
from ...models.memory_consolidate_request import MemoryConsolidateRequest
from ...models.memory_consolidate_response import MemoryConsolidateResponse
from ...types import Response


def _get_kwargs(
    *,
    body: MemoryConsolidateRequest,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/memory/consolidate",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: Union[AuthenticatedClient, Client], response: httpx.Response
) -> Optional[Union[ApiError, MemoryConsolidateResponse]]:
    if response.status_code == 200:
        response_200 = MemoryConsolidateResponse.from_dict(response.json())

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

    if response.status_code == 500:
        response_500 = ApiError.from_dict(response.json())

        return response_500

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: Union[AuthenticatedClient, Client], response: httpx.Response
) -> Response[Union[ApiError, MemoryConsolidateResponse]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient,
    body: MemoryConsolidateRequest,
) -> Response[Union[ApiError, MemoryConsolidateResponse]]:
    """Replace a memory and record the supersession

     Commits three events atomically: soft-delete of the old record, insert of the new one, and a
    Supersedes edge from new to old. The returned `state_hash` covers all three.

    Args:
        body (MemoryConsolidateRequest): Replace an existing memory record with a new vector,
            committing a
            SoftDeleteRecord + AutoInsertRecord + AutoCreateEdge(Supersedes) to the
            BLAKE3 audit chain in one logical operation.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, MemoryConsolidateResponse]]
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
    body: MemoryConsolidateRequest,
) -> Optional[Union[ApiError, MemoryConsolidateResponse]]:
    """Replace a memory and record the supersession

     Commits three events atomically: soft-delete of the old record, insert of the new one, and a
    Supersedes edge from new to old. The returned `state_hash` covers all three.

    Args:
        body (MemoryConsolidateRequest): Replace an existing memory record with a new vector,
            committing a
            SoftDeleteRecord + AutoInsertRecord + AutoCreateEdge(Supersedes) to the
            BLAKE3 audit chain in one logical operation.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, MemoryConsolidateResponse]
    """

    return sync_detailed(
        client=client,
        body=body,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient,
    body: MemoryConsolidateRequest,
) -> Response[Union[ApiError, MemoryConsolidateResponse]]:
    """Replace a memory and record the supersession

     Commits three events atomically: soft-delete of the old record, insert of the new one, and a
    Supersedes edge from new to old. The returned `state_hash` covers all three.

    Args:
        body (MemoryConsolidateRequest): Replace an existing memory record with a new vector,
            committing a
            SoftDeleteRecord + AutoInsertRecord + AutoCreateEdge(Supersedes) to the
            BLAKE3 audit chain in one logical operation.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, MemoryConsolidateResponse]]
    """

    kwargs = _get_kwargs(
        body=body,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient,
    body: MemoryConsolidateRequest,
) -> Optional[Union[ApiError, MemoryConsolidateResponse]]:
    """Replace a memory and record the supersession

     Commits three events atomically: soft-delete of the old record, insert of the new one, and a
    Supersedes edge from new to old. The returned `state_hash` covers all three.

    Args:
        body (MemoryConsolidateRequest): Replace an existing memory record with a new vector,
            committing a
            SoftDeleteRecord + AutoInsertRecord + AutoCreateEdge(Supersedes) to the
            BLAKE3 audit chain in one logical operation.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, MemoryConsolidateResponse]
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
        )
    ).parsed
