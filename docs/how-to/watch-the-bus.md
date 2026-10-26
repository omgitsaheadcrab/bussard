# Watch the bus

Watch telegrams as they happen, read a single value, see the installation in a browser, and keep a history you can query later. Everything on this page only listens, except `read`, which asks one device for one value.

## Watch and decode the bus

```console
$ bussard monitor
12:03:41.221  1.1.7 Weather Station    → 3/0/0 Wind Alarm           = Alarm (1.005, obj "Wind Alarm")
12:03:44.809  1.1.1 Push Button Hallway → 0/0/0 Hallway Light Switch = On (1.001, obj "Rocker 1")
12:03:44.981  1.1.3 Switch Actuator     → 0/0/1 Hallway Light Status = On (1.001)
```

Each line is one telegram: the sending device, the group address it went to (a group address such as `0/0/0` is the shared channel a button and an actuator both use), its name, and the decoded value. Flip a switch and watch it name itself.

Narrow the stream with `--filter 3/2/0,1/0/`: group addresses, group-address prefixes, or sender addresses such as `1.1.30`. `--json` prints one JSON object per line for other tools. A group address the model does not know shows as raw hex; add it to `groups.toml` with a `dpt` (its data type, which tells bussard how to decode the value) and run `monitor` again.

Secured telegrams (KNX Data Secure) decode only with the keyring: pass `--keyring <file.knxkeys>` and set the password in `BUSSARD_KEYRING_PASSWORD`. Decoded ones are marked `[secured]`.

## Read a value

```console
$ bussard read 3/2/0
```

`read` sends one read request to the group address and prints the typed answer. It exits non-zero when nothing answers within 3 seconds. `--json` prints the result as one object, with `answered` set to `false` when no answer came.

A secured group address needs `--keyring`. Without a key for it, `read` refuses before sending anything.

## See the installation in a browser

```console
$ bussard viz
bussard viz serving on http://127.0.0.1:8080 (Ctrl-C to stop)
read-only: group writes return 403 (pass --allow-writes to enable them)
```

Open the address. You get every device drawn along the bus by floor and room, the group-address tree, and live telegrams as they happen. Select a device or a group address to see its links, senders and listeners. With no reachable gateway the page still shows the model, without live traffic.

A bare `bussard viz` never transmits, so it is safe to point at the live installation. It holds one tunnelling slot of your interface while it runs (see [Find what you have](find-what-you-have.md)).

!!! warning
    `--allow-writes` arms test-write buttons on the page, and `--watch-prog` sends a programming-mode query on the bus at intervals. Both transmit. Against a gateway that is not on your own machine the server refuses to start without `--allow-remote-gateway`. Every armed write asks you to confirm the group address and the gateway, and a `protected` group address needs a force checkbox on top.

## Capture a history and query it

```console
$ bussard capture --to knx/captures/bus.db --filter 3/
```

`capture` writes every matching telegram into a SQLite file, created if absent. It stores the raw bytes next to the decoded value, so you can decode the history again after you fix a name or a type in the model. `--filter` works as for `monitor`, and `--keyring` decrypts secured telegrams.

Query the file with any SQLite tool:

```console
$ sqlite3 knx/captures/bus.db \
    "SELECT ts_utc, source, destination, apci FROM telegrams
     WHERE destination = '3/2/0' ORDER BY ts_utc DESC LIMIT 10"
```

Or give it to the assistant, which then sees telegrams older than its own memory holds:

```console
$ claude mcp add knx -- bussard mcp --dir knx --capture-db knx/captures/bus.db
```

The table layout is in the [JSON and capture reference](../reference/json-output.md).

## Name group addresses from live traffic

```console
$ bussard learn --untyped --gateway 192.0.2.10

[1/12] 1/0/1: trigger the object you want to name (waiting up to 30s)
  1/0/1  sender 1.1.30 Schaltaktor (Kitchen)
  com object 3 Kanal A - Schalten, channel Ceiling light, declares 1.001
  payload 01 (1 byte(s))
    1. 1.001 [high] the sending com object declares DPT 1.001 in the model
  proposed name: Kitchen ceiling light, schalten
  accept as "Kitchen ceiling light, schalten" / 1.001? [a]ccept, [e]dit name, [d]pt, [s]kip, [q]uit:
```

`learn` asks you to press a button, waits for the telegram, and proposes a type and a name from the sending device's room, channel and com object (one input or output of a device). Accept, edit the name, change the type, skip, or quit. Press the same button again when the candidates are uncertain: each extra telegram narrows them.

| Flag | What it learns |
|---|---|
| `--untyped` | Every group address in the model without a type |
| `--unnamed` | Every group address with a placeholder name |
| `--ga <GA>` | The addresses you name, in that order (repeatable) |
| none | Whatever appears on the bus next, until a wait times out |

Accepted answers go into `groups.toml`, and into the sending device's file when the com object is clear. `learn` never transmits; it only changes files. Review the result and run `bussard validate`. `--timeout` changes the 30-second wait.

## Next steps

- [Find what you have](find-what-you-have.md): list devices, gaps in the model, and what bussard can do with each device.
- [Change what a button does](change-a-link.md): the first write, with plan and undo.
- [Bus commands reference](../reference/commands/bus.md): every flag of `monitor`, `capture`, `read` and `learn`.
