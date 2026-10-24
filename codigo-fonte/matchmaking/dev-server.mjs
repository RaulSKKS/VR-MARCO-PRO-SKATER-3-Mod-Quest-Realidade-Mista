// The Worker on this PC, for trying online rooms before (or without) deploying it: an in-memory SQLite database
// stands in for D1, and the caller's address stands in for Cloudflare's cf-connecting-ip.
// Run: node matchmaking/dev-server.mjs [port]       then start the game with --net-match-url http://<this PC>:8787
// (Node 22 or later. No relay: the TURN secrets are not set here, so only direct connections are made.)
import fs from "node:fs";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { DatabaseSync } from "node:sqlite";

const src = fs.readFileSync(new URL("./worker.js", import.meta.url), "utf8").replace("export default", "const worker =") + "\nexport { worker };";
const copy = path.join(fs.mkdtempSync(path.join(os.tmpdir(), "skate-worker-dev-")), "worker.mjs");
fs.writeFileSync(copy, src);
const { worker } = await import(pathToFileURL(copy).href);

const sqlite = new DatabaseSync(":memory:");
sqlite.exec(fs.readFileSync(new URL("./schema.sql", import.meta.url), "utf8"));
const DB = { prepare(sql) { const st = sqlite.prepare(sql); let args = []; return {
    bind(...a) { args = a; return this; },
    async first() { return st.get(...args) ?? null; },
    async all() { return { results: st.all(...args) }; },
    async run() { const r = st.run(...args); return { meta: { changes: Number(r.changes) } }; },
}; } };

const port = Number(process.argv[2] ?? 8787);
http.createServer(async (req, res) => {
    const chunks = [];
    for await (const c of req) chunks.push(c);
    const ip = String(req.socket.remoteAddress ?? "").replace(/^::ffff:/, "");
    const request = new Request(`http://localhost${req.url}`, {
        method: req.method,
        headers: { "cf-connecting-ip": ip, "content-type": "application/json" },
        body: req.method === "POST" ? Buffer.concat(chunks) : undefined,
    });
    const r = await worker.fetch(request, { DB });
    const body = await r.text();
    if (!req.url.endsWith("/poll")) console.log(`${ip} ${req.url} -> ${r.status} ${body.slice(0, 160)}`);
    res.writeHead(r.status, { "content-type": "application/json" });
    res.end(body);
}).listen(port, () => console.log(`matchmaking dev server on http://0.0.0.0:${port}`));
