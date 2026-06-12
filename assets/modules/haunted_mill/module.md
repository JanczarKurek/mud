# Module: The Haunted Mill
<!-- module-id: haunted_mill | tier: T1 -->

> Reference module for the `build-module` skill. It deliberately stays small —
> one new NPC, one new item, one fetch quest — and **reuses** existing content
> (`rat`, `potion`, `copper_coin`, `silver_coin`) wherever possible, to show the
> "reuse before inventing" rule. Use it as a smoke test: `build-module
> modules/example_haunted_mill.md`.

## Overview

Below the village of Lockwood, where the lane bends to follow the Sweetwater,
stands a grain-mill that no one tends. Old Maple, the miller, drowned in the
race a year ago come autumn, and the wheel has turned untended ever since.
Folk crossing the bridge at dusk swear they hear millstones grinding in an
empty building, and a smell of damp flour on the wind.

Lately it has gone from eerie to inconvenient: rats have made the flooded
cellar their own, bold and fat on spoiled flour, and they have started turning
up in Lockwood's pantries. The villagers would pay to be rid of them — but the
braver sort whisper that the rats are only a symptom, and that poor Maple will
not rest until one last sack of grain is finally milled.

This is a starter errand (T1): a short trip down a ladder into the dark, a
gentle ghost to satisfy, and a little coin for the trouble.

## Locations

### The Sweetwater Mill (id: sweetwater_mill_house)

A timber mill leaning out over its mossy wheel, roof furred with moss. The
ground floor is stacked with split sacks of spoiled flour and the quiet of a
place that has stopped being used. A ladder in the corner drops into a flooded
stone cellar where the water laps black against the walls and something keeps
moving just past the lantern-light. Up a narrow stair, the miller's account-book
still lies open on a desk, the last day's column never totted up.

## NPCs

### Old Maple, the Miller's Ghost (id: mill_ghost)

A translucent, pale-blue badger in a flour-dusted apron, stooped from a
lifetime over the millstones. He drifts about the upper room fretting over
ledgers that will never balance now, polite and unbearably sad. He bears no one
ill will — he simply cannot let go until one honest task is finished: the last
sack of moonshade grain, the one he meant to mill the morning he died, ground
properly at last. He knows the rats are bad and apologizes for them.

```hints
tier: T1
hostile: false
role: questgiver
gives_quest: mill_last_grain
appearance: translucent pale-blue badger ghost in a dusty flour-streaked miller's apron, stooped
```

## Items

### Sack of Moonshade Grain (id: moonshade_grain)

A heavy burlap sack of pale grain that glows the faint silver-white of
moonlight on water. It is the last harvest Old Maple brought in and never lived
to mill — the rats have been gnawing at the corner of it down in the cellar.

```hints
kind: pickup
tier: T1
weight: 3.0
appearance: bulging burlap sack spilling faintly luminous pale silver-white grain
```

## Quests

### The Last Grain (id: mill_last_grain)

Old Maple asks the adventurer to climb down into the flooded cellar — minding
the rats that nest there — and recover his Sack of Moonshade Grain so he can
mill it one final time and rest. It is a simple fetch: retrieve the one sack
and bring it back up to him. He rewards a healing potion and a handful of
copper from the mill's old strongbox. A traveler with a honeyed tongue might
talk the grateful ghost into parting with a little silver besides.

```hints
giver: mill_ghost
kind: fetch
objective: bring 1 moonshade_grain to mill_ghost
reward: [potion x1, copper_coin x15]
persuade_bonus: [silver_coin x3]
```
