"""The Ghidra workplace: an MCP server over HTTP on the unix socket in BATE_MCP_SOCKET.

Bate starts it once for the project. It drives one headless Ghidra that holds the project's
analysis in `project-data/<id>/ghidra/`: `ghidra_scripts/BateServer.java` runs in it and answers
JSON lines on stdin with JSON lines on stdout. Binaries are added with the tool `add_binary`, or
by copying them into `binaries/` in the project's folder; each is analysed on arrival.

It offers surfaces (see docs/workplaces.md): the project's binaries, a tree of the functions of
the binary shown, the decompiled function chosen in the tree, and its strings and imports.
It ends when its standard input closes (Bate went).
"""
import collections, glob, json, os, platform, queue, shutil, socketserver, subprocess, sys, threading, time
from http.server import BaseHTTPRequestHandler

sock = os.environ["BATE_MCP_SOCKET"]
DATA = os.path.dirname(sock)  # where it may write, with the project's folder
WORK = os.environ["BATE_WORK_DIR"]
FOLDER = os.getcwd()  # the workplace's own folder: Ghidra and the Java it needs are in it
PROJECT = os.path.join(DATA, "ghidra")
INBOX = os.path.join(WORK, "binaries")
LISTED = 200  # most lines a tool lists at once

lock = threading.RLock()  # the state below
binaries = {}  # name -> {"state": "analysing" | "ready" | "failed", "error": str}
shown = {"binary": None, "function": None}  # what the person's surfaces show
loaded = False
cache = {}  # what Ghidra answered, until something changes
subscribed = set()  # uris Bate asked to hear about
streams = []  # one queue per open event stream (GET)


class Failed(Exception):
    """Something the caller is told, in words."""


def natives(launcher):
    """Ghidra's release has no decompiler for macOS: builds it once from the source in the
    release, into an add-on in the data folder, where Ghidra looks for add-ons."""
    root = os.path.dirname(os.path.dirname(launcher))
    os_dir, arch = {"arm64": ("mac_arm_64", "arm64"), "x86_64": ("mac_x86_64", "x86_64")}.get(
        platform.machine(), (None, None))
    if sys.platform != "darwin" or os_dir is None:
        return
    if os.path.exists(f"{root}/Ghidra/Features/Decompiler/os/{os_dir}/decompile"):
        return
    add_on = f"{DATA}/Library/ghidra/{os.path.basename(root)}/Extensions/BateNatives"
    out = f"{add_on}/os/{os_dir}/decompile"
    if os.path.exists(out):
        return
    build = f"{DATA}/natives-build"
    shutil.rmtree(build, ignore_errors=True)
    shutil.copytree(f"{root}/Ghidra/Features/Decompiler/src/decompile/cpp", build)
    made = subprocess.run(["make", f"-j{os.cpu_count() or 2}", "ghidra_opt", f"ARCH_TYPE=-arch {arch}"],
                          cwd=build, stdin=subprocess.DEVNULL, capture_output=True, text=True)
    if made.returncode:
        raise Failed("cannot build Ghidra's decompiler (it needs `make` and a C++ compiler, from "
                     "Xcode's command line tools: `xcode-select --install`): " + made.stderr[-500:])
    os.makedirs(os.path.dirname(out), exist_ok=True)
    shutil.copy(f"{build}/ghidra_opt", out)
    with open(f"{add_on}/Module.manifest", "w"):
        pass
    with open(f"{add_on}/extension.properties", "w") as f:
        f.write("name=BateNatives\nversion=1\n")
    shutil.rmtree(build, ignore_errors=True)


