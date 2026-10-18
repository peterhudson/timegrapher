# Timegrapher for macOS

This disk image holds everything in this release. It runs on Apple silicon
and Intel Macs with macOS 11 (Big Sur) or later.

| On this disk image | What it is |
|---|---|
| **Timegrapher.app** | The desktop app |
| **Applications** | A shortcut to your Applications folder, to drag the app onto |
| **Command Line** | `timegrapher`, the command line tool, and `timegrapher-mcp`, the server that lets AI chat apps use it |
| **Claude Desktop** | `Timegrapher.mcpb`, the Claude Desktop extension |
| **Documentation** | The guides, as pages that open in your browser |
| **LICENSE.txt** | The licence (GPL-3.0) |

## Open the App

1. Drag **Timegrapher.app** onto **Applications** in this window.
2. Open Timegrapher from Applications. This build is not signed with an
   Apple developer certificate, so macOS says *"Timegrapher" Not Opened*.
   Click **Done**.
3. Open **System Settings**, then **Privacy & Security**. Scroll down to
   *"Timegrapher" was blocked to protect your Mac* and click
   **Open Anyway**. Enter your password if asked, then click
   **Open Anyway** again.
4. When Timegrapher asks to use the microphone, click **Allow**.

macOS asks once for each new version; after that the app opens normally.
On macOS 14 and earlier, right-clicking the app, choosing **Open** and then
**Open** again does the same as steps 2 and 3.

## Use the Command Line Tool

The command line tool is also inside the app. To run it from Terminal by
name:

```sh
sudo ln -s /Applications/Timegrapher.app/Contents/MacOS/timegrapher /usr/local/bin/timegrapher
timegrapher --help
```

Or copy the **Command Line** folder somewhere you keep and run
`./timegrapher --help` there. Keep `timegrapher-mcp` beside `timegrapher`:
the server runs it.

Programs copied off the disk image meet the same check as the app. If macOS
says it cannot verify `timegrapher` or `timegrapher-mcp`, click **Done**,
then **Open Anyway** in **System Settings**, **Privacy & Security**, as in
step 3 above; each of the two programs asks once. Or clear the check for
both at once in Terminal: type `xattr -dr com.apple.quarantine ` (with the
space at the end), drag your copied **Command Line** folder onto the
Terminal window, and press Return.

## Add It to Claude Desktop

Double-click **Claude Desktop/Timegrapher.mcpb**, or drag it into Claude
Desktop's window, and choose **Install**. It holds both programs, so nothing
else needs installing. See [the MCP server guide](../../docs/mcp.md).

## Learn More

- [Installing and updating](../../docs/install.md)
- [The desktop app](../../docs/app.md)
- [Read Me](../../README.md): what the software measures, and the command line
- [All documentation](../../docs/index.md)
