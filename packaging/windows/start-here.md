# Timegrapher for Windows

This folder holds everything in this release, for 64-bit Windows 10 or 11.
Timegrapher is a standalone app: there is nothing to install. Extract the
whole folder somewhere you keep, such as `Documents`, and run it from
there. To remove it, delete the folder.

| In this folder | What it is |
|---|---|
| **Timegrapher.exe** | The desktop app |
| **Command Line** | `timegrapher.exe`, the command line tool, and `timegrapher-mcp.exe`, the server that lets AI chat apps use it |
| **Claude Desktop** | `Timegrapher.mcpb`, the Claude Desktop extension |
| **Documentation** | The guides, as pages that open in your browser |
| **LICENSE.txt** | The licence (GPL-3.0) |

## Open the App

1. Double-click **Timegrapher.exe**. This build is not signed with a
   Microsoft developer certificate, so Windows says *Windows protected your
   PC*. Click **More info**, then **Run anyway**.
2. If Windows asks whether Timegrapher may use the microphone, allow it. If
   no input is found later, open **Settings**, **Privacy & security**,
   **Microphone**, and turn on *Let desktop apps access your microphone*.

Windows asks once for each new version. To pin the app, right-click
**Timegrapher.exe** and choose **Pin to Start**.

## Use the Command Line Tool

Open a terminal in the **Command Line** folder and run
`.\timegrapher.exe --help`. Keep `timegrapher-mcp.exe` beside
`timegrapher.exe`: the server runs it.

## Add It to Claude Desktop

Double-click **Claude Desktop\Timegrapher.mcpb**, or drag it into Claude
Desktop's window, and choose **Install**. It holds both programs, so nothing
else needs installing. See [the MCP server guide](../../docs/mcp.md).

## Learn More

- [Installing and updating](../../docs/install.md)
- [The desktop app](../../docs/app.md)
- [Read Me](../../README.md): what the software measures, and the command line
- [All documentation](../../docs/index.md)
