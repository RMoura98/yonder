// Static server: "/" -> this folder, "/app/" -> Yonder app/src (HTML gets the Tauri stub injected).
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = fileURLToPath(new URL(".", import.meta.url));
const APP = process.argv[2] || join(HERE, "..", "..", "app", "src");
const PORT = Number(process.argv[3] || 8765);
const MIME = { ".html": "text/html", ".js": "text/javascript", ".mjs": "text/javascript", ".css": "text/css", ".svg": "image/svg+xml", ".png": "image/png", ".jpg": "image/jpeg", ".json": "application/json" };

createServer(async (req, res) => {
  try {
    const url = new URL(req.url, "http://x");
    let path = decodeURIComponent(url.pathname);
    let file;
    if (path.startsWith("/app/")) file = join(APP, normalize(path.slice(5)));
    else file = join(HERE, normalize(path === "/" ? "stage.html" : path.slice(1)));
    let body = await readFile(file);
    const ext = extname(file);
    if (ext === ".html" && path.startsWith("/app/")) {
      body = Buffer.from(body.toString("utf8").replace("<head>", '<head><script src="/stub.js"></script>'));
    }
    res.writeHead(200, { "content-type": MIME[ext] || "application/octet-stream", "cache-control": "no-store" });
    res.end(body);
  } catch (e) {
    res.writeHead(404);
    res.end("not found");
  }
}).listen(PORT, "127.0.0.1", () => console.log("serving on", PORT));
