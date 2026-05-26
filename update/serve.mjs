#!/usr/bin/env node
/**
 * 独立更新静态服务器（可选，不依赖 Tauri 内置服务）
 *
 * 用法：
 *   node update/serve.mjs [目录] [端口]
 *   node update/serve.mjs ./update-files 8765
 *
 * 将 latest.json 与安装包放在同一目录，客户端填写：
 *   http://127.0.0.1:8765/latest.json
 */

import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(process.argv[2] ?? path.join(__dirname, "files"));
const port = Number(process.argv[3] ?? 8765);

if (!fs.existsSync(root)) {
  fs.mkdirSync(root, { recursive: true });
  console.log(`已创建目录: ${root}`);
}

const mime = {
  ".json": "application/json; charset=utf-8",
  ".exe": "application/octet-stream",
  ".msi": "application/octet-stream",
  ".zip": "application/zip",
};

const server = http.createServer((req, res) => {
  res.setHeader("Access-Control-Allow-Origin", "*");
  const urlPath = decodeURIComponent((req.url ?? "/").split("?")[0]);
  const safe = path.normalize(urlPath).replace(/^(\.\.[/\\])+/, "");
  const filePath = path.join(root, safe === path.sep ? "index.html" : safe);

  if (!filePath.startsWith(root)) {
    res.writeHead(403);
    res.end("Forbidden");
    return;
  }

  fs.stat(filePath, (err, stat) => {
    if (err || !stat.isFile()) {
      res.writeHead(404);
      res.end("Not Found");
      return;
    }
    const ext = path.extname(filePath).toLowerCase();
    res.writeHead(200, { "Content-Type": mime[ext] ?? "application/octet-stream" });
    fs.createReadStream(filePath).pipe(res);
  });
});

server.listen(port, "0.0.0.0", () => {
  console.log(`更新文件目录: ${root}`);
  console.log(`清单地址:     http://127.0.0.1:${port}/latest.json`);
  console.log(`局域网访问:   将 127.0.0.1 换为本机 IP 即可`);
});
