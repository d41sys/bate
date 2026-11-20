# Workplaces

A workplace is a package that equips a project for one kind of work: a program that Bate runs once for the project, with the skills and roles that go with it. The idea is in `docs/adr/0001-workplaces-are-separate-programs.md`; the requests are in `PROTOCOL.md` (Workplaces). A small working example is `workplaces/counter/`; to install one from a list, see The catalog.

## The folder

```
my-workplace/
  workplace.json      required
  server.py           the program, or whatever the manifest names
  skills/<name>/SKILL.md     optional: skills offered to the project's bots
  roles/<name>.md            optional: roles (see docs/roles.md) that the project can add as bots
```

## `workplace.json`

```json
{
  "name": "counter",
  "version": "1.0.0",
  "description": "A counter that every bot of a project shares.",
  "program": { "command": "python3", "args": ["server.py"] },
  "needs": { "folders": [], "hosts": [], "commands": ["python3"] },
  "downloads": [
    { "url": "https://example.com/model.bin", "size": 123456789, "sha256": "<64 hex digits>", "dest": "data/model.bin" },
    { "url": "https://example.com/tools.tar.gz", "size": 2345678, "sha256": "<64 hex digits>", "dest": "tools", "unpack": "tar.gz" }
  ]
}
```

| Key | Value |
|---|---|
| `name` | Required. The name of the workplace and of the connector its program is: 1 to 40 characters of `a-z`, `0-9`, `_` and `-`, without `__`. Its tools reach the bots as `<name>__<tool>`. |
| `version` | Required. One line, at most 40 characters. Bate shows it and does not compare versions. |
| `description` | Optional. One line, at most 500 characters. |
| `program.command` | Required. A path inside the workplace's folder (no leading `/`, no `..`), which must be there and be runnable, or a bare name such as `python3` that is looked up on the `PATH` Bate runs with. |
| `program.args` | Optional list of arguments. A relative path in them starts in the workplace's folder. |
| `needs.folders`, `needs.hosts`, `needs.commands` | Optional lists (at most 50 entries of one line, 200 characters each): the folders, network hosts and commands the workplace uses. The window shows them before a person switches the workplace on, and Bate stores them. Bate enforces the hosts: the program reaches no network host but those, and cannot widen them (see Network below). |

| `downloads` | Optional list of at most 20 files to fetch at install (see Downloads). |

Anything else in the file is ignored. The file is at most 64 KiB.

## Downloads

Files too big to ship in the folder are listed under `downloads`. Each entry has:

| Key | Value |
|---|---|
| `url` | Required. `https://…` (or `http://127.0.0.1…`, to this Mac), no user name. Up to 5 redirects are followed (GitHub serves release files from another host), so a host Bate shows may send it on to another; the hash, not the host, is what makes the file trusted. |
| `size` | Required. The size in bytes, 1 to 16 GiB. A download that sends more stops at once; one that sends less is refused. |
| `sha256` | Required. 64 lowercase hex digits. |
| `dest` | Required. A path inside the workplace's folder (no leading `/`, no `..`), different for each entry, and not a name the workplace's own files use. Folders on the way are made. |
| `unpack` | Optional: `tar.gz`. The archive is unpacked into the folder `dest` instead of being saved as a file. It is the only format: Bate unpacks it with the system's `/usr/bin/tar`, as it does for its own updates. Make the files that must run executable in the archive. |

Before it installs a workplace with downloads, the window shows how much it will download and from which hosts, and asks. Nothing is contacted until the person agrees. Then Bate fetches the files one after the other, with a progress line, into the same staging folder the workplace is copied to, and checks each as it arrives: more than `size` bytes, a different length, a wrong hash, more than 5 redirects, an HTTP error or a broken connection stops the install, and everything fetched so far is deleted. Nothing is installed in that case, and nothing is resumed: install again to start over. An archive is unpacked only after its hash matched, and only if it holds nothing but plain files and folders: a link, an absolute path or one with `..` is refused, so unpacking cannot write outside the workplace's folder.

The downloaded files are part of the workplace's folder: they count toward its limit of 2000 files and folders and 12 levels (not toward the 200 MiB of the copy), and the fingerprint covers them, so a change to one switches the workplace off until it is approved. Bate does not check the free disk space, nor cap the size an archive unpacks to.

