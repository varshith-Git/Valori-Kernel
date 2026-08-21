from http import HTTPStatus
from typing import Any, Optional, Union

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.api_error import ApiError
from ...models.tree_answer_result import TreeAnswerResult
from ...models.tree_query_request import TreeQueryRequest
from ...types import Response


def _get_kwargs(
    *,
    body: TreeQueryRequest,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/tree/query",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: Union[AuthenticatedClient, Client], response: httpx.Response
) -> Optional[Union[ApiError, TreeAnswerResult]]:
    if response.status_code == 200:
        response_200 = TreeAnswerResult.from_dict(response.json())

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
) -> Response[Union[ApiError, TreeAnswerResult]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient,
    body: TreeQueryRequest,
) -> Response[Union[ApiError, TreeAnswerResult]]:
    """Navigate a document tree to an answer

     Deterministic table-of-contents navigation with breadcrumb citations. Every answer carries a BLAKE3
    receipt chained onto `prev_hash`. Send either `tree` or a `cache_key` from a previous build.

    Args:
        body (TreeQueryRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, TreeAnswerResult]]
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
    body: TreeQueryRequest,
) -> Optional[Union[ApiError, TreeAnswerResult]]:
    """Navigate a document tree to an answer

     Deterministic table-of-contents navigation with breadcrumb citations. Every answer carries a BLAKE3
    receipt chained onto `prev_hash`. Send either `tree` or a `cache_key` from a previous build.

    Args:
        body (TreeQueryRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, TreeAnswerResult]
    """

    return sync_detailed(
        client=client,
        body=body,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient,
    body: TreeQueryRequest,
) -> Response[Union[ApiError, TreeAnswerResult]]:
    """Navigate a document tree to an answer

     Deterministic table-of-contents navigation with breadcrumb citations. Every answer carries a BLAKE3
    receipt chained onto `prev_hash`. Send either `tree` or a `cache_key` from a previous build.

    Args:
        body (TreeQueryRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, TreeAnswerResult]]
    """

    kwargs = _get_kwargs(
        body=body,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient,
    body: TreeQueryRequest,
) -> Optional[Union[ApiError, TreeAnswerResult]]:
    """Navigate a document tree to an answer

     Deterministic table-of-contents navigation with breadcrumb citations. Every answer carries a BLAKE3
    receipt chained onto `prev_hash`. Send either `tree` or a `cache_key` from a previous build.

    Args:
        body (TreeQueryRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, TreeAnswerResult]
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
        )
    ).parsed
