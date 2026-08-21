from http import HTTPStatus
from typing import Any, Optional, Union

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.api_error import ApiError
from ...models.crypto_status_response import CryptoStatusResponse
from ...types import Response


def _get_kwargs(
    key_id: str,
) -> dict[str, Any]:
    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/crypto/status/{key_id}".format(
            key_id=key_id,
        ),
    }

    return _kwargs


def _parse_response(
    *, client: Union[AuthenticatedClient, Client], response: httpx.Response
) -> Optional[Union[ApiError, CryptoStatusResponse]]:
    if response.status_code == 200:
        response_200 = CryptoStatusResponse.from_dict(response.json())

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
) -> Response[Union[ApiError, CryptoStatusResponse]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    key_id: str,
    *,
    client: AuthenticatedClient,
) -> Response[Union[ApiError, CryptoStatusResponse]]:
    """Check whether a crypto-shredding key still exists

     `exists: false` means the key was shredded and every record encrypted under it is permanently
    unreadable.

    Args:
        key_id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, CryptoStatusResponse]]
    """

    kwargs = _get_kwargs(
        key_id=key_id,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    key_id: str,
    *,
    client: AuthenticatedClient,
) -> Optional[Union[ApiError, CryptoStatusResponse]]:
    """Check whether a crypto-shredding key still exists

     `exists: false` means the key was shredded and every record encrypted under it is permanently
    unreadable.

    Args:
        key_id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, CryptoStatusResponse]
    """

    return sync_detailed(
        key_id=key_id,
        client=client,
    ).parsed


async def asyncio_detailed(
    key_id: str,
    *,
    client: AuthenticatedClient,
) -> Response[Union[ApiError, CryptoStatusResponse]]:
    """Check whether a crypto-shredding key still exists

     `exists: false` means the key was shredded and every record encrypted under it is permanently
    unreadable.

    Args:
        key_id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, CryptoStatusResponse]]
    """

    kwargs = _get_kwargs(
        key_id=key_id,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    key_id: str,
    *,
    client: AuthenticatedClient,
) -> Optional[Union[ApiError, CryptoStatusResponse]]:
    """Check whether a crypto-shredding key still exists

     `exists: false` means the key was shredded and every record encrypted under it is permanently
    unreadable.

    Args:
        key_id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, CryptoStatusResponse]
    """

    return (
        await asyncio_detailed(
            key_id=key_id,
            client=client,
        )
    ).parsed
