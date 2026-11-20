---
name: reverse-engineering
description: Use to look into a compiled program (a binary): list its functions, read them decompiled, follow references, name and comment them, with the project's Ghidra.
---
The project has one Ghidra analysis that the person and every bot share. Its tools start with `ghidra__`.

- A binary is added with `ghidra__add_binary` (a file you can read), or by copying it into `binaries/` in the project's folder. It is analysed when it arrives; `ghidra__list_binaries` says when it is `ready`. Until then the other tools refuse.
- `ghidra__list_functions`, `ghidra__list_strings` and `ghidra__list_imports` list at most 200 lines: narrow them with `filter`, or go on with `skip`.
- `ghidra__decompile` takes a function's name or any address in it. Functions Ghidra has not named are `FUN_<address>`; a function you rename keeps its address, so give addresses when you talk about functions.
- `ghidra__find_references` takes a function or an address and says where it is referred to from.
- `ghidra__rename_function` and `ghidra__comment` change the shared analysis at once: the person's code view shows them, and so does every bot's next `ghidra__decompile`. Comments the person writes in Ghidra's own window (open it only after the workplace is switched off) are in the decompiled code the same way.
- Read what is already named and commented before you add: other hands were here first.
