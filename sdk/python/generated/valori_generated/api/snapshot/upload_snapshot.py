from http import HTTPStatus
from typing import Any, Optional, Union, cast

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.api_error import ApiError
from ...types import File, Response


def _get_kwargs(
    *,
    body: File,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/snapshot/upload",
    }

    _kwargs["content"] = body.payload

    headers["Content-Type"] = "application/octet-stream"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: Union[AuthenticatedClient, Client], response: httpx.Response
) -> Optional[Union[Any, ApiError]]:
    if response.status_code == 200:
        response_200 = cast(Any, None)
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
) -> Response[Union[Any, ApiError]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient,
    body: File,
) -> Response[Union[Any, ApiError]]:
    """Restore state from an uploaded snapshot

     Replaces the entire in-memory state with the uploaded snapshot and rebuilds the state hash from
    scratch. Destructive.

    Args:
        body (File): Raw snapshot bytes, as the OpenAPI binary idiom.

            Phase API-3.3: `/v1/snapshot/download` and `/v1/snapshot/upload` were
            annotated `body = Vec<u8>`, which utoipa renders literally — `type: array,
            items: {type: integer, format: int32}`. Generators believe it: the
            throwaway Python client typed the download as `list[int]`, so restoring a
            snapshot meant round-tripping every byte of a multi-megabyte file through
            a Python integer list.

            `type: string, format: binary` is the OpenAPI idiom for an opaque byte
            stream, and generators map it to `bytes` / `Blob` / `File`. The wire format
            is unchanged — this describes the same octet-stream correctly.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[Any, ApiError]]
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
    body: File,
) -> Optional[Union[Any, ApiError]]:
    """Restore state from an uploaded snapshot

     Replaces the entire in-memory state with the uploaded snapshot and rebuilds the state hash from
    scratch. Destructive.

    Args:
        body (File): Raw snapshot bytes, as the OpenAPI binary idiom.

            Phase API-3.3: `/v1/snapshot/download` and `/v1/snapshot/upload` were
            annotated `body = Vec<u8>`, which utoipa renders literally — `type: array,
            items: {type: integer, format: int32}`. Generators believe it: the
            throwaway Python client typed the download as `list[int]`, so restoring a
            snapshot meant round-tripping every byte of a multi-megabyte file through
            a Python integer list.

            `type: string, format: binary` is the OpenAPI idiom for an opaque byte
            stream, and generators map it to `bytes` / `Blob` / `File`. The wire format
            is unchanged — this describes the same octet-stream correctly.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[Any, ApiError]
    """

    return sync_detailed(
        client=client,
        body=body,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient,
    body: File,
) -> Response[Union[Any, ApiError]]:
    """Restore state from an uploaded snapshot

     Replaces the entire in-memory state with the uploaded snapshot and rebuilds the state hash from
    scratch. Destructive.

    Args:
        body (File): Raw snapshot bytes, as the OpenAPI binary idiom.

            Phase API-3.3: `/v1/snapshot/download` and `/v1/snapshot/upload` were
            annotated `body = Vec<u8>`, which utoipa renders literally — `type: array,
            items: {type: integer, format: int32}`. Generators believe it: the
            throwaway Python client typed the download as `list[int]`, so restoring a
            snapshot meant round-tripping every byte of a multi-megabyte file through
            a Python integer list.

            `type: string, format: binary` is the OpenAPI idiom for an opaque byte
            stream, and generators map it to `bytes` / `Blob` / `File`. The wire format
            is unchanged — this describes the same octet-stream correctly.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[Any, ApiError]]
    """

    kwargs = _get_kwargs(
        body=body,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient,
    body: File,
) -> Optional[Union[Any, ApiError]]:
    """Restore state from an uploaded snapshot

     Replaces the entire in-memory state with the uploaded snapshot and rebuilds the state hash from
    scratch. Destructive.

    Args:
        body (File): Raw snapshot bytes, as the OpenAPI binary idiom.

            Phase API-3.3: `/v1/snapshot/download` and `/v1/snapshot/upload` were
            annotated `body = Vec<u8>`, which utoipa renders literally — `type: array,
            items: {type: integer, format: int32}`. Generators believe it: the
            throwaway Python client typed the download as `list[int]`, so restoring a
            snapshot meant round-tripping every byte of a multi-megabyte file through
            a Python integer list.

            `type: string, format: binary` is the OpenAPI idiom for an opaque byte
            stream, and generators map it to `bytes` / `Blob` / `File`. The wire format
            is unchanged — this describes the same octet-stream correctly.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[Any, ApiError]
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
        )
    ).parsed
