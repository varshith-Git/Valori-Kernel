from http import HTTPStatus
from typing import Any, Optional, Union

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.api_error import ApiError
from ...models.restore_from_store_request import RestoreFromStoreRequest
from ...models.restore_from_store_response import RestoreFromStoreResponse
from ...types import Response


def _get_kwargs(
    *,
    body: RestoreFromStoreRequest,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/storage/snapshots/restore",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: Union[AuthenticatedClient, Client], response: httpx.Response
) -> Optional[Union[ApiError, RestoreFromStoreResponse]]:
    if response.status_code == 200:
        response_200 = RestoreFromStoreResponse.from_dict(response.json())

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

    if response.status_code == 502:
        response_502 = ApiError.from_dict(response.json())

        return response_502

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: Union[AuthenticatedClient, Client], response: httpx.Response
) -> Response[Union[ApiError, RestoreFromStoreResponse]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient,
    body: RestoreFromStoreRequest,
) -> Response[Union[ApiError, RestoreFromStoreResponse]]:
    """Restore from a snapshot in the object store

     Omit `key` to restore whatever `manifest.json` names as current — the recommended disaster-recovery
    entry point. Destructive.

    Args:
        body (RestoreFromStoreRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, RestoreFromStoreResponse]]
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
    body: RestoreFromStoreRequest,
) -> Optional[Union[ApiError, RestoreFromStoreResponse]]:
    """Restore from a snapshot in the object store

     Omit `key` to restore whatever `manifest.json` names as current — the recommended disaster-recovery
    entry point. Destructive.

    Args:
        body (RestoreFromStoreRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, RestoreFromStoreResponse]
    """

    return sync_detailed(
        client=client,
        body=body,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient,
    body: RestoreFromStoreRequest,
) -> Response[Union[ApiError, RestoreFromStoreResponse]]:
    """Restore from a snapshot in the object store

     Omit `key` to restore whatever `manifest.json` names as current — the recommended disaster-recovery
    entry point. Destructive.

    Args:
        body (RestoreFromStoreRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, RestoreFromStoreResponse]]
    """

    kwargs = _get_kwargs(
        body=body,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient,
    body: RestoreFromStoreRequest,
) -> Optional[Union[ApiError, RestoreFromStoreResponse]]:
    """Restore from a snapshot in the object store

     Omit `key` to restore whatever `manifest.json` names as current — the recommended disaster-recovery
    entry point. Destructive.

    Args:
        body (RestoreFromStoreRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, RestoreFromStoreResponse]
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
        )
    ).parsed
