from http import HTTPStatus
from typing import Any, Optional, Union

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.api_error import ApiError
from ...models.insert_record_request import InsertRecordRequest
from ...models.insert_record_response import InsertRecordResponse
from ...types import Response


def _get_kwargs(
    *,
    body: InsertRecordRequest,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/records",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: Union[AuthenticatedClient, Client], response: httpx.Response
) -> Optional[Union[ApiError, InsertRecordResponse]]:
    if response.status_code == 200:
        response_200 = InsertRecordResponse.from_dict(response.json())

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

    if response.status_code == 500:
        response_500 = ApiError.from_dict(response.json())

        return response_500

    if response.status_code == 507:
        response_507 = ApiError.from_dict(response.json())

        return response_507

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: Union[AuthenticatedClient, Client], response: httpx.Response
) -> Response[Union[ApiError, InsertRecordResponse]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient,
    body: InsertRecordRequest,
) -> Response[Union[ApiError, InsertRecordResponse]]:
    """Insert a record

     Q16.16 fixed-point insert. Supplying `request_id` makes the call idempotent: a replay returns the
    original record id rather than inserting twice.

    Args:
        body (InsertRecordRequest): **The** public request body for `POST /v1/records` — one
            model, both routers.

            Phase API-2 merged the two divergent bodies that existed before:
            standalone accepted `{values, collection, text}` and silently discarded
            everything else; cluster accepted `{values, collection, metadata, tag,
            request_id}` and silently discarded `text`. Every field below is now
            honoured on **both** paths.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, InsertRecordResponse]]
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
    body: InsertRecordRequest,
) -> Optional[Union[ApiError, InsertRecordResponse]]:
    """Insert a record

     Q16.16 fixed-point insert. Supplying `request_id` makes the call idempotent: a replay returns the
    original record id rather than inserting twice.

    Args:
        body (InsertRecordRequest): **The** public request body for `POST /v1/records` — one
            model, both routers.

            Phase API-2 merged the two divergent bodies that existed before:
            standalone accepted `{values, collection, text}` and silently discarded
            everything else; cluster accepted `{values, collection, metadata, tag,
            request_id}` and silently discarded `text`. Every field below is now
            honoured on **both** paths.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, InsertRecordResponse]
    """

    return sync_detailed(
        client=client,
        body=body,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient,
    body: InsertRecordRequest,
) -> Response[Union[ApiError, InsertRecordResponse]]:
    """Insert a record

     Q16.16 fixed-point insert. Supplying `request_id` makes the call idempotent: a replay returns the
    original record id rather than inserting twice.

    Args:
        body (InsertRecordRequest): **The** public request body for `POST /v1/records` — one
            model, both routers.

            Phase API-2 merged the two divergent bodies that existed before:
            standalone accepted `{values, collection, text}` and silently discarded
            everything else; cluster accepted `{values, collection, metadata, tag,
            request_id}` and silently discarded `text`. Every field below is now
            honoured on **both** paths.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, InsertRecordResponse]]
    """

    kwargs = _get_kwargs(
        body=body,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient,
    body: InsertRecordRequest,
) -> Optional[Union[ApiError, InsertRecordResponse]]:
    """Insert a record

     Q16.16 fixed-point insert. Supplying `request_id` makes the call idempotent: a replay returns the
    original record id rather than inserting twice.

    Args:
        body (InsertRecordRequest): **The** public request body for `POST /v1/records` — one
            model, both routers.

            Phase API-2 merged the two divergent bodies that existed before:
            standalone accepted `{values, collection, text}` and silently discarded
            everything else; cluster accepted `{values, collection, metadata, tag,
            request_id}` and silently discarded `text`. Every field below is now
            honoured on **both** paths.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, InsertRecordResponse]
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
        )
    ).parsed
