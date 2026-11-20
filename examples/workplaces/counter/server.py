"""The counter workplace: an MCP server over HTTP on the unix socket in BATE_MCP_SOCKET.

Bate starts it once for the project; every bot of the project calls the same server, so they
share one count. It ends when its standard input closes (Bate went).

It also offers one surface of each kind as MCP resources (see docs/workplaces.md, Surfaces):
a table of the ticks, a tree that groups them by tens, and a code view of its state.
"""
import json, os, queue, socketserver, sys, threading
from http.server import BaseHTTPRequestHandler

sock = os.environ["BATE_MCP_SOCKET"]
# Its own folder is where it may write: the process id shows that it runs.
with open(os.path.join(os.path.dirname(sock), "pid"), "w") as f:
    f.write(str(os.getpid()))

count = 0
picked = None  # the tick the person (or a bot) chose
opened = None  # the tick that was opened last
lock = threading.Lock()
subscribed = set()  # uris Bate asked to hear about
streams = []  # one queue per open event stream (GET)

TABLE, TREE, CODE = (f"application/vnd.bate.{k}+json" for k in ("table", "tree", "code"))
TICKS, TENS, REPORT = "counter://ticks", "counter://tens", "counter://report"
TOOLS = [
    {"name": "tick", "description": "Adds one to the project's counter and returns the total.",
     "inputSchema": {"type": "object", "properties": {"times": {"type": "integer", "minimum": 1}}}},
    {"name": "reset", "description": "Sets the counter back to zero.",
     "inputSchema": {"type": "object", "properties": {}}},
    {"name": "pick", "description": "Chooses tick `target` (a surface's select action).",
     "inputSchema": {"type": "object", "properties": {"uri": {"type": "string"}, "target": {"type": "string"}}}},
    {"name": "peek", "description": "Opens tick `target` (a surface's open action) and describes it.",
     "inputSchema": {"type": "object", "properties": {"uri": {"type": "string"}, "target": {"type": "string"}}}},
]
ACTIONS = [{"tool": "tick", "label": "Tick"}, {"tool": "reset", "label": "Reset"}]


def changed():
    """Tells every stream about the surfaces that someone asked to hear about. Call with `lock`."""
    for uri in subscribed:
        event = {"jsonrpc": "2.0", "method": "notifications/resources/updated", "params": {"uri": uri}}
        for q in streams:
            q.put(f"data: {json.dumps(event)}\n\n".encode())


def listing(did):
    """Tells every stream, when `did`, that the list of surfaces changed. Call with `lock`."""
    if did:
        for q in streams:
            q.put(b'data: {"jsonrpc":"2.0","method":"notifications/resources/list_changed"}\n\n')


def resources():
    found = [
        {"uri": TICKS, "name": "Ticks", "description": "Every tick so far.", "mimeType": TABLE},
        {"uri": REPORT, "name": "State", "description": "What the program keeps.", "mimeType": CODE},
    ]
    if count:  # a tree of nothing is not offered
        found.insert(1, {"uri": TENS, "name": "Tens", "description": "The ticks by tens.", "mimeType": TREE})
    return found


def table(offset, limit):
    rows = [{"id": str(n), "cells": [str(n), "*" if n == picked else ""]}
            for n in range(offset + 1, min(count, offset + limit) + 1)]
    return {"columns": ["Tick", "Picked"], "rows": rows, "offset": offset, "total": count,
            "select": "pick", "open": "peek", "actions": ACTIONS}


def tree():
    # Sends everything and leaves the cutting to Bate (no `offset`).
    nodes = []
    for g in range((count + 9) // 10):
        nodes.append({"id": f"g{g}", "label": f"ticks {g * 10 + 1} to {min(count, g * 10 + 10)}", "depth": 0})
        nodes += [{"id": str(n), "label": f"tick {n}", "depth": 1} for n in range(g * 10 + 1, min(count, g * 10 + 10) + 1)]
    return {"nodes": nodes, "select": "pick", "open": "peek", "actions": ACTIONS}


def report():
    state = {"count": count, "picked": picked, "opened": opened}
    return {"language": "json", "lines": json.dumps(state, indent=2).splitlines(), "actions": ACTIONS}


def read(uri, part):
    if uri == TICKS:
        return TABLE, table(max(0, int(part.get("offset", 0))), max(1, int(part.get("limit", 100))))
    if uri == TENS and count:
        return TREE, tree()
    if uri == REPORT:
        return CODE, report()
    raise KeyError(uri)


def call(name, args):
    global count, picked, opened
    if name == "tick":
        count += max(1, int(args.get("times", 1)))
        return {"content": [{"type": "text", "text": f"count: {count}"}], "count": count}
    if name == "reset":
        count, picked, opened = 0, None, None
        return {"content": [{"type": "text", "text": "count: 0"}], "count": 0}
    n = int(args.get("target", 0))
    if not 1 <= n <= count:
        return {"isError": True, "content": [{"type": "text", "text": f"there is no tick {n}"}]}
    if name == "pick":
        picked = n
        return {"content": [{"type": "text", "text": f"picked {n}"}]}
    opened = n
    return {"content": [{"type": "text", "text": f"tick {n} of {count}"}]}


def answer(body):
    method = body.get("method")
    params = body.get("params") or {}
    if method == "initialize":
        return {"protocolVersion": "2025-03-26",
                "capabilities": {"tools": {}, "resources": {"subscribe": True, "listChanged": True}},
                "serverInfo": {"name": "counter", "version": "1.0.0"}}
    if method == "tools/list":
        return {"tools": TOOLS}
    if method == "tools/call":
        with lock:
            before = count
            out = call(params.get("name", "tick"), params.get("arguments") or {})
            changed()
            listing(bool(before) != bool(count))
            return out
    if method == "resources/list":
        with lock:
            return {"resources": resources()}
    if method == "resources/read":
        part = (params.get("_meta") or {}).get("bate/part") or {}
        with lock:
            mime, doc = read(params["uri"], part)
        return {"contents": [{"uri": params["uri"], "mimeType": mime, "text": json.dumps(doc)}]}
    if method == "resources/subscribe":
        with lock:
            subscribed.add(params["uri"])
        return {}
    return {}


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def address_string(self):
        return "unix"

    def do_GET(self):
        # The stream of notifications (MCP's GET): one event per line group, until the client goes.
        q = queue.Queue()
        with lock:
            streams.append(q)
        self.close_connection = True
        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.send_header("connection", "close")
        self.end_headers()
        try:
            while True:
                self.wfile.write(q.get())
                self.wfile.flush()
        except OSError:
            pass
        finally:
            with lock:
                streams.remove(q)

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["content-length"])))
        if "id" not in body:  # a notification has no answer
            self.send_response(202)
            self.send_header("content-length", "0")
            self.end_headers()
            return
        try:
            result = {"result": answer(body)}
        except (KeyError, ValueError) as e:
            result = {"error": {"code": -32602, "message": f"{type(e).__name__}: {e}"}}
        out = json.dumps({"jsonrpc": "2.0", "id": body["id"], **result}).encode()
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(out)))
        self.end_headers()
        self.wfile.write(out)


class Server(socketserver.ThreadingMixIn, socketserver.UnixStreamServer):
    daemon_threads = True


threading.Thread(target=lambda: (sys.stdin.read(), os._exit(0)), daemon=True).start()
Server(sock, Handler).serve_forever()
