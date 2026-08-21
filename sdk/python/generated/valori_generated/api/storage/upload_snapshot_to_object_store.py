from http import HTTPStatus
from typing import Any, Optional, Union

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.api_error import ApiError
from ...models.storage_snapshot_upload_response import StorageSnapshotUploadResponse
from ...types import Response


def _get_kwargs() -> dict[str, Any]:
    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/storage/snapshots/upload",
    }

    return _kwargs


def _parse_response(
    *, client: Union[AuthenticatedClient, Client], response: httpx.Response
) -> Optional[Union[ApiError, StorageSnapshotUploadResponse]]:
    if response.status_code == 200:
        response_200 = StorageSnapshotUploadResponse.from_dict(response.json())

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
) -> Response[Union[ApiError, StorageSnapshotUploadResponse]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient,
) -> Response[Union[ApiError, StorageSnapshotUploadResponse]]:
    """Offload a snapshot to the object store

     Takes a snapshot, uploads it, prunes to `VALORI_OBJECT_STORE_KEEP`, and rewrites `manifest.json` to
    name the new snapshot as current.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, StorageSnapshotUploadResponse]]
    """

    kwargs = _get_kwargs()

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient,
) -> Optional[Union[ApiError, StorageSnapshotUploadResponse]]:
    """Offload a snapshot to the object store

     Takes a snapshot, uploads it, prunes to `VALORI_OBJECT_STORE_KEEP`, and rewrites `manifest.json` to
    name the new snapshot as current.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, StorageSnapshotUploadResponse]
    """

    return sync_detailed(
        client=client,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient,
) -> Response[Union[ApiError, StorageSnapshotUploadResponse]]:
    """Offload a snapshot to the object store

     Takes a snapshot, uploads it, prunes to `VALORI_OBJECT_STORE_KEEP`, and rewrites `manifest.json` to
    name the new snapshot as current.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, StorageSnapshotUploadResponse]]
    """

    kwargs = _get_kwargs()

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient,
) -> Optional[Union[ApiError, StorageSnapshotUploadResponse]]:
    """Offload a snapshot to the object store

     Takes a snapshot, uploads it, prunes to `VALORI_OBJECT_STORE_KEEP`, and rewrites `manifest.json` to
    name the new snapshot as current.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, StorageSnapshotUploadResponse]
    """

    return (
        await asyncio_detailed(
            client=client,
        )
    ).parsed
