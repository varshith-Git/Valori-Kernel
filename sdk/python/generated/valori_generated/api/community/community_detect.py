from http import HTTPStatus
from typing import Any, Optional, Union

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.api_error import ApiError
from ...models.community_detect_request import CommunityDetectRequest
from ...models.community_detect_response import CommunityDetectResponse
from ...types import Response


def _get_kwargs(
    *,
    body: CommunityDetectRequest,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/community/detect",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: Union[AuthenticatedClient, Client], response: httpx.Response
) -> Optional[Union[ApiError, CommunityDetectResponse]]:
    if response.status_code == 200:
        response_200 = CommunityDetectResponse.from_dict(response.json())

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

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: Union[AuthenticatedClient, Client], response: httpx.Response
) -> Response[Union[ApiError, CommunityDetectResponse]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient,
    body: CommunityDetectRequest,
) -> Response[Union[ApiError, CommunityDetectResponse]]:
    """Detect communities in the knowledge graph

     Label propagation, O(n+e), with a lowest-label tie-break so the result is deterministic for a given
    graph. Produces a BLAKE3 receipt over the sorted assignment. Must run before search or overview.

    Args:
        body (CommunityDetectRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, CommunityDetectResponse]]
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
    body: CommunityDetectRequest,
) -> Optional[Union[ApiError, CommunityDetectResponse]]:
    """Detect communities in the knowledge graph

     Label propagation, O(n+e), with a lowest-label tie-break so the result is deterministic for a given
    graph. Produces a BLAKE3 receipt over the sorted assignment. Must run before search or overview.

    Args:
        body (CommunityDetectRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, CommunityDetectResponse]
    """

    return sync_detailed(
        client=client,
        body=body,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient,
    body: CommunityDetectRequest,
) -> Response[Union[ApiError, CommunityDetectResponse]]:
    """Detect communities in the knowledge graph

     Label propagation, O(n+e), with a lowest-label tie-break so the result is deterministic for a given
    graph. Produces a BLAKE3 receipt over the sorted assignment. Must run before search or overview.

    Args:
        body (CommunityDetectRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, CommunityDetectResponse]]
    """

    kwargs = _get_kwargs(
        body=body,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient,
    body: CommunityDetectRequest,
) -> Optional[Union[ApiError, CommunityDetectResponse]]:
    """Detect communities in the knowledge graph

     Label propagation, O(n+e), with a lowest-label tie-break so the result is deterministic for a given
    graph. Produces a BLAKE3 receipt over the sorted assignment. Must run before search or overview.

    Args:
        body (CommunityDetectRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, CommunityDetectResponse]
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
        )
    ).parsed
