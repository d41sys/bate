# Workplaces

A workplace is a package that equips a project for one kind of work: a program that Bate runs once for the project, with the skills and roles that go with it. The idea is in `docs/adr/0001-workplaces-are-separate-programs.md`; the requests are in `PROTOCOL.md` (Workplaces). A small working example is `examples/workplaces/counter/`.

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
  "needs": { "folders": [], "hosts": [], "commands": ["python3"] }
}
```

| Key | Value |
|---|---|
| `name` | Required. The name of the workplace and of the connector its program is: 1 to 40 characters of `a-z`, `0-9`, `_` and `-`, without `__`. Its tools reach the bots as `<name>__<tool>`. |
| `version` | Required. One line, at most 40 characters. Bate shows it and does not compare versions. |
| `description` | Optional. One line, at most 500 characters. |
| `program.command` | Required. A path inside the workplace's folder (no leading `/`, no `..`), which must be there and be runnable, or a bare name such as `python3` that is looked up on the `PATH` Bate runs with. |
| `program.args` | Optional list of arguments. A relative path in them starts in the workplace's folder. |
| `needs.folders`, `needs.hosts`, `needs.commands` | Optional lists (at most 50 entries of one line, 200 characters each): the folders, network hosts and commands the workplace uses. The window shows them before a person switches the workplace on, and Bate stores them. Bate does not yet enforce the hosts. |

Anything else in the file is ignored. The file is at most 64 KiB.

## The program

Bate starts it once for each project that has the workplace on, under the same folder limit as a project's own connector (`PROTOCOL.md`, A project's connector): it serves MCP over HTTP on the unix socket named in `BATE_MCP_SOCKET`, and ends when its standard input closes. Its working directory is the workplace's folder, which it can read but not write. It writes only the data folder that Bate keeps for the project (`project-data/<projectId>/`, where the socket is) and the project's folder, which is `BATE_WORK_DIR`. Every bot of the project gets its tools, and all of them reach the same program, so they share one state.

## Installing, switching on, removing

- **Install** (Projects tab, Workplaces; or `install_workplace`): Bate copies the folder as it copies a skill: into a staging folder first, no links or special files, at most 2000 files and folders, 200 MiB and 12 folders deep. A malformed manifest or a program that is missing is refused with the reason. A workplace of the same name is replaced, unless a project has it on.
- **Switch on** (in the project; or `workplace` in `set_project`): the window shows what the workplace needs and asks first. A project has a workplace *or* a connector of its own, not both. The workplace's skills are offered to every bot of the project, from the workplace's own folder (nothing is copied into the bots' folders). Its roles are added through "Add bots from role files", whose paths the list shows.
- **Change on disk:** Bate keeps a SHA-256 fingerprint of the installed files. When they differ (checked when the program starts and whenever the list is shown), the workplace is switched off in every project: the program stops, the project says why, and the bots go on without its tools and skills. After you have looked at the files, **Approve** records them as they are and the program starts again.
- **Switch off:** choose no workplace in the project. The program stops, and each bot loses its tools and skills at its next turn.
- **Remove:** refused while a project has the workplace on, unless you confirm; the project is then switched off. What the program kept in `project-data/<projectId>/` is never deleted by this.

A workplace that cannot be used (its manifest no longer parses, its program does not start) is reported in the list and in the project; the project still opens and its bots still answer.
