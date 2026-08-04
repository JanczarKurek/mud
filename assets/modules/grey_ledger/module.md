<!-- module-id: grey_ledger | tier: 1 -->

# The Grey Ledger

**A verification module.** Its purpose is to stress-test the yarn-only quest
pipeline: declarative journal files driven purely by `<<set $var>>` state,
live `{$var}` interpolation, `{ var, is }` exact-match stages, deep stage
lists, and the imperative `<<log_write>>` command. No quest scripts, no items,
no maps, no sprites (debug-color placeholders only).

Both NPCs are placed at the east edge of the Emberbrook plaza in
`assets/maps/overworld.yaml` (Quill at 39,24 behind a desk, Moss at 41,24
behind a crate, under a Grey Ledger sign) — a few steps east of the spawn
point. Map edits only take effect on a fresh world snapshot: delete
`~/.local/share/mud2/embedded/saves/world-state.json` (characters live in
`accounts.db` and survive). To uninstall, delete `assets/modules/grey_ledger/`
**and** the Grey Ledger block in `overworld.yaml`, then delete the snapshot
again. The NPCs can also be spawned anywhere via the embedded Python console:
`world.spawn("grey_ledger/assessor_quill", x, y)`.

A traveling assessor's office has set up at a crossroads: one desk, one heron,
one vole, and a quantity of paper that did not all arrive on one cart. The
Ledger records everything. The Ledger is never wrong. When the Ledger is
wrong, see form 7-C.

## NPCs

### Assessor Quill (id: assessor_quill)

A tall grey heron in a pince-nez, stationed behind a folding desk with a
stamp, an inkwell, and a bell nobody is permitted to ring. Precise, unhurried,
immune to irony. Speaks in filed clauses.

```hints
role: questgiver
level: 2
appearance: tall grey heron, pince-nez, ink-stained wing tips, folding desk
```

### Underclerk Moss (id: underclerk_moss)

A small harried vole behind a rampart of paper, keeper of the forms, the good
pen, and an ongoing census of small things. Kind, exhausted, keeps miscounting
the geese on purpose so the number stays plausible.

```hints
role: questgiver
level: 1
appearance: small brown vole, spectacles pushed up fur, drift of paper
```

## Quests

All four are **yarn-only** (no `.py`) — that is the point of the module.

### Permit in Triplicate (id: permit_in_triplicate)

`kind: talk`, giver: assessor_quill. Registering as an adventurer requires
Form 7-C: obtained from Moss, stamped by Quill, countersigned by Moss, filed
with Quill — at which point the filing is rejected (revision f was voided
that morning) and the whole loop repeats on revision g. Seven journal stages;
tests deep ordered stage lists on a two-NPC back-and-forth.
Reward: copper x30, potion x1.

### Census of Small Things (id: census_of_small_things)

`kind: talk`, giver: underclerk_moss. Report doors, buckets, and geese to the
census desk, one at a time, as many as you like; submit once seven entries
are recorded. Tests live `{$var}` counter interpolation in the journal (the
entry re-renders after every report) and a `{ var, is: 3 }` exact-match stage
that appears when exactly three geese are on record and disappears at four.
Reward: copper x20.

### The Wax Question (id: the_wax_question)

`kind: talk`, giver: assessor_quill (gated behind Permit in Triplicate). The
office seal must be commissioned: choose a wax colour (a string variable) and
a number of sticks (a number variable), both revisable before committing.
Tests `{ var, is }` exact matching on strings and numbers, and journal stages
switching live as the player changes their mind. Reward: copper x12.

### Marginalia (no journal file — deliberate)

Quill dictates margin notes. Each dictation runs `<<log_write>>` with its own
subsection (`grey_ledger/marginalia_i` … `_iii`), and the first note can be
revised (overwritten) later. Tests the imperative command path; ships **no**
journal YAML on purpose, since a declarative file would overwrite these
entries on the next variable change.
