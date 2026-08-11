#!/usr/bin/env node
// Fronts a bare PostgREST instance at /rest/v1/* — see docker-compose.yml's
// `rest-shim` service comment for why this exists (real Supabase serves
// PostgREST behind this exact path prefix; a bare PostgREST serves at
// root, so @supabase/supabase-js's real request path 404s without this).
const http = require("http");

const targetHost = process.env.TARGET_HOST || "localhost";
const targetPort = Number(process.env.TARGET_PORT || 3211);
const listenPort = Number(process.env.LISTEN_PORT || 3212);

const server = http.createServer((req, res) => {
  const path = req.url.startsWith("/rest/v1")
    ? req.url.slice("/rest/v1".length) || "/"
    : req.url;
  const proxyReq = http.request(
    { host: targetHost, port: targetPort, path, method: req.method, headers: req.headers },
    (proxyRes) => {
      res.writeHead(proxyRes.statusCode, proxyRes.headers);
      proxyRes.pipe(res);
    },
  );
  req.pipe(proxyReq);
  proxyReq.on("error", (e) => {
    res.writeHead(502);
    res.end(String(e));
  });
});

server.listen(listenPort, () => {
  console.log(`rest/v1 shim on :${listenPort} -> ${targetHost}:${targetPort}`);
});
