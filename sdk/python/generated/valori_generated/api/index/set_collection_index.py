from http import HTTPStatus
from typing import Any, Optional, Union

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.api_error import ApiError
from ...models.index_build_request import IndexBuildRequest
from ...models.index_status_response import IndexStatusResponse
from ...types import Response


def _get_kwargs(
    name: str,
    *,
    body: IndexBuildRequest,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/namespaces/{name}/index".format(
            name=name,
        ),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: Union[AuthenticatedClient, Client], response: httpx.Response
) -> Optional[Union[ApiError, IndexStatusResponse]]:
    if response.status_code == 200:
        response_200 = IndexStatusResponse.from_dict(response.json())

        return response_200

    if response.status_code == 202:
        response_202 = IndexStatusResponse.from_dict(response.json())

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

    if response.status_code == 404:
        response_404 = ApiError.from_dict(response.json())

        return response_404

    if response.status_code == 409:
        response_409 = ApiError.from_dict(response.json())

        return response_409

    if response.status_code == 501:
        response_501 = ApiError.from_dict(response.json())

        return response_501

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: Union[AuthenticatedClient, Client], response: httpx.Response
) -> Response[Union[ApiError, IndexStatusResponse]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    name: str,
    *,
    client: AuthenticatedClient,
    body: IndexBuildRequest,
) -> Response[Union[ApiError, IndexStatusResponse]]:
    """Create, change, or drop a collection index

     `type` is `hnsw`, `ivf`, `bq`, or null to drop the index and revert to exact search. A build is
    asynchronous: 202 means the build started, and the response carries the building generation. Poll
    the GET form for completion.

    Args:
        name (str):
        body (IndexBuildRequest): The request body for `POST /v1/namespaces/{name}/index`.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, IndexStatusResponse]]
    """

    kwargs = _get_kwargs(
        name=name,
        body=body,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    name: str,
    *,
    client: AuthenticatedClient,
    body: IndexBuildRequest,
) -> Optional[Union[ApiError, IndexStatusResponse]]:
    """Create, change, or drop a collection index

     `type` is `hnsw`, `ivf`, `bq`, or null to drop the index and revert to exact search. A build is
    asynchronous: 202 means the build started, and the response carries the building generation. Poll
    the GET form for completion.

    Args:
        name (str):
        body (IndexBuildRequest): The request body for `POST /v1/namespaces/{name}/index`.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, IndexStatusResponse]
    """

    return sync_detailed(
        name=name,
        client=client,
        body=body,
    ).parsed


async def asyncio_detailed(
    name: str,
    *,
    client: AuthenticatedClient,
    body: IndexBuildRequest,
) -> Response[Union[ApiError, IndexStatusResponse]]:
    """Create, change, or drop a collection index

     `type` is `hnsw`, `ivf`, `bq`, or null to drop the index and revert to exact search. A build is
    asynchronous: 202 means the build started, and the response carries the building generation. Poll
    the GET form for completion.

    Args:
        name (str):
        body (IndexBuildRequest): The request body for `POST /v1/namespaces/{name}/index`.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ApiError, IndexStatusResponse]]
    """

    kwargs = _get_kwargs(
        name=name,
        body=body,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    name: str,
    *,
    client: AuthenticatedClient,
    body: IndexBuildRequest,
) -> Optional[Union[ApiError, IndexStatusResponse]]:
    """Create, change, or drop a collection index

     `type` is `hnsw`, `ivf`, `bq`, or null to drop the index and revert to exact search. A build is
    asynchronous: 202 means the build started, and the response carries the building generation. Poll
    the GET form for completion.

    Args:
        name (str):
        body (IndexBuildRequest): The request body for `POST /v1/namespaces/{name}/index`.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ApiError, IndexStatusResponse]
    """

    return (
        await asyncio_detailed(
            name=name,
            client=client,
            body=body,
        )
    ).parsed
