# Authentication & Security

## Enabling Authentication

By default, `valori-node` runs without authentication for local development. To enable it:

```bash
export VALORI_AUTH_TOKEN="$(openssl rand -hex 32)"
cargo run --release -p valori-node
```

Any request without a matching `Authorization: Bearer <token>` header is rejected with 401.

## Connecting with Authentication

```python
from valoricore.remote import SyncRemoteClient

client = SyncRemoteClient("http://localhost:3000", token="your-secure-secret-token")

# All calls now include Authorization: Bearer <token>
client.health()
client.insert([0.1, 0.2, 0.3])
```

The `token` parameter sets `Authorization: Bearer <value>` on every request.

## curl example

```bash
curl -H "Authorization: Bearer your-secure-secret-token" \
     http://localhost:3000/health
```

## Security best practices

1. **HTTPS in production** — put `valori-node` behind a TLS-terminating reverse proxy (Nginx, AWS ALB, Cloudflare). The node speaks plain HTTP.
2. **Key rotation** — restart `valori-node` with a new `VALORI_AUTH_TOKEN` value. Existing connections are severed; clients must re-authenticate.
3. **Network isolation** — run Valori inside a VPC or private subnet, accessible only from your application tier. Auth is defence-in-depth, not a perimeter.
4. **mTLS for cluster** — inter-node Raft traffic can be protected with `VALORI_TLS_CA` / `VALORI_TLS_CERT` / `VALORI_TLS_KEY`. See [CLUSTER.md](./CLUSTER.md).
