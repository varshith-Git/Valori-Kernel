#!/usr/bin/env node
// Signs an HS256 JWT for the local PostgREST instance — standing in for
// what Supabase's own GoTrue issues in production. Same shape PostgREST
// expects: a `role` claim it uses to SET ROLE per request, plus (for
// `authenticated`) a `sub` claim auth.uid() reads.
//
// Usage: node sign_jwt.js <role> [sub-uuid] <jwt-secret> [expires-in-secs]
const crypto = require("crypto");

function b64url(input) {
  return Buffer.from(input)
    .toString("base64")
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");
}

function sign(payload, secret) {
  const header = b64url(JSON.stringify({ alg: "HS256", typ: "JWT" }));
  const body = b64url(JSON.stringify(payload));
  const sig = crypto
    .createHmac("sha256", secret)
    .update(`${header}.${body}`)
    .digest("base64")
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");
  return `${header}.${body}.${sig}`;
}

const [, , role, subOrSecret, maybeSecret, maybeExpiresIn] = process.argv;
if (!role) {
  console.error("usage: sign_jwt.js <role> [sub-uuid] <jwt-secret> [expires-in-secs]");
  process.exit(1);
}
const sub = maybeSecret ? subOrSecret : undefined;
const secret = maybeSecret ? maybeSecret : subOrSecret;
if (!secret) {
  console.error("usage: sign_jwt.js <role> [sub-uuid] <jwt-secret> [expires-in-secs]");
  process.exit(1);
}
const expiresIn = maybeExpiresIn ? Number(maybeExpiresIn) : 7200;

const payload = { role, exp: Math.floor(Date.now() / 1000) + expiresIn };
if (sub) payload.sub = sub;

console.log(sign(payload, secret));
