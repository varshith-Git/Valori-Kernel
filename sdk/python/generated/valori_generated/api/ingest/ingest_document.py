from http import HTTPStatus
from typing import Any, Optional, Union

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.api_error import ApiError
from ...models.ingest_accepted_response import IngestAcceptedResponse
from ...models.ingest_request import IngestRequest
from ...models.ingest_response import IngestResponse
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    body: IngestRequest,
    async_: Union[Unset, bool] = UNSET,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    params: dict[str, Any] = {}

    params["async"] = async_

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/ingest",
        "params": params,
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: Union[AuthenticatedClient, Client], response: httpx.Response
) -> Optional[Union[ApiError, IngestAcceptedResponse, IngestResponse]]:
    if response.status_code == 200:
        response_200 = IngestResponse.from_dict(response.json())

        return response_200

    if response.status_code == 202:
        response_202 = IngestAcceptedResponse.from_dict(response.json())

        return response_202

    if response.status_code == 400:
        response_400 = ApiError.from_dict(response.json())

        return response_400

    if response.status_code == 401:
        response_401 = ApiError.from_dict(response.json())

        return response_401

    if response.status_code == 403:
        response_403 = ApiError.from_dict(response.json())

        return response_403

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
) -> Response[Union[ApiError, IngestAcceptedResponse, IngestResponse]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient,
    body: IngestRequest,
    async_: Union[Unset, bool] = UNSET,
) -> Response[Union[ApiError, IngestAcceptedResponse, IngestResponse]]:
    """Chunk, embed, and insert a document

     The full pipeline in one call. Requires `VALORI_EMBED_PROVIDER`. With `async: true` the call returns
    immediately and progress is polled through `GET /v1/ingest/status/{job_id}`.

    Args:
        async_ (Union[Unset, bool]):
        body (IngestRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, IngestAcceptedResponse, IngestResponse]]
    """

    kwargs = _get_kwargs(
        body=body,
        async_=async_,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient,
    body: IngestRequest,
    async_: Union[Unset, bool] = UNSET,
) -> Optional[Union[ApiError, IngestAcceptedResponse, IngestResponse]]:
    """Chunk, embed, and insert a document

     The full pipeline in one call. Requires `VALORI_EMBED_PROVIDER`. With `async: true` the call returns
    immediately and progress is polled through `GET /v1/ingest/status/{job_id}`.

    Args:
        async_ (Union[Unset, bool]):
        body (IngestRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, IngestAcceptedResponse, IngestResponse]
    """

    return sync_detailed(
        client=client,
        body=body,
        async_=async_,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient,
    body: IngestRequest,
    async_: Union[Unset, bool] = UNSET,
) -> Response[Union[ApiError, IngestAcceptedResponse, IngestResponse]]:
    """Chunk, embed, and insert a document

     The full pipeline in one call. Requires `VALORI_EMBED_PROVIDER`. With `async: true` the call returns
    immediately and progress is polled through `GET /v1/ingest/status/{job_id}`.

    Args:
        async_ (Union[Unset, bool]):
        body (IngestRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, IngestAcceptedResponse, IngestResponse]]
    """

    kwargs = _get_kwargs(
        body=body,
        async_=async_,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient,
    body: IngestRequest,
    async_: Union[Unset, bool] = UNSET,
) -> Optional[Union[ApiError, IngestAcceptedResponse, IngestResponse]]:
    """Chunk, embed, and insert a document

     The full pipeline in one call. Requires `VALORI_EMBED_PROVIDER`. With `async: true` the call returns
    immediately and progress is polled through `GET /v1/ingest/status/{job_id}`.

    Args:
        async_ (Union[Unset, bool]):
        body (IngestRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, IngestAcceptedResponse, IngestResponse]
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
            async_=async_,
        )
    ).parsed
