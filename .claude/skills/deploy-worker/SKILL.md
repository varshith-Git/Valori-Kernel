# Deploy: worker (valori-node)

## Scope
Rolling out a `valori-node` binary/image version to a running host, cluster, or Kubernetes deployment.
Three real, verified deployment paths exist in this repo — use whichever the target actually runs, don't
invent a fourth.

1. **Docker Compose** — `docker-compose.yml` (single node), `docker-compose.cluster.yml` (3-node local/prod-shaped
   cluster). `make cluster` / `make cluster-down` (the latter runs `docker compose down -v` — **wipes
   volumes**, see Never do below).
2. **Kubernetes / Helm** — `deploy/helm/valori/` (`Chart.yaml`, `values.yaml`, `templates/{statefulset,
   service,headless-service,snapshot-cronjob}.yaml`). `values.yaml`'s own header: `helm install valori
   ./deploy/helm/valori -f my-values.yaml`. Persistence is explicit and separate per volume: `eventLog`
   (audit chain — "highly recommended in production," per the file's own comment) and `raftLog`
   (`raft.redb` — persistent Raft log + vote state).
3. **VM / bare infrastructure** — `terraform/aws/` and `terraform/azure/` provision the hosts;
   `docs/DEPLOYMENT.md`, `docs/DEPLOY_AWS.md`, `docs/DEPLOY_AZURE.md` document the node configuration on
   top of that infrastructure.

## Durable data — verify before any destructive step
`valori-node`'s durable state is: the local `events.log` (audit chain), any local snapshot file, the
`raft.redb` log (cluster mode), and — if `VALORI_OBJECT_STORE_URL` is set — the object-store copy of
snapshots/WAL archives (`docs/DR.md`). A worker/pod being individually replaceable (Kubernetes reschedule,
container restart) is **only** safe if the durable state survives that replacement — either via a
persistent volume (Helm's `persistence.eventLog`/`persistence.raftLog`) or object-store offload. Before
any destructive step (volume deletion, `docker compose down -v`, pod eviction from a node without PV
reattachment), verify which of these applies to the target deployment. Don't assume "it's just a
container, it's stateless" — check `values.yaml`'s `persistence:` block or the equivalent for the target.

## Steps

1. **Identify target**: which of the three paths above, and which host/cluster/namespace.
2. **Current version**: `curl http://<host>:<port>/health` (see `docs/DEPLOYMENT.md` §3.3 for the response
   shape) or check the running image tag directly (`docker ps` / `kubectl get pods -o jsonpath=...image`).
3. **Desired version**: the release tag from `.claude/skills/release/SKILL.md` — confirm it's actually
   published (`docker pull ghcr.io/varshith-git/valori-kernel/valori-node:<tag>` succeeds) before pointing
   a deployment at it.
4. **Durable-state check**: per the section above — confirm persistence config for the target.
5. **Deploy**:
   - Compose: update the image tag in the compose file (or `IMAGE_TAG` env if parameterized), `docker
     compose up -d` (not `up -d --build` unless rebuilding from source is actually intended).
   - Helm: `helm upgrade valori ./deploy/helm/valori -f <values> --set image.tag=<version>`.
   - Bare VM: follow `docs/DEPLOYMENT.md`'s process-management section for the target OS.
6. **Health**: re-check `/health` on the new instance before considering the rollout done.
7. **Version**: confirm the deployed version actually matches what you intended (`/health` or a version
   endpoint, per `docs/DEPLOYMENT.md`) — a deploy that silently pulled a stale cached image is a real
   failure mode, not a hypothetical one.
8. **Storage connectivity**: if `VALORI_OBJECT_STORE_URL` is set, confirm the node's startup self-test
   passed (the node write/read-tests the object store at startup and exits non-zero if unreachable —
   check startup logs, don't assume it's fine because the process is running).
9. **Metrics**: confirm `/metrics` or the equivalent is scraping if this deployment is monitored.
10. **Rollback path**: confirm the previous image tag is still pullable before proceeding, so step 5 is
    reversible.

## Failure handling
If the new instance fails health checks: do not delete the old instance/volume until the new one is
confirmed healthy — Compose and Helm both support running the previous version back up from its existing
image tag. For Helm specifically, `helm rollback valori <previous-revision>` is the built-in path.

## Never do
- **Never run `make cluster-down` (`docker compose down -v`) against anything with real durable data** —
  `-v` deletes volumes. This target is for local dev cluster teardown, not production.
- **Never assume a worker is safely replaceable without checking `persistence:` config or object-store
  offload first** — "it's stateless, just restart it" is only true if durable state is proven to survive
  the restart for *this specific deployment*, not as a general architecture assumption.
- Never prune Docker volumes/images (`docker system prune`, `docker volume prune`) on a host running a
  production node without first confirming which volumes hold `events.log`/`raft.redb` and excluding them.
- Never destroy the old instance before the new one passes its health check.
