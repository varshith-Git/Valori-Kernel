from http import HTTPStatus
from typing import Any, Optional, Union

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.api_error import ApiError
from ...models.record_response import RecordResponse
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
        "method": "get",
        "url": "/v1/records/{id}".format(
            id=id,
        ),
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: Union[AuthenticatedClient, Client], response: httpx.Response
) -> Optional[Union[ApiError, RecordResponse]]:
    if response.status_code == 200:
        response_200 = RecordResponse.from_dict(response.json())

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
) -> Response[Union[ApiError, RecordResponse]]:
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
) -> Response[Union[ApiError, RecordResponse]]:
    """Fetch one record by id

     Returns the stored vector converted back to f32, plus whatever metadata was committed with it. The
    vector round-trips through Q16.16, so it is equal to the inserted value only to the fixed-point
    quantum.

    Args:
        id (int):
        collection (Union[Unset, str]):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, RecordResponse]]
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
) -> Optional[Union[ApiError, RecordResponse]]:
    """Fetch one record by id

     Returns the stored vector converted back to f32, plus whatever metadata was committed with it. The
    vector round-trips through Q16.16, so it is equal to the inserted value only to the fixed-point
    quantum.

    Args:
        id (int):
        collection (Union[Unset, str]):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, RecordResponse]
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
) -> Response[Union[ApiError, RecordResponse]]:
    """Fetch one record by id

     Returns the stored vector converted back to f32, plus whatever metadata was committed with it. The
    vector round-trips through Q16.16, so it is equal to the inserted value only to the fixed-point
    quantum.

    Args:
        id (int):
        collection (Union[Unset, str]):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, RecordResponse]]
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
) -> Optional[Union[ApiError, RecordResponse]]:
    """Fetch one record by id

     Returns the stored vector converted back to f32, plus whatever metadata was committed with it. The
    vector round-trips through Q16.16, so it is equal to the inserted value only to the fixed-point
    quantum.

    Args:
        id (int):
        collection (Union[Unset, str]):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, RecordResponse]
    """

    return (
        await asyncio_detailed(
            id=id,
            client=client,
            collection=collection,
        )
    ).parsed