class Ghidra:
    """One headless Ghidra, asked one thing at a time."""

    def __init__(self):
        self.lock = threading.Lock()
        self.proc = None
        self.answers = queue.Queue()
        self.tail = collections.deque(maxlen=30)  # its last lines, to say why it stopped
        self.n = 0

    def start(self):
        found = lambda pattern: (glob.glob(f"{FOLDER}/{pattern}") or [None])[0]
        launcher, jdk = found("ghidra/*/support/analyzeHeadless"), found("jdk/*/Contents/Home")
        if not launcher or not jdk:
            raise Failed("Ghidra or its Java is missing from the workplace's folder: install the workplace again")
        natives(launcher)
        # Java takes its home and temporary folder from the system, not from HOME and TMPDIR: Ghidra
        # keeps its settings and caches there, and the sandbox lets it write only the data folder.
        env = dict(os.environ, JAVA_HOME=jdk, JAVA_TOOL_OPTIONS=f"-Duser.home={DATA} -Djava.io.tmpdir={os.environ.get('TMPDIR', DATA)}",
                   PATH=f"{jdk}/bin:{os.environ.get('PATH', '')}")
        os.makedirs(PROJECT, exist_ok=True)
        # With no binary to import, the script runs by itself and answers until stdin closes.
        self.proc = subprocess.Popen(
            [launcher, PROJECT, "bate", "-scriptPath", f"{FOLDER}/ghidra_scripts", "-preScript", "BateServer.java"],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, env=env)
        answers = self.answers = queue.Queue()

        def read(out):
            for line in out:
                if line.startswith("BATE> "):
                    answers.put(json.loads(line[6:]))
                else:
                    self.tail.append(line.rstrip())
            answers.put(None)

        threading.Thread(target=read, args=(self.proc.stdout,), daemon=True).start()
        if self.wait(600) != {"ready": True}:
            raise Failed("Ghidra did not start")

    def wait(self, seconds):
        try:
            answer = self.answers.get(timeout=seconds)
        except queue.Empty:
            self.proc.kill()
            raise Failed(f"Ghidra did not answer in {seconds} seconds")
        if answer is None:
            self.proc = None
            raise Failed("Ghidra stopped: " + " | ".join(list(self.tail)[-6:]))
        return answer

    def ask(self, op, timeout=600, **args):
        if op != "import" and any(b["state"] == "analysing" for b in binaries.values()):
            raise Failed("Ghidra is busy analysing a binary: ask again when its state is ready")
        with self.lock:
            if self.proc is None or self.proc.poll() is not None:
                self.start()
            self.n += 1
            try:
                self.proc.stdin.write(json.dumps({"id": self.n, "op": op, **args}) + "\n")
                self.proc.stdin.flush()
            except OSError:
                self.wait(5)
            answer = self.wait(timeout)
            if "error" in answer:
                raise Failed(str(answer["error"]))
            return answer["result"]


ghidra = Ghidra()


def changed(edited=False):
    """Tells every stream about the surfaces that someone asked to hear about. `edited`: the
    analysis changed (a rename, a comment), so what was read from it is stale."""
    if edited:
        cache.clear()
    for uri in subscribed:
        event = {"jsonrpc": "2.0", "method": "notifications/resources/updated", "params": {"uri": uri}}
        for q in streams:
            q.put(f"data: {json.dumps(event)}\n\n".encode())


def load():
    """Learns the binaries that the project's analysis already holds."""
    global loaded
    with lock:
        if not loaded and os.path.exists(f"{PROJECT}/bate.gpr"):
            for name in ghidra.ask("programs"):
                binaries.setdefault(name, {"state": "ready", "error": ""})
        loaded = True
        if shown["binary"] is None:
            shown["binary"] = next(iter(sorted(binaries)), None)


def read_of(op, name, **args):
    """What Ghidra says of binary `name`, asked once until something changes."""
    key = (op, name, tuple(sorted(args.items())))
    with lock:
        if key not in cache:
            cache[key] = ghidra.ask(op, program=name, **args)
        return cache[key]


def binary_of(args):
    load()
    name = args.get("binary") or shown["binary"]
    if name is None:
        raise Failed(f"there is no binary yet: add one with add_binary, or copy it into {INBOX}")
    b = binaries.get(name)
    if b is None:
        raise Failed(f"there is no binary {name}; the binaries are: {', '.join(sorted(binaries))}")
    if b["state"] == "analysing":
        raise Failed(f"{name} is still being analysed: ask again when its state is ready")
    if b["state"] == "failed":
        raise Failed(f"{name} could not be analysed: {b['error']}")
    return name


