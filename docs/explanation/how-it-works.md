# How bussard works

bussard keeps a KNX installation as text files and treats the devices as the
place those files get written to. This page explains the pieces and how a
command moves between them. The commands themselves are in the
[reference](../reference/cli.md); the files are specified in
[the model format](../reference/model-format.md).

## The model is the source of truth

A KNX device holds tables and parameter memory, but none of the meaning: no
room names, no group-address names, no record of why a parameter has its
value. ETS keeps the meaning in the project file, and an owner without that
file faces a reconstruction job. bussard keeps it in the model directory:

| File | Holds | Written by |
|---|---|---|
| `bussard.toml` | the gateway, lint settings | you |
| `groups.toml` | the group-address plan with names and data types | you, or `import` |
| `devices/<address>.toml` | one device: name, room, parameters, links per channel | you, or `import` and `adopt` |
| `bussard.lock` | what the device runs: product, application, derived keys | `import` and `adopt` only |

Everything but the lock is yours to edit, by hand or through an assistant.
The model is plain text, so a change is a diff, and the directory can live in
git or travel as a single bundle file. Nothing bussard writes to a device
changes the model: the model changes first, then a command carries it to the
device.

## Three layers of a device

A device's configuration has three parts of very different difficulty, and
bussard has a command for each:

| Layer | What it is | Commands |
|---|---|---|
| Individual address | `1.1.4` | `assign`, `adopt` |
| Links | the group-address, association and com-object tables | `plan`, `apply` |
| Parameters | channel modes, run times, alarm behaviour | `flash`, and `apply` for parameters that have a memory cell |

The address is a standard procedure that works on every device. The link
tables are standard loadable parts, written through documented interface
objects on System B devices. The parameters live in a memory layout that only
the vendor's product data describes, which is why the third layer needs a
`.knxprod` file.

## Product data makes parameters possible

A `.knxprod` is the vendor's product database. It describes the parameters
and their dialog, where each one sits in memory, the com-objects, and the
download procedure ETS follows for that device. bussard reads the file you
supply, generates a model of each application program under `.bussard/`, and
interprets the vendor's download procedure when it flashes. It never ships or
redistributes product data; see [product data](../reference/product-data.md).

The lock pins which application program each device runs, so a device file is
validated against the right dialog and a flash writes the right image.
`bussard device <address>` shows that dialog, page by page, as ETS would.

## The bus

bussard talks to the installation through a KNXnet/IP gateway, in tunnelling
or routing mode. Reading never changes a device: `monitor`, `read`, `scan`,
`describe` and `plan` decode and compare against the model and are never
gated. Writing is physical and is gated, so a write to a gateway that is not
on localhost needs an explicit opt-in. A tunnelling interface has a fixed
number of connections, and every running bussard command, ETS and a
visualisation each hold one.

## What a device write does

Every device write runs the same sequence, whether it starts on the CLI or in
the MCP programming tier:

1. read the device's identity and compare it with the lock,
2. read the live tables or parameters and show the difference to the model as
   a plan,
3. ask for confirmation, naming the gateway,
4. back up the device (not on `flash`, which rewrites a device wholesale),
5. write,
6. read back and compare with the model.

A result says "verified" only when the read-back matches. The exact rails of
each command, and what happens on a timeout or a refusal, are in
[Safety](safety.md).

## The assistant is an operator

`bussard mcp` exposes the model and the bus to an LLM assistant. The default
server reads: it answers "what do I have", decodes telegrams, and edits the
model. Every model edit is snapshotted first and rendered as a sentence, so
you judge a sentence and `undo` takes it back. Group writes need
`--allow-writes`, device programming needs `--allow-programming` and an
approved plan, and protected group addresses are refused over MCP without an
override. The split is deliberate: the assistant proposes and edits text, and
a human runs the physical steps or approves each one.

## Where ETS still fits

ETS remains the tool for certification, planning and the odd device bussard
cannot program. The two meet through files: `bussard import` reads an ETS
project export, `bussard diff` shows in sentences what a new export would
change, and `bussard keys export` writes a keyring ETS accepts. ETS signs its
project files, so bussard cannot write into a `.knxproj`; a change made in
bussard is carried to ETS by hand from the diff.
