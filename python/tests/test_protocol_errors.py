# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
import pytest
import json
from unittest.mock import MagicMock, patch
from valori.protocol import ProtocolRemoteClient, ProtocolError, AuthError, ValidationError

BASE_URL = "http://test-server"

@pytest.fixture
def client():
    # Mock embedder
    embed = lambda x: [0.0] * 16
    return ProtocolRemoteClient(BASE_URL, embed, expected_dim=16, api_key="secret")

def mock_response(status=200, json_data=None, text="", reason="OK"):
    mock = MagicMock()
    mock.status_code = status
    mock.reason = reason
    mock.ok = 200 <= status < 300
    mock.text = text
    if json_data is not None:
        mock.json.return_value = json_data
    else:
        # If json() is called when no json_data provided, raise ValueError
        mock.json.side_effect = ValueError("No JSON")
    return mock

# ...

def test_path_construction(client):
    # Valid 200 OK
    resp = mock_response(json_data={"ok": True}) # Fixed kwarg name
    
    with patch.object(client.session, 'post', return_value=resp) as mock_post:
        # Passing path with leading slash
        client._post("/foo/bar", {})
        args, _ = mock_post.call_args
        assert args[0] == f"{BASE_URL}/foo/bar"
        
        # Passing path without leading slash
        client._post("foo/bar", {})
        args, _ = mock_post.call_args
        assert args[0] == f"{BASE_URL}/foo/bar"

def test_validation_error(client):
    # Wrong Dim
    with pytest.raises(ValueError, match="16-dimensional"):
        client.upsert_vector([0.0]*15)
        
    # Out of bounds (ValidationError)
    # Match "out of allowed range" which is what protocol.py raises
    with pytest.raises(ValidationError, match="out of allowed range"):
        client.upsert_vector([40000.0] + [0.0]*15)

def test_server_error_json(client):
    # Server error with JSON message
    resp = mock_response(status=500, json_data={"error": "Something went wrong"})
    with patch.object(client.session, 'post', return_value=resp):
        with pytest.raises(ProtocolError, match="Something went wrong"):
            client._post("fail", {})

def test_server_error_raw(client):
    # Server error without JSON
    resp = mock_response(status=500, text="Internal Server Error")
    with patch.object(client.session, 'post', return_value=resp):
        with pytest.raises(ProtocolError, match="Internal Server Error"):
            client._post("fail", {})

def test_non_json_success(client):
    # 200 OK but bad JSON
    resp = mock_response(status=200, text="{invalid_json")
    with patch.object(client.session, 'post', return_value=resp):
        with pytest.raises(ProtocolError, match="non-JSON response"):
            client._post("badjson", {})
