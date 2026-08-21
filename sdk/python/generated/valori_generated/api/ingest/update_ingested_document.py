from http import HTTPStatus
from typing import Any, Optional, Union

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.api_error import ApiError
from ...models.ingest_update_request import IngestUpdateRequest
from ...models.ingest_update_response import IngestUpdateResponse
from ...types import Response


def _get_kwargs(
    *,
    body: IngestUpdateRequest,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/ingest/update",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: Union[AuthenticatedClient, Client], response: httpx.Response
) -> Optional[Union[ApiError, IngestUpdateResponse]]:
    if response.status_code == 200:
        response_200 = IngestUpdateResponse.from_dict(response.json())

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

    if response.status_code == 413:
        response_413 = ApiError.from_dict(response.json())

        return response_413

    if response.status_code == 422:
        response_422 = ApiError.from_dict(response.json())

        return response_422

    if response.status_code == 500:
        response_500 = ApiError.from_dict(response.json())

        return response_500

    if response.status_code == 502:
        response_502 = ApiError.from_dict(response.json())

        return response_502

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: Union[AuthenticatedClient, Client], response: httpx.Response
) -> Response[Union[ApiError, IngestUpdateResponse]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient,
    body: IngestUpdateRequest,
) -> Response[Union[ApiError, IngestUpdateResponse]]:
    """Re-ingest a document, re-embedding only what changed

     Diffs the new chunk set against the stored one by BLAKE3 content hash. Unchanged chunks keep their
    existing records and are never re-embedded; the counts in the response say exactly what happened.

    Args:
        body (IngestUpdateRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, IngestUpdateResponse]]
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
    body: IngestUpdateRequest,
) -> Optional[Union[ApiError, IngestUpdateResponse]]:
    """Re-ingest a document, re-embedding only what changed

     Diffs the new chunk set against the stored one by BLAKE3 content hash. Unchanged chunks keep their
    existing records and are never re-embedded; the counts in the response say exactly what happened.

    Args:
        body (IngestUpdateRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, IngestUpdateResponse]
    """

    return sync_detailed(
        client=client,
        body=body,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient,
    body: IngestUpdateRequest,
) -> Response[Union[ApiError, IngestUpdateResponse]]:
    """Re-ingest a document, re-embedding only what changed

     Diffs the new chunk set against the stored one by BLAKE3 content hash. Unchanged chunks keep their
    existing records and are never re-embedded; the counts in the response say exactly what happened.

    Args:
        body (IngestUpdateRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, IngestUpdateResponse]]
    """

    kwargs = _get_kwargs(
        body=body,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient,
    body: IngestUpdateRequest,
) -> Optional[Union[ApiError, IngestUpdateResponse]]:
    """Re-ingest a document, re-embedding only what changed

     Diffs the new chunk set against the stored one by BLAKE3 content hash. Unchanged chunks keep their
    existing records and are never re-embedded; the counts in the response say exactly what happened.

    Args:
        body (IngestUpdateRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, IngestUpdateResponse]
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
        )
    ).parsed