jobs = queue.Queue()


def analyse():
    """Imports and analyses the binaries one after the other, as they were added."""
    while True:
        path, name = jobs.get()
        try:
            ghidra.ask("import", timeout=6 * 3600, path=path)
            state = {"state": "ready", "error": ""}
        except Failed as e:
            state = {"state": "failed", "error": str(e)[:500]}
        with lock:
            binaries[name] = state
            if shown["binary"] is None and state["state"] == "ready":
                shown["binary"] = name
            changed()


def add(path):
    load()
    path = os.path.realpath(path)
    if not os.path.isfile(path):
        raise Failed(f"{path} is not a file")
    name = os.path.basename(path)
    with lock:
        if name in binaries and binaries[name]["state"] != "failed":
            raise Failed(f"a binary called {name} is in the project already")
        binaries[name] = {"state": "analysing", "error": ""}
        changed()
    jobs.put((path, name))
    return name


def watch():
    """Adds what lands in `binaries/` in the project's folder, once its size has settled."""
    last, tried = {}, set()
    while True:
        time.sleep(1)
        try:
            now = {e.name: (e.stat().st_size, e.stat().st_mtime) for e in os.scandir(INBOX) if e.is_file()}
            for name, key in now.items():
                if last.get(name) == key and (name, key) not in tried:
                    tried.add((name, key))
                    load()
                    if name not in binaries:
                        add(os.path.join(INBOX, name))
            last = now
        except (OSError, Failed) as e:
            if not isinstance(e, FileNotFoundError):
                print(f"watching {INBOX}: {e}", file=sys.stderr, flush=True)


# The surfaces.
TABLE, TREE, CODE = (f"application/vnd.bate.{k}+json" for k in ("table", "tree", "code"))
BINARIES, FUNCTIONS, CODE_URI, STRINGS, IMPORTS = (f"ghidra://{k}" for k in ("binaries", "functions", "code", "strings", "imports"))
SURFACES = [
    (BINARIES, "Binaries", "The project's binaries and whether they are analysed.", TABLE),
    (FUNCTIONS, "Functions", "The functions of the binary shown, by whether they have a name.", TREE),
    (CODE_URI, "Code", "The chosen function, decompiled.", CODE),
    (STRINGS, "Strings", "The strings of the binary shown.", TABLE),
    (IMPORTS, "Imports", "What the binary shown takes from libraries.", TABLE),
]


def table(columns, rows, part, **more):
    offset, limit = max(0, int(part.get("offset", 0))), max(1, int(part.get("limit", 100)))
    return {"columns": columns, "rows": [{"id": i, "cells": c} for i, c in rows[offset:offset + limit]],
            "offset": offset, "total": len(rows), **more}


def viewed():
    """The binary the surfaces show, or None while there is none that is ready."""
    load()
    b = binaries.get(shown["binary"])
    return shown["binary"] if b and b["state"] == "ready" else None


def read(uri, part):
    with lock:
        try:
            return read_surface(uri, part)
        except Failed as e:  # shown to the person, as what the surface holds
            message = str(e)
        if uri == FUNCTIONS:
            return TREE, {"nodes": [{"label": message, "depth": 0}]}
        if uri == CODE_URI:
            return CODE, {"language": "text", "lines": [message]}
        return TABLE, table(["Message"], [(None, [message])], {})


