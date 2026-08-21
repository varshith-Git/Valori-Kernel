from http import HTTPStatus
from typing import Any, Optional, Union

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.api_error import ApiError
from ...models.update_metadata_response import UpdateMetadataResponse
from ...models.update_record_metadata_body import UpdateRecordMetadataBody
from ...types import UNSET, Response, Unset


def _get_kwargs(
    id: int,
    *,
    body: UpdateRecordMetadataBody,
    collection: Union[Unset, str] = UNSET,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    params: dict[str, Any] = {}

    params["collection"] = collection

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "patch",
        "url": "/v1/records/{id}/metadata".format(
            id=id,
        ),
        "params": params,
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: Union[AuthenticatedClient, Client], response: httpx.Response
) -> Optional[Union[ApiError, UpdateMetadataResponse]]:
    if response.status_code == 200:
        response_200 = UpdateMetadataResponse.from_dict(response.json())

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

    if response.status_code == 500:
        response_500 = ApiError.from_dict(response.json())

        return response_500

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: Union[AuthenticatedClient, Client], response: httpx.Response
) -> Response[Union[ApiError, UpdateMetadataResponse]]:
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
    body: UpdateRecordMetadataBody,
    collection: Union[Unset, str] = UNSET,
) -> Response[Union[ApiError, UpdateMetadataResponse]]:
    """Replace a record's metadata

     The request body replaces the stored metadata blob wholesale — this is not a merge. The vector is
    untouched. The change is committed to the BLAKE3 audit chain.

    Args:
        id (int):
        collection (Union[Unset, str]):
        body (UpdateRecordMetadataBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, UpdateMetadataResponse]]
    """

    kwargs = _get_kwargs(
        id=id,
        body=body,
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
    body: UpdateRecordMetadataBody,
    collection: Union[Unset, str] = UNSET,
) -> Optional[Union[ApiError, UpdateMetadataResponse]]:
    """Replace a record's metadata

     The request body replaces the stored metadata blob wholesale — this is not a merge. The vector is
    untouched. The change is committed to the BLAKE3 audit chain.

    Args:
        id (int):
        collection (Union[Unset, str]):
        body (UpdateRecordMetadataBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, UpdateMetadataResponse]
    """

    return sync_detailed(
        id=id,
        client=client,
        body=body,
        collection=collection,
    ).parsed


async def asyncio_detailed(
    id: int,
    *,
    client: AuthenticatedClient,
    body: UpdateRecordMetadataBody,
    collection: Union[Unset, str] = UNSET,
) -> Response[Union[ApiError, UpdateMetadataResponse]]:
    """Replace a record's metadata

     The request body replaces the stored metadata blob wholesale — this is not a merge. The vector is
    untouched. The change is committed to the BLAKE3 audit chain.

    Args:
        id (int):
        collection (Union[Unset, str]):
        body (UpdateRecordMetadataBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, UpdateMetadataResponse]]
    """

    kwargs = _get_kwargs(
        id=id,
        body=body,
        collection=collection,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    id: int,
    *,
    client: AuthenticatedClient,
    body: UpdateRecordMetadataBody,
    collection: Union[Unset, str] = UNSET,
) -> Optional[Union[ApiError, UpdateMetadataResponse]]:
    """Replace a record's metadata

     The request body replaces the stored metadata blob wholesale — this is not a merge. The vector is
    untouched. The change is committed to the BLAKE3 audit chain.

    Args:
        id (int):
        collection (Union[Unset, str]):
        body (UpdateRecordMetadataBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, UpdateMetadataResponse]
    """

    return (
        await asyncio_detailed(
            id=id,
            client=client,
            body=body,
            collection=collection,
        )
    ).parsed
