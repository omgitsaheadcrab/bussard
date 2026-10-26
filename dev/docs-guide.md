# Documentation guide

How the pages in `docs/` are written. Read this before adding or changing a
page. The layout follows the [Divio system](https://docs.divio.com/documentation-system/):
tutorials, how-to guides, reference, explanation. The page style follows two
sites that do this well: the [Prisma docs](https://www.prisma.io/docs/compute/request-timeout)
(one topic per page, the answer first, short sections, a "Next steps" footer)
and the [Requests quickstart](https://requests.readthedocs.io/en/latest/user/quickstart/)
(example first, then one paragraph of explanation, second person, no ceremony).

## The reader

Two people read these pages. Write for the first; the second skims and
needs the reference.

**The owner.** Bought or inherited a KNX house, 40 to 100 devices. Follows
terminal tutorials, runs Home Assistant, has never opened ETS and does not
use git. Works with an LLM assistant over MCP for most of the reading and
editing, and runs the physical commands on the CLI. Knows: lights, blinds and
heating are on "the bus", a button talks to an actuator, there is an ETS
project file somewhere. Does not know: individual versus group address, DPT,
mask version, tunnelling slot, load state, application program, keyring. Is
afraid of breaking the heating in winter, of being locked out, and of paying
a stranger to relabel a button. Wants to know what every button does, change
one safely, undo it, and hold a backup that survives any single device.

**The integrator.** Years of ETS, reads a group monitor fluently, scripts
nothing, works on Windows because ETS does. Knows every KNX term. Needs: the
command, its flags, what it refuses and why, and how it shares the interface
with ETS. Skims headings and tables; reads prose only when something failed.

What every page has to teach the owner, once, in plain words: the model
directory is the installation as files; a device holds tables and memory but
no meaning; every write is plan, confirm, write, verify; the gateway is a
shared, slow resource; bussard never redistributes vendor product data.

## Page kinds

| Kind | Answers | Length | Opens with |
|---|---|---|---|
| Tutorial | "Take me through it" | up to 250 lines | what you will have at the end, and what you need |
| How-to | "How do I X?" | up to 120 lines | the command, then its output |
| Reference | "What exactly does X do?" | as long as the facts, split by command group | a one-sentence summary and the synopsis |
| Explanation | "Why is it like this?" | up to 200 lines | the claim the page defends |

One topic per page. A page that needs a table of contents to navigate is two
pages.

## Page anatomy

Every page:

1. `# Title`: a task or a noun phrase, two to five words. How-to titles are
   imperative ("Change what a button does"), not questions.
2. One or two sentences: what this page covers and who it is for. No
   "This page describes".
3. Sections with `##` headings. A how-to heading is a step; a reference
   heading is the command or the key; an explanation heading is a claim.
4. `## Next steps` (how-to, tutorial, explanation) or `## Related` (reference):
   two to four links with a half-sentence each.

Inside a section: example first, then one short paragraph on what happened.
A command is always shown with its output, in a `console` block with a `$`
prompt. Mark illustrative output as such in the sentence before it.

Callouts, using Material admonitions:

- `!!! warning` before anything that transmits on the bus or writes a device.
- `!!! note` for a fact the reader may need later but not now.
- `??? note "Details"` (collapsed) for wire-level behaviour, timing, and
  the exact sequence a command sends. This is where protocol detail goes;
  the main text says what the reader sees.

Tables for options, files, keys and exit codes. Prose for everything that
has a verb.

## Reference entries

Each command has its own `## \`bussard <command> [ARGS]\`` section on its
group page (`docs/reference/commands/*.md`), in this order:

1. One sentence: what the command does, what it reads, what it writes.
2. `console` block: synopsis and the typical invocation with output.
3. Options table: `| Flag | Default | Meaning |`. Every long flag the
   command's `--help` prints must appear in its section; a test enforces it.
4. Behaviour: a bullet list of what it refuses and what it verifies.
5. `??? note "Details"` for the on-wire sequence, where there is one.

Global options, exit codes and environment variables are documented once on
their own pages and are not repeated per command.

## Writing rules

- Second person, present tense, active voice. "bussard refuses" not "the
  write will be refused".
- Sentences under 25 words. Paragraphs of two to four sentences. Three or
  more parallel items become a list.
- No em-dashes. Colons, commas and full stops.
- Define a KNX term the first time it appears on a page, in a half-sentence,
  only when the owner would not know it: individual address, group address,
  DPT, mask, tunnelling slot, load state, application program, keyring.
- Name the command once per sentence, in backticks. Keep code out of prose
  otherwise.
- Say what the reader sees and does. Internals (crates, functions, PIDs,
  load-state machines, captures, oracle rows) go into a collapsed Details
  block or into `dev/`.
- No issue numbers, dates of verification, capture file names, test names,
  or "since version". The changelog carries history.
- Keep every fact that changes what the reader does: a refusal and its
  cause, a default, what is written and what is not, what "verified" means.
- Never invent behaviour, flags, output or numbers. Rewrite from the source
  pages; when the source is silent, leave it out.
- Link forward to the reference for exhaustive flags, backward to the
  explanation for the why, sideways to the how-to for the next task.

## Structure

```
docs/
  index.md                              what bussard is, who it is for, the map
  getting-started/
    install.md                          binary, checksum, build from source
    first-model.md                      import or init, validate, monitor
    assistant.md                        connect an LLM over MCP, first conversation
    first-weekend.md                    the owner's weekend, end to end
  how-to/
    watch-the-bus.md                    monitor, read, viz, capture and history
    find-what-you-have.md               audit, scan, describe, reconstruct, free tunnel slots
    change-a-link.md                    change what a button does, plan, apply, undo
    change-a-parameter.md               set a parameter, the device dialog, apply or flash
    add-a-device.md                     adopt, flash a fresh device, bench labelling, no project file
    back-up-and-replace.md              backup, restore, replace a dead device, stale links
    commission-a-line.md                plan group addresses, commission, acceptance test
    exchange-with-ets.md                names into ETS, what a new export changes
    work-with-an-integrator.md          bundles and repositories
    handover-checklist.md               the checklist
    home-assistant.md                   generate and tune the HA config
    knx-secure.md                       keyring, secure tunnelling, secure a group address
  reference/
    commands/
      model.md                          init, import, export, diff, validate, groups, export-groups, doc
      bus.md                            monitor, capture, read, write, learn, test
      devices.md                        scan, assign, describe, reconstruct, adopt, status, device, import-product
      programming.md                    plan, apply, flash, commission, backup, restore, replace
      secure.md                         keys, groups secure, interface secure
      history.md                        history, show, undo
      servers.md                        mcp, viz, ha-config, audit
    global-options.md                   conventions, global options, confirmation and JSON, exit codes
    environment.md                      environment variables, the .env file
    model-directory.md                  discovery, products/, .bussard/, the bundle format
    files/
      bussard-toml.md
      groups-toml.md
      device-files.md
      bussard-lock.md
      bussard-keys.md
      tests-toml.md
      ha-toml.md
      errors.md                         parse errors and hints
    diagnostics.md                      validation codes
    json-output.md                      telegram JSON, the capture database
    mcp-tools.md                        the MCP server: tools, tiers, the bus connection
    viz-api.md                          the viz server endpoints
    product-data.md                     .knxprod handling, the product store, the index
    home-assistant.md                   entity derivation, ha.toml keys
  explanation/
    how-it-works.md                     the model, product data, the bus, a write
    safety.md                           the rules: one page, read before the first write
    write-commands.md                   what each write command does and its rails
    knx-secure.md                       keys, the key store, secrets, what secured means
    assistant.md                        MCP tiers, history and undo, protected addresses
    device-support.md                   supported masks, tunnel budget, source-address check, known limitations
```

Maintainer material lives in `dev/` and is never rendered.
