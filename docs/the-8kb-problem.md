# The 8 KB problem: why your MCP server hangs, and how to stop it

*Draft — patchwright, 2026. Companion writeup for [mcpdrain](https://github.com/patchwright/mcpdrain).*

You wire up a local MCP server. It works in the inspector. Then you point a real
client at it and, every so often, it just... stops. No error. No crash. No log
line. The client sits there waiting for a response that never comes, and the
server process is alive but doing nothing. Restart it and it works again — until
it doesn't.

If you have hit this, you have hit the 8 KB problem. It is not your code. It is a
40-year-old corner of how Unix pipes work, and MCP walks straight into it.

## A 30-second reproduction

An MCP server that talks over stdio is just a child process. The client writes
JSON-RPC to its stdin and reads JSON-RPC from its stdout. Now give it a server
that logs to stderr and returns a big `tools/list`:

```
# the server prints a few hundred KB to stderr while answering,
# and a 65 KB tools/list to stdout
client → spawn server → write {"method":"tools/list"} → read stdout …forever
```

The client never finishes reading. The server never finishes writing. Both are
blocked, each waiting on the other. That is a deadlock, and nobody printed a
single error to tell you so.

## What is actually happening

A pipe is not an infinite tube. It is a fixed-size kernel buffer. On Linux the
default is 64 KiB; historically it was a single 4 KiB page, and on some systems
the practical figure people quote is 8 KB — hence the name. You can read your
own with `fcntl(fd, F_GETPIPE_SZ)`.

Two rules follow from "fixed-size":

1. If you **write** to a pipe whose buffer is full, your `write()` call blocks
   until someone reads from the other end.
2. If nobody ever reads that end, your `write()` blocks **forever**.

Now replay the MCP exchange with those rules in mind. The single most common
version has nothing to do with the size of your response — it is **stderr**:

- The client spawns the server and reads its **stdout** for responses.
- The client does **not** read the server's **stderr**. Most clients don't;
  stderr is "just logs."
- The server logs to stderr while it works. Those logs fill the 64 KiB stderr
  pipe. The next log call **blocks**.
- A blocked log call means the server's request handler can't return, so it
  never finishes writing the response to stdout.
- The client waits forever for a stdout response the server can't send.

Total silence. The server is "running." Nothing is wrong with the protocol, the
JSON, or your tool code. The pipe filled up and the kernel did exactly what it
is documented to do.

The large-`tools/list` variant is the same trap from the other side: a 65 KB
response is bigger than the 64 KB buffer, so the server can't write it in one
shot, and if the client drains it in a way that ever stops to do something else,
the two can wedge. (A real case: ruflo #2426, a 65,747-byte `tools/list` against
a 64 KiB pipe.)

## The fix that doesn't work

The first suggestion you will hear is "chunk your JSON-RPC into messages under
8 KB." It sounds right and it is wrong. A JSON-RPC response is one message. You
cannot split one response across several — the client is parsing a single JSON
document and will reject a half of one. The buffer limit is on the **transport**,
not on the message, so the message can't be the thing you shrink.

The other non-fix is "just make the buffer bigger." You can grow a pipe with
`F_SETPIPE_SZ`, but you are only moving the cliff: a chattier server or a bigger
tool list walks off the new edge. The deadlock is structural, not a tuning knob.

## The fix that works

The deadlock only exists because *something stops reading*. So never stop
reading. Concretely:

- **Drain every stream, always, on its own reader.** stdout, and — the part
  everyone forgets — **stderr**. An undrained stderr is the number-one cause of
  these hangs, so a fix that only handles stdout doesn't actually fix it.
- **Decouple "read from the server" from "write to the client."** Read the
  server as fast as it will talk, into a bounded in-memory spool, and write to
  the client independently. The server is never throttled by a slow client.
- **Detect a genuine stall, not idle.** A client that is simply quiet is normal;
  a client that has wedged is not. A timed write tells the two apart, so a truly
  stuck client can be surfaced or the server restarted instead of hanging forever.

That is all [mcpdrain](https://github.com/patchwright/mcpdrain) does. It is a
single binary you drop between the client and any stdio MCP server:

```bash
mcpdrain run -- your-mcp-server --its --args
```

It spawns your server, drains stdout and stderr concurrently, spools, and
forwards — transparently, so the client and server are unchanged. It adds no
tools and speaks no protocol of its own; it just keeps the pipes from filling.

It can also tell you what you are up against:

```bash
$ mcpdrain doctor
{"pipe_capacity_bytes":65536}
OS pipe capacity ≈ 65536 bytes (64 KiB). Any single MCP response larger than
this can deadlock a client that does not drain stderr concurrently.
```

## Scope, honestly

v0.1 is deliberately small: `run` and `doctor`, Linux and macOS, Apache-2.0. No
Windows named pipes yet, no replay, no plugins. It does one boring thing and does
it correctly, with a test that floods 256 KiB to stderr and proves a non-draining
client hangs while mcpdrain delivers the full response.

```bash
cargo install mcpdrain
# or: curl -fsSL https://raw.githubusercontent.com/patchwright/mcpdrain/main/install.sh | sh
```

If your local MCP setup hangs once a day and you have been blaming your server,
check the pipes first. It is almost always the pipes.
