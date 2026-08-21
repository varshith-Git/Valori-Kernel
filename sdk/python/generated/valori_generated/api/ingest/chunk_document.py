from http import HTTPStatus
from typing import Any, Optional, Union

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.api_error import ApiError
from ...models.ingest_document_request import IngestDocumentRequest
from ...models.ingest_document_response import IngestDocumentResponse
from ...types import Response


def _get_kwargs(
    *,
    body: IngestDocumentRequest,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/ingest/document",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: Union[AuthenticatedClient, Client], response: httpx.Response
) -> Optional[Union[ApiError, IngestDocumentResponse]]:
    if response.status_code == 200:
        response_200 = IngestDocumentResponse.from_dict(response.json())

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
) -> Response[Union[ApiError, IngestDocumentResponse]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient,
    body: IngestDocumentRequest,
) -> Response[Union[ApiError, IngestDocumentResponse]]:
    """Chunk a document without embedding or storing it

     Stateless: no embedding provider is called and nothing is written. Use it to preview how a strategy
    will split a document before committing to an ingest.

    Args:
        body (IngestDocumentRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, IngestDocumentResponse]]
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
    body: IngestDocumentRequest,
) -> Optional[Union[ApiError, IngestDocumentResponse]]:
    """Chunk a document without embedding or storing it

     Stateless: no embedding provider is called and nothing is written. Use it to preview how a strategy
    will split a document before committing to an ingest.

    Args:
        body (IngestDocumentRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, IngestDocumentResponse]
    """

    return sync_detailed(
        client=client,
        body=body,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient,
    body: IngestDocumentRequest,
) -> Response[Union[ApiError, IngestDocumentResponse]]:
    """Chunk a document without embedding or storing it

     Stateless: no embedding provider is called and nothing is written. Use it to preview how a strategy
    will split a document before committing to an ingest.

    Args:
        body (IngestDocumentRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, IngestDocumentResponse]]
    """

    kwargs = _get_kwargs(
        body=body,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient,
    body: IngestDocumentRequest,
) -> Optional[Union[ApiError, IngestDocumentResponse]]:
    """Chunk a document without embedding or storing it

     Stateless: no embedding provider is called and nothing is written. Use it to preview how a strategy
    will split a document before committing to an ingest.

    Args:
        body (IngestDocumentRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, IngestDocumentResponse]
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
        )
    ).parsed
