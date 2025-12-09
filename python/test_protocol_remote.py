
# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
import unittest
from unittest.mock import MagicMock, patch
from valori.protocol import ProtocolClient

class TestProtocolRemote(unittest.TestCase):
    def setUp(self):
        self.dummy_embed = lambda x: [0.1] * 16
        self.client = ProtocolClient(embed=self.dummy_embed, remote="http://mock-node:3000")

    @patch("requests.Session.post")
    def test_upsert_vector(self, mock_post):
        # Mock response
        mock_resp = MagicMock()
        mock_resp.json.return_value = {
            "memory_id": "rec:10",
            "record_id": 10,
            "document_node_id": 100,
            "chunk_node_id": 200
        }
        mock_resp.raise_for_status.return_value = None
        mock_post.return_value = mock_resp

        vec = [0.1] * 16
        res = self.client.upsert_vector(vec)
        
        self.assertEqual(res["record_id"], 10)
        self.assertEqual(res["memory_id"], "rec:10")
        
        # Verify call
        args, kwargs = mock_post.call_args
        self.assertEqual(args[0], "http://mock-node:3000/v1/memory/upsert_vector")
        self.assertEqual(kwargs["json"]["vector"], vec)

    @patch("requests.Session.post")
    def test_search_vector(self, mock_post):
        mock_resp = MagicMock()
        mock_resp.json.return_value = {
            "results": [
                {"memory_id": "rec:5", "record_id": 5, "score": 999}
            ]
        }
        mock_post.return_value = mock_resp

        vec = [0.1] * 16
        res = self.client.search_vector(vec, k=3)
        
        self.assertEqual(len(res["results"]), 1)
        self.assertEqual(res["results"][0]["record_id"], 5)
        
        args, kwargs = mock_post.call_args
        self.assertEqual(args[0], "http://mock-node:3000/v1/memory/search_vector")
        self.assertEqual(kwargs["json"]["k"], 3)

    @patch("requests.Session.post")
    def test_upsert_text(self, mock_post):
        # We test a case that produces exactly 1 chunk
        
        resp1 = {
            "memory_id": "rec:1", "record_id": 1, 
            "document_node_id": 50, "chunk_node_id": 51
        }
        
        mock_resp = MagicMock()
        mock_resp.json.side_effect = [resp1]
        mock_resp.raise_for_status.return_value = None # Ensure this is mocked too
        mock_post.return_value = mock_resp

        # text that fits in one chunk
        text = "short text"
        res = self.client.upsert_text(text, chunk_size=512)
        
        self.assertEqual(res["document_node_id"], 50)
        self.assertEqual(res["chunk_count"], 1)
        self.assertEqual(res["record_ids"], [1])
        
        # Verify it called upsert_vector
        args, kwargs = mock_post.call_args
        self.assertEqual(args[0], "http://mock-node:3000/v1/memory/upsert_vector")

if __name__ == "__main__":
    unittest.main()
