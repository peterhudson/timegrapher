# Timegrapher for Linux

This folder holds everything in this release, for 64-bit Intel and AMD PCs
with a distribution from 2022 or later (Ubuntu 22.04, Debian 12, Fedora 36
or newer).

| In this folder | What it is |
|---|---|
| **Timegrapher.AppImage** | The desktop app, in one file |
| **Command Line** | `timegrapher`, the command line tool, and `timegrapher-mcp`, the server that lets AI chat apps use it |
| **Documentation** | The guides, as pages that open in your browser |
| **LICENSE.txt** | The licence (GPL-3.0) |

## Open the App

Double-click **Timegrapher.AppImage**. If your file manager does not run
it, open its **Properties**, then **Permissions**, tick *Allow executing
file as program*, and double-click it again. From a terminal:

```sh
chmod +x Timegrapher.AppImage
./Timegrapher.AppImage
```

If it says FUSE is missing, install `libfuse2` (Ubuntu 22.04:
`sudo apt install libfuse2`; Ubuntu 24.04: `sudo apt install libfuse2t64`)
or run it with `--appimage-extract-and-run`. Sound input goes through ALSA,
which PipeWire and PulseAudio systems provide through their ALSA plugin.

## Use the Command Line Tool

In a terminal in the **Command Line** folder, run `./timegrapher --help`.
Keep `timegrapher-mcp` beside `timegrapher`: the server runs it. The
AppImage carries the tool too: `./Timegrapher.AppImage cli --help`.

## Learn More

- [Installing and updating](../../docs/install.md)
- [The desktop app](../../docs/app.md)
- [The MCP server](../../docs/mcp.md), for AI chat apps
- [Read Me](../../README.md): what the software measures, and the command line
- [All documentation](../../docs/index.md)
