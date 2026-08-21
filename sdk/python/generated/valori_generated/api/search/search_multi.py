from http import HTTPStatus
from typing import Any, Optional, Union

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.api_error import ApiError
from ...models.multi_search_request import MultiSearchRequest
from ...models.multi_search_response import MultiSearchResponse
from ...types import Response


def _get_kwargs(
    *,
    body: MultiSearchRequest,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/search/multi",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: Union[AuthenticatedClient, Client], response: httpx.Response
) -> Optional[Union[ApiError, MultiSearchResponse]]:
    if response.status_code == 200:
        response_200 = MultiSearchResponse.from_dict(response.json())

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
) -> Response[Union[ApiError, MultiSearchResponse]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient,
    body: MultiSearchRequest,
) -> Response[Union[ApiError, MultiSearchResponse]]:
    """Search several compatible collections and merge the results

     All named collections must share a dimension and metric — scores from different corpora are
    incomparable and would corrupt the merge. Collections that fail individually are reported in
    `partial_failures` rather than failing the whole request.

    Args:
        body (MultiSearchRequest): Request body for `POST /v1/search/multi`.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, MultiSearchResponse]]
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
    body: MultiSearchRequest,
) -> Optional[Union[ApiError, MultiSearchResponse]]:
    """Search several compatible collections and merge the results

     All named collections must share a dimension and metric — scores from different corpora are
    incomparable and would corrupt the merge. Collections that fail individually are reported in
    `partial_failures` rather than failing the whole request.

    Args:
        body (MultiSearchRequest): Request body for `POST /v1/search/multi`.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, MultiSearchResponse]
    """

    return sync_detailed(
        client=client,
        body=body,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient,
    body: MultiSearchRequest,
) -> Response[Union[ApiError, MultiSearchResponse]]:
    """Search several compatible collections and merge the results

     All named collections must share a dimension and metric — scores from different corpora are
    incomparable and would corrupt the merge. Collections that fail individually are reported in
    `partial_failures` rather than failing the whole request.

    Args:
        body (MultiSearchRequest): Request body for `POST /v1/search/multi`.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, MultiSearchResponse]]
    """

    kwargs = _get_kwargs(
        body=body,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient,
    body: MultiSearchRequest,
) -> Optional[Union[ApiError, MultiSearchResponse]]:
    """Search several compatible collections and merge the results

     All named collections must share a dimension and metric — scores from different corpora are
    incomparable and would corrupt the merge. Collections that fail individually are reported in
    `partial_failures` rather than failing the whole request.

    Args:
        body (MultiSearchRequest): Request body for `POST /v1/search/multi`.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, MultiSearchResponse]
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
        )
    ).parsed