def read_surface(uri, part):
    name = viewed()
    if uri == BINARIES:
        load()
        rows = [(n, [n, b["state"] + (": " + b["error"] if b["error"] else ""), "*" if n == shown["binary"] else ""])
                for n, b in sorted(binaries.items())]
        return TABLE, table(["Binary", "State", "Shown"], rows, part, select="show_binary")
    if uri == FUNCTIONS:
        functions = read_of("functions", name) if name else []
        nodes = []
        for label, group in (("Named", [f for f in functions if f["source"] != "DEFAULT"]),
                             ("Unnamed", [f for f in functions if f["source"] == "DEFAULT"])):
            nodes.append({"id": f"group-{label}", "label": f"{label} ({len(group)})", "depth": 0})
            nodes += [{"id": f["address"], "label": f"{f['name']}  {f['address']}", "depth": 1} for f in group]
        return TREE, {"nodes": nodes, "select": "show_function"}
    if uri == CODE_URI:
        if name is None:
            return CODE, {"language": "text", "lines": [f"No binary is analysed yet: add one with add_binary, or copy it into {INBOX}"]}
        if shown["function"] is None:
            return CODE, {"language": "text", "lines": ["Choose a function in the tree of functions."]}
        return CODE, {"language": "c", "lines": decompiled(name, shown["function"]).splitlines(),
                      "actions": [{"tool": "comment_shown", "label": "Comment", "input": True}]}
    if uri == STRINGS:
        rows = [(s["address"], [s["address"], s["value"]]) for s in read_of("strings", name)] if name else []
        return TABLE, table(["Address", "String"], rows, part)
    if uri == IMPORTS:
        rows = [(i["address"] or i["name"], [i["library"], i["name"], i["address"]]) for i in read_of("imports", name)] if name else []
        return TABLE, table(["Library", "Name", "Address"], rows, part)
    raise KeyError(uri)


def decompiled(name, function):
    """The function's code with the comments in it, as the person's view and a bot's read show it."""
    d = read_of("decompile", name, function=function)
    return d["code"]


# The tools.
def tool(name, description, properties=None, required=()):
    return {"name": name, "description": description,
            "inputSchema": {"type": "object", "properties": properties or {}, "required": list(required)}}


S = {"type": "string"}
BINARY = {"binary": {**S, "description": "The binary's name; the one the person is looking at when left out."}}
SURFACE = {"uri": S, "target": S}
TOOLS = [
    tool("add_binary", "Adds a binary (a file on this Mac that you can read) to the project and analyses it, which "
         "takes from seconds to many minutes. Files copied into `binaries/` in the project's folder are added too.",
         {"path": S}, ["path"]),
    tool("list_binaries", "Lists the project's binaries and whether each is analysed."),
    tool("list_functions", f"Lists the functions of a binary: address, name and size in bytes, {LISTED} at most.",
         {**BINARY, "filter": {**S, "description": "Only names that contain this text."}, "skip": {"type": "integer", "minimum": 0}}),
    tool("decompile", "Decompiles a function to C. Comments that the person or a bot made on it are in the code.",
         {**BINARY, "function": {**S, "description": "The function's name or an address in it."}}, ["function"]),
    tool("find_references", "Lists where an address or a function is referred to from.",
         {**BINARY, "target": {**S, "description": "A function's name, or an address."}}, ["target"]),
    tool("rename_function", "Gives a function a new name; the person's views change at once.",
         {**BINARY, "function": S, "name": S}, ["function", "name"]),
    tool("comment", "Writes a comment on a function (above its code), or on one address in it. The person sees it, "
         "and so does every bot that reads the function. An empty text removes the comment.",
         {**BINARY, "function": S, "text": S, "address": {**S, "description": "An address in the function; its start when left out."}},
         ["function", "text"]),
    tool("list_strings", f"Lists the strings of a binary: address and text, {LISTED} at most.",
         {**BINARY, "filter": {**S, "description": "Only strings that contain this text."}, "skip": {"type": "integer", "minimum": 0}}),
    tool("list_imports", f"Lists what a binary takes from libraries: library, name, address, {LISTED} at most.",
         {**BINARY, "filter": {**S, "description": "Only names that contain this text."}, "skip": {"type": "integer", "minimum": 0}}),
    tool("show_binary", "Shows binary `target` in the person's surfaces (a surface's select action).", SURFACE),
    tool("show_function", "Shows the function at `target` in the person's code view (a surface's select action).", SURFACE),
    tool("comment_shown", "Writes the person's comment `text` on the function their code view shows (the code view's "
         "action); an empty text removes it.", {**SURFACE, "text": S}),
]


def said(text):
    return {"content": [{"type": "text", "text": text}]}