A workplace has no signature: only the hashes in its manifest, which the person chose to install, pin its downloads. App updates are another matter: they are accepted only with an Ed25519 signature over `bate-update\n<version>\n<sha256>` by the key built into Bate, from Bate's own feed address, so nothing a workplace lists or downloads, a signed release included, counts as an update.

## The program

Bate starts it once for each project that has the workplace on, under the same folder limit as a project's own connector (`PROTOCOL.md`, A project's connector): it serves MCP over HTTP on the unix socket named in `BATE_MCP_SOCKET`, and ends when its standard input closes. Its working directory is the workplace's folder, which it can read but not write. It writes only the data folder that Bate keeps for the project (`project-data/<projectId>/`, where the socket is) and the project's folder, which is `BATE_WORK_DIR`. Every bot of the project gets its tools, and all of them reach the same program, so they share one state.

### Network

The program is held to the hosts in `needs.hosts`, by the system and not by its good behaviour (`PROTOCOL.md`, Network limit). Each is `host`, `*.domain` (the subdomains of `domain`) or either with `:port`; with no port, 80 and 443. A host that is written some other way makes the manifest malformed. With none listed, the program reaches no host at all; it still serves its socket.

Bate sets `HTTP_PROXY`, `HTTPS_PROXY` and `ALL_PROXY` (and the lower-case names) for the program to a proxy of its own, which lets through only those hosts. A program that opens connections itself, ignoring those settings, is refused by the system. The downloads are fetched by Bate at install, not by the program; their hosts are shown for the person to agree, and are not added to what the program may reach. The program cannot widen the list: only the manifest the person saw and approved sets it.

## Surfaces

A program may fill **surfaces**: a table, a tree or a code view that Bate's window draws in its pane while the person and the bots work on the same data. A surface is an MCP resource, so the program needs `resources` in its `initialize` answer's `capabilities` (add `subscribe` and `listChanged` to be heard when things change) and answers `resources/list` and `resources/read`. What makes a resource a surface is its `mimeType`:

| `mimeType` | Kind | Content (JSON in the resource's `text`) |
|---|---|---|
| `application/vnd.bate.table+json` | table | `{"columns": ["Tick"], "rows": [{"id": "1", "cells": ["1"]}]}` |
| `application/vnd.bate.tree+json` | tree | `{"nodes": [{"id": "g0", "label": "ticks 1 to 10", "depth": 0}, {"id": "1", "label": "tick 1", "depth": 1}]}` |
| `application/vnd.bate.code+json` | code | `{"language": "json", "lines": ["{", "  \"count\": 3", "}"]}` |

Everything is data: Bate shows every string as plain text, so markup and script in it do nothing. Large content is read in parts: a `resources/read` carries `params._meta["bate/part"] = {"offset": 0, "limit": 100}`. Send just those items, with `"offset"` and `"total"` in the content, or ignore it and send everything: Bate cuts the part itself (at most 500 items, 512 KiB) and never loads more.

The person's actions are tool calls. Name them in the content: `"select"` (a tool called when a row, node or line is chosen), `"open"` (when it is opened: double click or Enter) and `"actions"`, a list of `{"tool", "label"}` shown as buttons. Bate calls the tool with `{"uri": <the surface>, "target": <the id of the row or node, or the line's number>}`; it calls only tools that the surface names. Change the content in the tool (or when a bot calls any tool) and send `notifications/resources/updated` for the uri on the `GET /mcp` stream of server-sent events: the open surface updates in every window. `workplaces/counter/` has one surface of each kind. The wire details are in `PROTOCOL.md` (Surfaces). The web client on the local network has no surfaces.

## Installing, switching on, removing

- **Install** (Projects tab, Workplaces; or `install_workplace`): Bate copies the folder as it copies a skill: into a staging folder first, no links or special files, at most 2000 files and folders, 200 MiB and 12 folders deep, then fetches its downloads there. A malformed manifest or a program that is missing is refused with the reason. A workplace of the same name is replaced, unless a project has it on.
- **Switch on** (in the project; or `workplace` in `set_project`): the window shows what the workplace needs and asks first. A project has a workplace *or* a connector of its own, not both. The workplace's skills are offered to every bot of the project, from the workplace's own folder (nothing is copied into the bots' folders). Its roles are added through "Add bots from role files", whose paths the list shows.
- **Change on disk:** Bate keeps a SHA-256 fingerprint of the installed files. When they differ (checked when the program starts and whenever the list is shown), the workplace is switched off in every project: the program stops, the project says why, and the bots go on without its tools and skills. After you have looked at the files, **Approve** records them as they are and the program starts again.
- **Switch off:** choose no workplace in the project. The program stops, and each bot loses its tools and skills at its next turn.
- **Remove:** refused while a project has the workplace on, unless you confirm; the project is then switched off. What the program kept in `project-data/<projectId>/` is never deleted by this.

A workplace that cannot be used (its manifest no longer parses, its program does not start) is reported in the list and in the project; the project still opens and its bots still answer.

## The catalog

Bate's own list of workplaces is the file `catalog/workplaces.json` in this repository. In the window (Projects tab, Workplaces, Catalog) a person asks for it, reads the entries and installs one; nothing is contacted before that, and no server of the project is involved: the list is read from GitHub's raw files and the archives from wherever the entries say (GitHub releases, for Bate's own). A workplace that is not in the list is installed from a folder, as above.

```json
{
  "workplaces": [
    {
      "name": "counter",
      "description": "A counter that every bot of a project shares.",
      "version": "1.0.0",
      "url": "https://github.com/d41sys/bate/releases/download/wp-counter-v1.0.0/counter-1.0.0.tar.gz",
      "size": 20503,
      "sha256": "<64 hex digits>"
    }
  ]
}
```

| Key | Value |
|---|---|
| `name` | The workplace's `name` (rules as in `workplace.json`), different for each entry. |
| `description` | One line, at most 500 characters. |
| `version` | One line, at most 40 characters: the `version` of the `workplace.json` inside. |
| `url` | `https://…` (or `http://127.0.0.1…`, to this Mac) of a `.tar.gz` of the workplace's folder: `workplace.json` and the rest at the top of the archive. At most 5 redirects are followed. |
| `size`, `sha256` | The archive's size in bytes (1 to 200 MiB) and SHA-256 (64 lowercase hex digits). |

