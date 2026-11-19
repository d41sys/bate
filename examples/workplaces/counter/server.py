"""The counter workplace: an MCP server over HTTP on the unix socket in BATE_MCP_SOCKET.

Bate starts it once for the project; every bot of the project calls the same server, so they
share one count. It ends when its standard input closes (Bate went).
"""
import json, os, socketserver, sys, threading
from http.server import BaseHTTPRequestHandler

sock = os.environ["BATE_MCP_SOCKET"]
# Its own folder is where it may write: the process id shows that it runs.
with open(os.path.join(os.path.dirname(sock), "pid"), "w") as f:
    f.write(str(os.getpid()))

count = 0
lock = threading.Lock()
TICK = {
    "name": "tick",
    "description": "Adds one to the project's counter and returns the total.",
    "inputSchema": {"type": "object", "properties": {}},
}


def answer(body):
    method = body.get("method")
    if method == "initialize":
        return {"protocolVersion": "2025-03-26", "capabilities": {"tools": {}},
                "serverInfo": {"name": "counter", "version": "1.0.0"}}
    if method == "tools/list":
        return {"tools": [TICK]}
    if method == "tools/call":
        global count
        with lock:
            count += 1
            return {"content": [{"type": "text", "text": f"count: {count}"}], "count": count}
    return {}


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def address_string(self):
        return "unix"

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["content-length"])))
        if "id" not in body:  # a notification has no answer
            self.send_response(202)
            self.send_header("content-length", "0")
            self.end_headers()
            return
        out = json.dumps({"jsonrpc": "2.0", "id": body["id"], "result": answer(body)}).encode()
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(out)))
        self.end_headers()
        self.wfile.write(out)


class Server(socketserver.ThreadingMixIn, socketserver.UnixStreamServer):
    daemon_threads = True


threading.Thread(target=lambda: (sys.stdin.read(), os._exit(0)), daemon=True).start()
Server(sock, Handler).serve_forever()
