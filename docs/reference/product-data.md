# Product data (`.knxprod`) and generated models

`bussard` reads vendor product data to learn a device's com-object table, parameters,
and how to download a configuration into it. This document covers the `.knxprod` format,
what `bussard import-product` does with it, the model files it generates, and the
licensing and versioning caveats. The command's flags are in
[reference.md](cli.md#bussard-import-product-file); a worked adopt-a-device recipe
is in [howto.md](../how-to/recipes.md#-add-a-brand-new-device).

## Where product data comes from

A `.knxprod` is a manufacturer's product database for one or more devices. You obtain it
from:

- the manufacturer's service/download site (most publish `.knxprod` files free),
- the MyKNX catalogue (knx.org), or
- an ETS export of a product you already have.

bussard reads these files; it never ships or redistributes them. See
[the never-redistribute rule](#the-never-redistribute-rule) below.

To save you hunting for the right file, bussard ships a
[pointer index](#the-product-data-pointer-index) that maps an order number to where its
`.knxprod` can be downloaded: the URL and a checksum, never the file itself.
`bussard import-product --order-number <ORDER>` uses it to fetch and verify the file for
a device you saw on the bus.

### The ETS project export as product source

An ETS 6 project export (`.knxproj`) carries the converted ApplicationProgram XML of
every product used in the project, unencrypted under `M-<manufacturer>/`. This is the
only route for devices whose vendor ships product data solely as ETS3-era `.vd4`
files, and it is convenient whenever the project already holds the exact application
version a device runs. `bussard import-product <project>.knxproj` and `bussard import
<project>.knxproj` extract each application program the model needs from that XML,
once, into its own archive under `<dir>/products/` (see
[the product store](#the-product-store-products)). After that no command reads the
export again. `bussard flash <ia> --product <project>.knxproj` still reads an export directly;
the program is the one the lock pins for the device, and `--application
<application-id>` is needed only when the lock pins none. The generated models are byte-for-byte the same as from the
vendor `.knxprod` when the versions match: two ETS3-era Jung devices were flashed from
their project export and verified against ETS captures.

The export itself is never copied into the model: it is the owner's project, and large.
The extracted archive holds only `knx_master.xml`, the manufacturer's `Hardware.xml`
and `Catalog.xml`, and the program (plus the programs its `Hardware2Program` lists
next to it), copied byte for byte into a plain ZIP with fixed timestamps. A flash
image built from the extracted archive is byte-identical to one built from the export
(checked on 1.1.52, `M-0004_A-A012-12-D63A-O000A`, and on the Data Secure device
1.1.12, `M-0004_A-D141-22-151B-O000A`). Extracting the same program from the same
export again gives the same bytes, so a re-import does not change the lock.

### Reading product data at flash time

`flash`, `plan`, `reconstruct` and `adopt` read an archive in two steps. First the
table of contents: the ApplicationProgram ids from the entry names
(`M-XXXX/M-XXXX_A-….xml`), every `Hardware.xml` and `knx_master.xml`. Then only the
program the command selects, by `--application`, the model's application reference or
the order number, together with the programs its `Hardware2Program` lists next to it.
A 15 MB project export with 30 programs costs one program parse instead of 30. When the
selection cannot be made from the ids alone, every program is parsed and the command
decides as before.

The table of contents and each parsed program are cached as JSON under
`<dir>/.bussard/products/`, keyed by the SHA-256 of the archive and the bussard build, so
the next command reads them back instead of inflating and parsing the XML. A cached
program is the same value a fresh parse returns (floats travel as their bit pattern), so
the images are byte-identical. See [reference.md](cli.md#bussardproducts) for the
layout and `BUSSARD_PRODUCT_CACHE=off`.

## What is inside a `.knxprod`

A `.knxprod` is a plain ZIP (no encryption, unlike a password-protected `.knxproj`). It
contains:

```
knx_master.xml            # global enums: manufacturers, mask versions, DPTs
M-XXXX/                    # one folder per manufacturer id, e.g. M-0004 (Jung)
  Hardware.xml            # order numbers → Hardware2Program → application refs
  Catalog.xml             # catalogue tree (not used by bussard)
  M-XXXX_A-….xml          # one ApplicationProgram per programmable variant
  *.signature             # RSA signatures, ignored (see below)
```

The signature files authenticate the archive to ETS. They are not access control: they
stop third parties from forging files ETS will accept, but they do not stop reading a
legitimately downloaded file. bussard skips them.

### The ApplicationProgram XML

Each `M-XXXX_A-….xml` is the interesting part. These files are large (20-28 MB is
normal), so bussard streams them with `quick-xml` and never builds a DOM. It extracts:

- Identity: `Id`, `ApplicationNumber`, `ApplicationVersion`, `MaskVersion`, `Name`
  (resolved to `en-US` where a translation exists), `LoadProcedureStyle`, and the XML
  schema version.
- Com-objects: the `ComObject` base table and `ComObjectRef` overrides, resolved into
  effective number / DPT / flags / size / text, exactly as the `.knxproj` importer does
  (the two agree).
- Parameter types: `TypeNumber` (int min/max/size), `TypeRestriction` (enum value/text
  pairs), `TypeText` (string length), `TypeFloat`, `TypeNone`, and any other `Type*`
  element preserved by name. A `TypeText` value is written in the code page the
  program declares in `<Options TextParameterEncoding>` (`iso-8859-15`, `utf-8`, ...),
  padded with NUL to its field; a character outside that code page, or a text longer
  than the field, is refused. A program that declares no encoding takes ASCII text
  only, because no capture shows ETS's choice for one.
- Parameters and parameter refs: name, text, type ref, default value, access, and the
  memory location (`CodeSegment` + `Offset` + `BitOffset`). A parameter-ref
  `Value`/`Access` overrides the parameter's own.
- Code segments: `RelativeSegment` / `AbsoluteSegment` metadata only (id, size,
  address/offset, load-state-machine). The binary payload is dropped.
- Load procedure: the `LdCtrl*` control script (`LdCtrlConnect`, `LdCtrlUnload`,
  `LdCtrlLoad`, `LdCtrlWriteRelMem`, `LdCtrlTaskSegment`, `LdCtrlLoadCompleted`,
  `LdCtrlRestart`, ...), parsed into a typed op list. Unknown ops are kept verbatim so
  nothing is lost. The downloader (`bussard-download`) interprets this list for
  `bussard flash`.

## The product store (`products/`)

`<dir>/products/` holds every product archive the model depends on: vendor `.knxprod`
files as downloaded or supplied, and the archives extracted once from an ETS export.
It is retained data, not a cache. Vendor downloads can disappear and the export is used
once, so bussard never regenerates anything in it. File names stay readable: the
vendor's file name, or `<application-id>.knxprod` for an extracted program. A
different file under a name already taken is stored with the first eight hex digits of
its SHA-256 appended; nothing in the store is overwritten.

`bussard.lock` records each archive as a `[[product]]` entry (`file`, `sha256`,
`origin`, the applications and order numbers it carries) and links each device to one
(see [the model format](model-format.md#bussardlock)). Every use checks the file's
SHA-256 against the lock before the archive is read.

**Committing it is your decision.** bussard does not git-ignore `products/`. It holds
copyrighted vendor data, so a public repository should not carry it; a private one
usually does, because a clone without it cannot flash, apply parameters, commission
with `--flash` or replace a device until the archives are back.

**A missing or changed archive** refuses `flash`, `apply` (the parameters),
`commission --flash` and `replace` before anything reaches the bus, with the lock's
record and the way back, for example:

```
product data for 1.1.12 (52911ST, products/M-0004_A-D141-22-151B-O000A.knxprod, sha256 9f1c0a3b7d2e…) is missing; restore it from your backup or version control, or re-import the ETS export: bussard import ../house.knxproj
```

The recovery depends on the origin: restore the file from a backup or version control;
for origin `index`, `bussard import-product --order-number <ORDER>`; for origin
`file`, `bussard import-product <file>`; for origin `knxproj`, `bussard import
<export>`. An archive whose SHA-256 differs from the lock's is refused naming both
hashes; `bussard import-product <file>` re-pins it, which is a reviewable lock change.
`plan` and `reconstruct` warn and read the links only.

**Overrides.** Every command that needs product data (`flash`, `apply`, `plan`,
`reconstruct`, `commission --flash`, `replace`, `adopt`) resolves it from the lock
and the store; `--product` and `--application` stay as overrides. One that
contradicts the lock (another archive hash, another application) is refused by
`flash`, `apply` and `replace` unless `--force`, which uses it and pins the archive
for the device; `plan` and `reconstruct` warn. `replace` needs no `--product`: the
device's order number comes from the model and its application from the lock.
`flash --product <export>.knxproj` needs no `--application` when the lock pins the
device's program. `adopt` given an export stores it the way `import-product` does.

**Migration.** A model directory from an earlier bussard kept its archives in
`vendor/`. The first command that reads the model moves every `vendor/*.knxprod` into
`products/`, pins it (origin `index` when the pointer index knows its hash, else
`file`), links the devices whose order number its catalogue carries, removes the empty
`vendor/` with its old `.gitignore`, and generates `.bussard/models/`. A device whose
product data only existed in an ETS export gets its archive on the next `bussard
import` of that export.

## What `import-product` writes

In positional mode (`bussard import-product <file>`) the command:

1. reads the `.knxprod`, or the `.knxproj` export,
2. stores a `.knxprod` byte-identically under `<dir>/products/<original-name>` (an
   identical file already there is kept); from an export it extracts every
   application program into `<dir>/products/<application-id>.knxprod`,
3. pins each archive in `bussard.lock` and links the devices it serves, and
4. writes one product model per ApplicationProgram to
   `<dir>/.bussard/models/<application-id>.yaml`.

With `--order-number` the downloaded file is stored the same way, origin `index`.
Output is deterministic: running the command twice on the same file produces
byte-identical archives, lock and model YAML.

## Product data and the device files

The texts that make a device file readable come from the product data, not from the ETS
project: the channel names, the parameter texts and enum labels, and the object
functions. `import` and `adopt` derive the keys from them and record the mapping in
`bussard.lock`, so a channel reads `betriebsart = "Jalousie"` and
`langzeitbetrieb.listen = ["0/1/3"]`. Without the product model, a device file still
holds the device's name, location and links, but its channels are keyed by vendor ids
and its objects by number, and its parameters can be neither checked nor flashed. The
lock carries what the rest of bussard needs (the com-object table, DPTs, flags, module
offsets), so a checkout without `.bussard/models/` still validates, plans and decodes. See
[the model format](model-format.md#without-product-data).

## The product-data pointer index

Finding the right `.knxprod` for a device you just scanned means knowing its order
number and then hunting the manufacturer's site. The pointer index does that lookup for
you. It is a small JSON file, `data/product-index.json`, that ships with bussard and
holds pointers only: a download URL and a checksum for each product database, never the
copyrighted payload. `import-product --list` shows the index; `--order-number` looks a
number up.

With `--order-number`, bussard:

1. normalizes the order number (trims whitespace, upper-cases) and looks it up in the
   index,
2. shows you what it found and where it will download from,
3. asks for confirmation (a TTY prompt, or `--yes-download` to skip it; on a non-TTY it
   refuses unless `--yes-download` is given),
4. downloads to memory with a hard 100 MiB cap,
5. verifies the download's byte size and SHA-256 against the index, where a mismatch is
   a hard error, since it means the file is not the one the index vouches for (the
   vendor may have re-published it), and
6. stores the verified file under `<dir>/products/` and runs the normal import on it.

### Index schema

`data/product-index.json` is `{ "entries": [ … ] }`, each entry:

| field             | type      | meaning |
| ----------------- | --------- | ------- |
| `manufacturer`    | string    | Human-readable name, e.g. `"MDT"`. |
| `manufacturer_id` | string    | KNX id as inside the `.knxprod`, e.g. `"M-0083"`. |
| `order_numbers`   | [string]  | Every order number this download serves, verbatim as in `Hardware.xml` (e.g. `"AKK-0216.03"`). One file often bundles a whole family. |
| `name`            | string    | Product/family display name. |
| `url`             | string    | Vendor-hosted download URL for the `.knxprod`. |
| `sha256`          | string    | Lower-case hex SHA-256 of the download. |
| `size`            | number    | Exact byte size of the download. |
| `filename`        | string    | Original vendor filename; also the file name under `products/`. |
| `application_ref` | string?   | Optional: the application-program ref an order number resolves to (documentation only). |
| `redistributable` | bool      | Whether bussard may mirror the file itself. Always `false` for vendor-hosted copyrighted data. |
| `notes`           | string?   | Optional version / date-verified / caveats. |

Order-number matching is case- and whitespace-insensitive but preserves interior
separators: `AKK-0216.03` and `akk-0216.03` match, but `AKK021603` does not, because `-`
and `.` distinguish real KNX order numbers.

### What is in the index

The index covers about 26 product databases from MDT, Zennio, Lingg & Janke,
Theben and Elsner, each verified by downloading it and computing its checksum.
`import-product --list` prints it. The index points only at bare `.knxprod`
downloads, so the checksum covers the exact file ETS would read.
`BUSSARD_PRODUCT_INDEX` replaces the built-in index with your own, for a
private mirror.

## The generated model files

`import-product` turns each application program into a model file under
`<dir>/.bussard/models/`: the com-object table, the parameters with their
types, defaults and memory locations, the download procedure and the ETS
dialog structure. The files are generated and carry a banner saying so; do not
edit them. The directory holds only data bussard regenerates: when it is
missing or a file is of an older format, the next command rebuilds it from the
archives in `products/`.

## The never-redistribute rule

bussard never hosts, ships or mirrors vendor product data. `products/` holds the
copyrighted `.knxprod` files you obtained (or extracted from your own export), and
`.bussard/models/` holds files derived from them. Whether `products/` is committed is
your decision; a private repository is the usual case, and a public one should leave it
out. This is the same footing as any interoperability tool: bussard reads legitimately
obtained vendor files to interoperate.

## ETS version and namespace caveats

ApplicationProgram XML declares its schema as the default namespace on the root `<KNX>`
element, e.g. `http://knx.org/xml/project/23` for the ETS 6 generation. bussard parses
namespace-agnostically by local element name, so a file from `/20`, `/21`, or `/23`
reads the same way, and it records the schema version in `identity.schema_version` for
provenance.

Other caveats:

- Mask version drives behaviour. `mask_version` (e.g. `07B0`, `0705`, `0701`, `0021`)
  tells the downloader whether the device is property-based (System B) or memory-based
  (older System 1/2). The reader only records it.
- Modules. Complex devices define com-objects and parameters inside reusable module
  definitions (`MD-…`) that the device instantiates (`MD-…_M-n_MI-n_…`). The model file
  lists the application-level refs; bussard maps a device's instances onto them by
  dropping the `_M-n_MI-n` instance segment, with the instance base offsets recorded in
  `bussard.lock`.
- Translations. Names and texts resolve to `en-US` when a translation is present,
  falling back to the file's default language otherwise, matching ETS.
