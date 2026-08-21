from http import HTTPStatus
from typing import Any, Optional, Union

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.api_error import ApiError
from ...models.timeline_response import TimelineResponse
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    from_: Union[Unset, str] = UNSET,
    to: Union[Unset, str] = UNSET,
    limit: Union[Unset, int] = UNSET,
    collection: Union[Unset, str] = UNSET,
) -> dict[str, Any]:
    params: dict[str, Any] = {}

    params["from"] = from_

    params["to"] = to

    params["limit"] = limit

    params["collection"] = collection

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/timeline",
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: Union[AuthenticatedClient, Client], response: httpx.Response
) -> Optional[Union[ApiError, TimelineResponse]]:
    if response.status_code == 200:
        response_200 = TimelineResponse.from_dict(response.json())

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
) -> Response[Union[ApiError, TimelineResponse]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient,
    from_: Union[Unset, str] = UNSET,
    to: Union[Unset, str] = UNSET,
    limit: Union[Unset, int] = UNSET,
    collection: Union[Unset, str] = UNSET,
) -> Response[Union[ApiError, TimelineResponse]]:
    """Committed events in chronological order

     Reads the event log directly, so it reflects committed state only. Known limitation: with
    `VALORI_SHARD_COUNT > 1` this reads shard 0's log.

    Args:
        from_ (Union[Unset, str]):
        to (Union[Unset, str]):
        limit (Union[Unset, int]):
        collection (Union[Unset, str]):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, TimelineResponse]]
    """

    kwargs = _get_kwargs(
        from_=from_,
        to=to,
        limit=limit,
        collection=collection,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient,
    from_: Union[Unset, str] = UNSET,
    to: Union[Unset, str] = UNSET,
    limit: Union[Unset, int] = UNSET,
    collection: Union[Unset, str] = UNSET,
) -> Optional[Union[ApiError, TimelineResponse]]:
    """Committed events in chronological order

     Reads the event log directly, so it reflects committed state only. Known limitation: with
    `VALORI_SHARD_COUNT > 1` this reads shard 0's log.

    Args:
        from_ (Union[Unset, str]):
        to (Union[Unset, str]):
        limit (Union[Unset, int]):
        collection (Union[Unset, str]):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, TimelineResponse]
    """

    return sync_detailed(
        client=client,
        from_=from_,
        to=to,
        limit=limit,
        collection=collection,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient,
    from_: Union[Unset, str] = UNSET,
    to: Union[Unset, str] = UNSET,
    limit: Union[Unset, int] = UNSET,
    collection: Union[Unset, str] = UNSET,
) -> Response[Union[ApiError, TimelineResponse]]:
    """Committed events in chronological order

     Reads the event log directly, so it reflects committed state only. Known limitation: with
    `VALORI_SHARD_COUNT > 1` this reads shard 0's log.

    Args:
        from_ (Union[Unset, str]):
        to (Union[Unset, str]):
        limit (Union[Unset, int]):
        collection (Union[Unset, str]):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, TimelineResponse]]
    """

    kwargs = _get_kwargs(
        from_=from_,
        to=to,
        limit=limit,
        collection=collection,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient,
    from_: Union[Unset, str] = UNSET,
    to: Union[Unset, str] = UNSET,
    limit: Union[Unset, int] = UNSET,
    collection: Union[Unset, str] = UNSET,
) -> Optional[Union[ApiError, TimelineResponse]]:
    """Committed events in chronological order

     Reads the event log directly, so it reflects committed state only. Known limitation: with
    `VALORI_SHARD_COUNT > 1` this reads shard 0's log.

    Args:
        from_ (Union[Unset, str]):
        to (Union[Unset, str]):
        limit (Union[Unset, int]):
        collection (Union[Unset, str]):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, TimelineResponse]
    """

    return (
        await asyncio_detailed(
            client=client,
            from_=from_,
            to=to,
            limit=limit,
            collection=collection,
        )
    ).parsed
