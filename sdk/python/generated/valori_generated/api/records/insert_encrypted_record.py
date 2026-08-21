from http import HTTPStatus
from typing import Any, Optional, Union

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.api_error import ApiError
from ...models.insert_encrypted_request import InsertEncryptedRequest
from ...models.insert_encrypted_response import InsertEncryptedResponse
from ...types import Response


def _get_kwargs(
    *,
    body: InsertEncryptedRequest,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/records/encrypted",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: Union[AuthenticatedClient, Client], response: httpx.Response
) -> Optional[Union[ApiError, InsertEncryptedResponse]]:
    if response.status_code == 201:
        response_201 = InsertEncryptedResponse.from_dict(response.json())

        return response_201

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
) -> Response[Union[ApiError, InsertEncryptedResponse]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient,
    body: InsertEncryptedRequest,
) -> Response[Union[ApiError, InsertEncryptedResponse]]:
    """Insert a crypto-shreddable record

     The payload is encrypted with a per-record key held in the node vault. Deleting that key through
    `DELETE /v1/crypto/shred/{key_id}` renders the record permanently unreadable without rewriting the
    audit chain.

    Args:
        body (InsertEncryptedRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, InsertEncryptedResponse]]
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
    body: InsertEncryptedRequest,
) -> Optional[Union[ApiError, InsertEncryptedResponse]]:
    """Insert a crypto-shreddable record

     The payload is encrypted with a per-record key held in the node vault. Deleting that key through
    `DELETE /v1/crypto/shred/{key_id}` renders the record permanently unreadable without rewriting the
    audit chain.

    Args:
        body (InsertEncryptedRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, InsertEncryptedResponse]
    """

    return sync_detailed(
        client=client,
        body=body,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient,
    body: InsertEncryptedRequest,
) -> Response[Union[ApiError, InsertEncryptedResponse]]:
    """Insert a crypto-shreddable record

     The payload is encrypted with a per-record key held in the node vault. Deleting that key through
    `DELETE /v1/crypto/shred/{key_id}` renders the record permanently unreadable without rewriting the
    audit chain.

    Args:
        body (InsertEncryptedRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, InsertEncryptedResponse]]
    """

    kwargs = _get_kwargs(
        body=body,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient,
    body: InsertEncryptedRequest,
) -> Optional[Union[ApiError, InsertEncryptedResponse]]:
    """Insert a crypto-shreddable record

     The payload is encrypted with a per-record key held in the node vault. Deleting that key through
    `DELETE /v1/crypto/shred/{key_id}` renders the record permanently unreadable without rewriting the
    audit chain.

    Args:
        body (InsertEncryptedRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, InsertEncryptedResponse]
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
        )
    ).parsed