At most 200 entries, 1 MiB. A list with one bad entry is refused whole.

**Installing.** Bate downloads the archive into its staging folder, stops at `size` bytes, and refuses it unless the size and the hash are the entry's and the `workplace.json` inside has the entry's `name` and `version`. It unpacks it with the checks a download's `tar.gz` gets (plain files and folders, nothing outside the folder), shows the manifest's needs, and then installs the folder as it installs one a person picked: the same limits, the same `downloads` with the same consent, the same fingerprint. Whatever fails leaves nothing behind. There is no signature: the pin of size and hash is what makes the archive the one the person was shown.

**Pinned.** An installed workplace keeps the version and the archive's hash it was installed at. When the catalog lists another archive for its name, the window offers **Update**, and shows what the new version needs that the installed one did not: folders, hosts, commands and downloads. It is applied only when the person agrees, and never while a project has the workplace on (switch it off first). A workplace installed from a folder has no pin; the catalog's entry of that name is offered to it.

### Releasing one of Bate's workplaces

A workplace of Bate's lives in `workplaces/<name>/`. To release version `X` of `<name>` (its `workplace.json` says `X`):

1. Pack it and read the size and hash: `python3 workplaces/pack.py workplaces/<name> <name>-X.tar.gz`. The script prints `<size> <sha256>`. The same folder gives the same bytes on every machine (members in order of their paths, no owner, time zero, modes 755 and 644, a gzip layer of stored blocks that no compressor version can change; `.DS_Store` and `__pycache__` are left out), so it can be done locally.
2. Put the entry in `catalog/workplaces.json`, with `url` `https://github.com/d41sys/bate/releases/download/wp-<name>-vX/<name>-X.tar.gz`, and merge it. (`cargo test` fails when an entry does not match the pack of its folder.)
3. Push the tag `wp-<name>-vX` on the merged commit. The job in `.github/workflows/ci.yml` packs the folder with the same script, checks that size, hash, version and `url` are the catalog entry's, and creates a release of that name with the archive, with `--latest=false`: the updater reads `releases/latest/download/feed.json`, so a workplace release must never become the latest one. It then checks that the release is not the latest.

The tag is `wp-…`, never `v…`: `v*` tags publish the app.