def listed(rows, args, what):
    """`rows` filtered by the text, from `skip`, as the lines of a tool's answer."""
    wanted = (args.get("filter") or "").lower()
    rows = [r for r in rows if wanted in r.lower()]
    skip = max(0, int(args.get("skip", 0)))
    out = rows[skip:skip + LISTED]
    more = len(rows) - skip - len(out)
    tail = f"\n... {more} more {what}: ask again with skip={skip + LISTED}" if more > 0 else ""
    return said("\n".join(out) + tail if out else f"no {what}")


def call(name, args):
    if name == "add_binary":
        return said(f"{add(args['path'])} is being analysed; list_binaries says when it is ready")
    if name == "list_binaries":
        load()
        return said("\n".join(f"{n}  {b['state']}{': ' + b['error'] if b['error'] else ''}{'  (shown)' if n == shown['binary'] else ''}"
                              for n, b in sorted(binaries.items())) or "no binaries yet")
    if name == "show_binary":
        load()
        if args.get("target") not in binaries:
            raise Failed(f"there is no binary {args.get('target')}")
        shown["binary"], shown["function"] = args["target"], None
        changed()
        return said(f"showing {args['target']}")
    name_of = viewed() if name in ("show_function", "comment_shown") else binary_of(args)
    if name == "list_functions":
        return listed([f"{f['address']}  {f['name']}  {f['size']}" for f in read_of("functions", name_of)], args, "functions")
    if name == "list_strings":
        return listed([f"{s['address']}  {json.dumps(s['value'])}" for s in read_of("strings", name_of)], args, "strings")
    if name == "list_imports":
        return listed([f"{i['library']}  {i['name']}  {i['address']}" for i in read_of("imports", name_of)], args, "imports")
    if name == "decompile":
        d = read_of("decompile", name_of, function=args["function"])
        comments = "".join(f"\n// {c['kind']} comment at {c['address']}: {c['text']}" for c in d["comments"])
        return said(f"// {d['name']} at {d['address']}: {d['signature']}\n{d['code']}{comments}")
    if name == "find_references":
        refs = read_of("refs", name_of, target=args["target"])
        return said("\n".join(f"{r['from']}  in {r['function'] or '(no function)'}  {r['type']}" for r in refs) or "no references")
    if name == "rename_function":
        done = ghidra.ask("rename", program=name_of, function=args["function"], name=args["name"])
        changed(True)
        return said(f"{done['was']} at {done['address']} is now {done['name']}")
    if name == "comment":
        done = ghidra.ask("comment", program=name_of, function=args["function"], address=args.get("address"), text=args["text"])
        changed(True)
        return said(f"commented on {done['function']} at {done['address']}")
    if name == "comment_shown":
        if name_of is None or shown["function"] is None:
            raise Failed("no function is shown: choose one in the tree of functions")
        done = ghidra.ask("comment", program=name_of, function=shown["function"], address=None, text=args.get("text") or "")
        changed(True)
        return said(f"commented on {done['function']} at {done['address']}")
    if name == "show_function":
        target = str(args.get("target"))
        if name_of is None or not any(f["address"] == target for f in read_of("functions", name_of)):
            raise Failed(f"there is no function at {target}")
        shown["function"] = target
        changed()
        return said(f"showing the function at {target}")
    raise KeyError(name)


def answer(body):
    method = body.get("method")
    params = body.get("params") or {}
    if method == "initialize":
        return {"protocolVersion": "2025-03-26",
                "capabilities": {"tools": {}, "resources": {"subscribe": True}},
                "serverInfo": {"name": "ghidra", "version": "1.0.0"}}
    if method == "tools/list":
        return {"tools": TOOLS}
    if method == "tools/call":
        try:
            with lock:
                return call(params.get("name"), params.get("arguments") or {})
        except Failed as e:
            return {"isError": True, "content": [{"type": "text", "text": str(e)}]}
    if method == "resources/list":
        return {"resources": [{"uri": u, "name": n, "description": d, "mimeType": m} for u, n, d, m in SURFACES]}
    if method == "resources/read":
        part = (params.get("_meta") or {}).get("bate/part") or {}
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
threading.Thread(target=analyse, daemon=True).start()
threading.Thread(target=watch, daemon=True).start()
Server(sock, Handler).serve_forever()
