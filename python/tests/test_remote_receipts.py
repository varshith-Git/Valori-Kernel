import pytest
from unittest.mock import MagicMock, patch
from valoricore.remote import SyncRemoteClient

def test_sync_remote_get_receipt():
    with patch("requests.Session.get") as mock_get:
        mock_resp = MagicMock()
        mock_resp.status_code = 200
        mock_resp.json.return_value = {
            "receipt_id": "test_id_123",
            "operation_hash": "abc...789",
            "state_hash_before": "state_1",
            "state_hash_after": "state_2"
        }
        mock_get.return_value = mock_resp

        client = SyncRemoteClient("http://localhost:3000")
        receipt = client.get_receipt()

        mock_get.assert_called_once_with("http://localhost:3000/v1/proof/receipt", timeout=5)
        assert receipt["receipt_id"] == "test_id_123"
        assert receipt["state_hash_before"] == "state_1"

def test_sync_remote_get_receipt_by_id():
    with patch("requests.Session.get") as mock_get:
        mock_resp = MagicMock()
        mock_resp.status_code = 200
        mock_resp.json.return_value = {
            "receipt_id": "rec_999",
            "operation_hash": "def...456"
        }
        mock_get.return_value = mock_resp

        client = SyncRemoteClient("http://localhost:3000")
        receipt = client.get_receipt_by_id("rec_999")

        mock_get.assert_called_once_with("http://localhost:3000/v1/proof/receipt/rec_999", timeout=5)
        assert receipt["receipt_id"] == "rec_999"
