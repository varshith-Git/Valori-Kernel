# Authentication & Security

Valori is designed to be secure-by-default when running in Remote Mode.

## Enabling Authentication

By default, `valori-node` runs without authentication for local development convenience. To enable authentication for production:

1.  **Set the `VALORI_AUTH_TOKEN` environment variable** when running the server.

```bash
export VALORI_AUTH_TOKEN="your-secure-secret-token"
./valori-node
```

When this variable is set, the server will **reject** any request that does not include the correct `Authorization` header.

### How to Generate a Secure Key

You can use any secure random string. A quick way to generate one is using `openssl`:

```bash
# Generate a 32-byte hex string
openssl rand -hex 32
```

## Connecting with Authentication

The Python client supports authentication seamlessly via the `api_key` parameter.

### Using `Valori` Interface

```python
from valori import Valori

# Connect to a secure remote node
client = Valori(remote="http://localhost:3000", api_key="your-secure-secret-token")

# All operations are now authenticated
client.upsert_vector([0.1, ...])
```

### Using `ProtocolClient` Directly

```python
from valori import ProtocolClient

client = ProtocolClient(
    remote="http://localhost:3000", 
    api_key="your-secure-secret-token",
    embed=my_embed_fn
)
```

### Using Adapters

Adapters also accept the `api_key`.

```python
from valori.adapters.base import ValoriAdapter

adapter = ValoriAdapter(
    base_url="http://localhost:3000",
    api_key="your-secure-secret-token"
)
```

## Security Best Practices

1.  **Use HTTPS**: In production, always put `valori-node` behind a reverse proxy (like Nginx or generic Cloud Load Balancer) that terminates TLS/SSL. The node itself speaks HTTP.
2.  **Rotation**: You can rotate the `VALORI_AUTH_TOKEN` by restarting the `valori-node` service with a new env var.
3.  **Network Isolation**: Even with Auth, it is recommended to run Valori inside a VPC or private network, exposing it only to your application services.
