#!/usr/bin/env node
// Supporter codes: proof that someone paid Sokki at least €20 in total, so
// they may switch off the Sokki line under summaries. Sokki has no accounts;
// the app checks the code offline against the public key in
// crates/sokki-engine/src/supporter.rs.
//
//   node scripts/supporter-code.mjs keygen          new key pair (once)
//   node scripts/supporter-code.mjs mint [via]       a code, e.g. via=github
//   node scripts/supporter-code.mjs check <code>     what a code says
//
// The private key lives outside the repo, in $SOKKI_SUPPORTER_KEY or
// ~/.config/sokki/supporter-signing-key.jwk. Keep a backup: without it no
// new codes; a new key pair makes every code given out so far invalid.
//
// Code: "sokki-" + base64url(JSON payload) + "." + base64url(Ed25519
// signature over the base64url payload text). Payload: {"v":1,"via","at"}.
// No name, no mail address, no amount.

import { webcrypto as crypto } from "node:crypto";
import { readFileSync, writeFileSync, existsSync, mkdirSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";

const KEY =
  process.env.SOKKI_SUPPORTER_KEY ||
  join(homedir(), ".config/sokki/supporter-signing-key.jwk");
const b64url = (buf) => Buffer.from(buf).toString("base64url");

async function keygen() {
  if (existsSync(KEY)) throw new Error(`${KEY} exists; not overwriting`);
  const pair = await crypto.subtle.generateKey({ name: "Ed25519" }, true, ["sign", "verify"]);
  const jwk = await crypto.subtle.exportKey("jwk", pair.privateKey);
  mkdirSync(dirname(KEY), { recursive: true });
  writeFileSync(KEY, JSON.stringify(jwk), { mode: 0o600 });
  console.log(`private key: ${KEY}`);
  console.log(`public key (base64url, put into supporter.rs): ${jwk.x}`);
}

async function mint(via = "manual") {
  if (!/^[a-z]{1,16}$/.test(via)) throw new Error("via: a short lowercase word");
  const jwk = JSON.parse(readFileSync(KEY, "utf8"));
  const key = await crypto.subtle.importKey("jwk", jwk, { name: "Ed25519" }, false, ["sign"]);
  const payload = b64url(
    JSON.stringify({ v: 1, via, at: new Date().toISOString().slice(0, 10) }),
  );
  const sig = await crypto.subtle.sign({ name: "Ed25519" }, key, Buffer.from(payload));
  console.log(`sokki-${payload}.${b64url(sig)}`);
}

async function check(code) {
  const jwk = JSON.parse(readFileSync(KEY, "utf8"));
  const pub = await crypto.subtle.importKey(
    "jwk",
    { kty: "OKP", crv: "Ed25519", x: jwk.x },
    { name: "Ed25519" },
    false,
    ["verify"],
  );
  const [payload, sig] = code.replace(/\s+/g, "").replace(/^sokki-/, "").split(".");
  const ok = await crypto.subtle.verify(
    { name: "Ed25519" },
    pub,
    Buffer.from(sig, "base64url"),
    Buffer.from(payload),
  );
  console.log(ok ? Buffer.from(payload, "base64url").toString() : "invalid");
  process.exit(ok ? 0 : 1);
}

const [cmd, arg] = process.argv.slice(2);
const run = { keygen, mint, check }[cmd];
if (!run) {
  console.error("usage: supporter-code.mjs keygen | mint [via] | check <code>");
  process.exit(2);
}
run(arg).catch((e) => {
  console.error(e.message);
  process.exit(1);
});
