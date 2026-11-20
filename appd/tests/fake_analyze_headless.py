#!/usr/bin/env python3
"""A stand-in for Ghidra's `analyzeHeadless` in the tests of the Ghidra workplace.

It speaks the protocol of workplaces/ghidra/ghidra_scripts/BateServer.java: one JSON request per
line on stdin, `BATE> ` and one JSON answer per line on stdout, among log lines of its own. Its
project is a file in the project folder, read again for each request, as the person's own changes to
a real project (made while the workplace is off) would be found. A binary is made of what its
first bytes say: `SLOW` takes 1.5 seconds to analyse, `BAD` cannot be read.
"""
import json, os, sys, time

project = sys.argv[1]
assert sys.argv[3:] == ["-scriptPath", sys.argv[4], "-preScript", "BateServer.java"], sys.argv
assert "-Duser.home=" in os.environ["JAVA_TOOL_OPTIONS"], "Java must keep its settings in the data folder"
assert os.environ["PATH"].split(":")[0].endswith("/Contents/Home/bin"), "the Java of the workplace comes first"
store = os.path.join(project, "fake.json")
open(os.path.join(project, "bate.gpr"), "a").close()


def load():
    try:
        with open(store) as f:
            return json.load(f)
    except FileNotFoundError:
        return {}


def save(programs):
    with open(store, "w") as f:
        json.dump(programs, f)


def find(program, what):
    for f in program["functions"]:
        if f["name"] == what or f["address"] == what:
            return f
    raise ValueError(f"there is no function {what}")


def handle(op, r):
    programs = load()
    if op == "programs":
        return sorted(programs)
    if op == "import":
        path = r["path"]
        with open(path, "rb") as f:
            head = f.read(4)
        if head == b"BAD!":
            raise ValueError(f"Ghidra cannot read {path} as a program: no loader")
        if head == b"SLOW":
            time.sleep(1.5)
        functions = [{"address": "100000978", "name": "entry", "size": 40, "source": "IMPORTED"}]
        functions += [{"address": f"{0x100001000 + 0x100 * i:x}", "name": f"FUN_{0x100001000 + 0x100 * i:x}",
                       "size": 64, "source": "DEFAULT"} for i in range(1, 6)]
        name = os.path.basename(path)
        programs[name] = {
            "functions": functions, "comments": {},
            "strings": [{"address": "1000005a4", "value": "/usr/lib/dyld"}, {"address": "100000630", "value": "usage: tool [-v]"}],
            "imports": [{"library": "/usr/lib/libSystem.B.dylib", "name": "_printf", "address": ""}],
        }
        save(programs)
        return {"program": name, "functions": len(functions)}
    program = programs.get(r["program"])
    if program is None:
        raise ValueError(f"there is no binary {r['program']}")
    if op == "functions":
        return program["functions"]
    if op == "strings":
        return program["strings"]
    if op == "imports":
        return program["imports"]
    f = find(program, r.get("function") or r.get("target"))
    if op == "decompile":
        lo = int(f["address"], 16)
        comments = [{"address": a, "kind": k, "text": t} for a, kinds in program["comments"].items()
                    for k, t in kinds.items() if lo <= int(a, 16) < lo + f["size"]]
        head = "".join(f"/* {c['text']} */\n" for c in comments if c["kind"] == "plate")
        return {"address": f["address"], "name": f["name"], "signature": f"void {f['name']}(void)",
                "code": f"{head}\nvoid {f['name']}(void)\n\n{{\n  helper();\n  return;\n}}\n", "comments": comments}
    if op == "refs":
        return [{"from": "100001010", "function": "entry", "type": "UNCONDITIONAL_CALL"}]
    if op == "rename":
        was, f["name"], f["source"] = f["name"], r["name"], "USER_DEFINED"
        save(programs)
        return {"address": f["address"], "was": was, "name": f["name"]}
    if op == "comment":
        address = r.get("address") or f["address"]
        kinds = program["comments"].setdefault(address, {})
        kind = "plate" if address == f["address"] else "pre"
        if r["text"]:
            kinds[kind] = r["text"]
        else:
            kinds.pop(kind, None)
        save(programs)
        return {"address": address, "function": f["name"]}
    raise ValueError(f"unknown op {op}")


def say(what):
    print("BATE> " + json.dumps(what), flush=True)


print("INFO  Using log file: noise that is not an answer (LoggingInitialization)", flush=True)
say({"ready": True})
for line in sys.stdin:
    request = json.loads(line)
    try:
        say({"id": request["id"], "result": handle(request["op"], request)})
    except (ValueError, OSError) as e:
        say({"id": request["id"], "error": str(e)})
