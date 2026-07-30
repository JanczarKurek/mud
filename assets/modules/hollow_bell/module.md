# Module: The Hollow Bell

<!-- module-id: hollow_bell | tier: T2-T4 -->

> **Installed / uninstalling.** Unlike most modules this one ships its own
> `maps/` (five spaces, registered as `hollow_bell/<stem>`), and it is reached
> by a portal added to the *core* `assets/maps/overworld.yaml`
> (`hollow_bell_entrance`, at tile 6,45).
>
> `SpaceDefinitions::load_from_disk` asserts that every portal destination
> exists, so **deleting this folder without also removing that overworld portal
> will panic the game at startup.** Uninstall = delete
> `assets/modules/hollow_bell/` *and* revert the `hollow_bell_entrance` portal
> and its arch/sign props in `assets/maps/overworld.yaml`.
>
> Note also that the map editor serialises to `assets/maps/<authored_id>.yaml`,
> so these slash-namespaced maps cannot be round-tripped through editor Save —
> they are hand-authored YAML.

## Overview

Two hundred years ago the badger-folk of **Ashen Hollow** sank a shaft under the
moor and found something no one expected: a seam of pale, faintly humming metal
that the old ledgers call **bell-bronze**. It was no good for coin and worse for
swords — strike it and it *sings*, and keeps singing long after you have stopped
listening. So they did the only sensible thing with a metal that sings. They cast
a bell.

The **Hollow Bell** hung in the Undercroft at the very bottom of the delve, and
every dawn for two centuries a rope-shift climbed down to ring it. Nobody could
tell you why. It was simply what one did, the way one salts a doorstep or leaves
milk for the hedge. The ledgers record the ringing beside the tally of ore and
the price of candles, in the same flat hand, for two hundred years.

Three weeks ago the bell cracked.

The mine went quiet — properly quiet, the quiet of a room where someone has
stopped breathing — and the things that had spent their whole long lives with
that sound in their bones woke up confused, furious, and very hungry. The
day-shift did not come up. The night-shift went down after them and did not come
up either. What is left of Ashen Hollow is four people and a dog's worth of
courage camped in the winding-house at the pit-head, boiling tea and arguing
about whose fault it is.

They will not abandon the delve. Badgers are like that.

**The work:** go down. Clear the flooded works. Get back the bell's tongue from
whatever took it. Cut new bell-bronze from the singing seam. Re-cast the bell in
the old foundry, carry it down to the Undercroft, and ring it — and decide,
when you are standing in front of the thing the bell was keeping asleep, whether
what you have come to do is kill it or sing it back to sleep.

**Tone note for whoever builds this:** the delve is dangerous and dark and the
body-count is real, but Ashen Hollow itself is warm. There is tea. There are
terrible jokes. The apprentice bellwright is nineteen and has never cast
anything larger than a doorbell and is *absolutely certain* he can do this. The
horror at the bottom is not evil. It has been in pain for three weeks and cannot
sleep, and it is very, very old, and it does not understand what it is doing.
That should land when the player meets it.

---

## Locations

### The Pit-Head Camp (id: pithead)

The winding-house at the mouth of the shaft: a stone drum of a building with a
great rusted drum-wheel in the middle of it, a chimney that smokes at all hours,
and canvas slung between it and the ore-sheds to make three lean-tos. Marten's
map of the delve is nailed to the wheel-housing with a pick. There is always
a kettle on. Beyond the canvas the moor goes out flat and grey in every
direction and the wind never entirely stops.

Safe ground: no hostiles spawn here. The cage-lift down into the works is at the
north end, and Marten will not let anyone use it until they have talked to him
about what is down there.

### The Winding Works (id: winding_works)

The upper galleries — two centuries of tunnel, timbered and re-timbered, running
off a central haulage-way still floored with iron rail. The pumps stopped when
the shift did, so the low workings are shin-deep in cold black water and the
sound of dripping never lets up. Rats the size of terriers. Bats. Something
long and pale in the sump that does not like light. And, in the eastern
store-galleries, a band of goblin scavengers who came down out of the moor when
they smelled an unattended mine and are currently having the best month of their
lives.

Hettie Marl of the day-shift is alive somewhere in here, behind a fall of rock,
rationing a candle.

Level 4–6.

### The Bell Foundry (id: foundry)

A cathedral of a room, cut square out of the rock, with a casting floor of packed
sand and a gantry running the walls one storey up. The furnace is cold. The
runner-channels that once carried molten bell-bronze across the floor are still
there, and still full — the last pour never got poured, and it set where it
stood, in great frozen rivers.

Everything in here is coated in tallow. The foundry burned tallow candles by the
gross — the ledgers are half candle-invoices — and something has been *eating*
them, and growing, and it is wearing the bell's clapper on its head like a crown
because it thinks the crown is what makes it king.

Level 6–9. **Cinderjack** is here. So is the old furnace, which still works if
somebody who knows what they are doing lights it.

### The Singing Seam (id: singing_seam)

Below the foundry the rock changes. The seam itself is a wall of pale crystalline
bell-bronze forty feet high, and it hums. Not loudly. You feel it in your teeth
before you hear it. Everything that lives down here has grown up inside that
note and taken on some of it: the shrikes that nest in the roof have crystal in
their wings, and the thing that calls itself **Knell** is not really a creature
at all so much as a place where the seam has been listening for so long that it
has learned to answer.

Ore can be cut here with a pickaxe, if you can stand still long enough.

Level 9–12.

### The Undercroft (id: undercroft)

The bottom. A round chamber, older than the mine by a very long way — the badgers
did not cut this, they *broke into* it — with a floor of fitted black stone worn
into a shallow bowl by something enormous turning over in its sleep, over and
over, for centuries. The Hollow Bell hangs from a frame at the centre, split from
lip to crown.

And in the dark past the edge of the lamplight, the **Deeplistener**: blind,
vast, mole-shaped in the way a mountain is hill-shaped, three weeks without
sleep and in more pain than anything ought to have to carry.

Level 12–14. This is the end of the module.

---

## NPCs

### Marten Coalbright, Pit-Captain (id: marten_coalbright)

A badger somewhere north of sixty, grey through the muzzle, with a pit-captain's
brass whistle he has not blown since the day the shift did not come up because
he cannot yet stand to hear the sound it makes. He runs the camp the way he ran
the delve: by list, by rota, and by not letting anyone see him stop moving. He
lost eleven people three weeks ago and he has their names written on the inside
of the wheel-housing door where the others will not look.

He is not warm to strangers. He is unfailingly *fair* to them, which from Marten
is nearly the same thing.

```hints
tier: T2
level: 6
hostile: false
role: questgiver
gives_quest: down_the_shaft
stats: { str: 14, con: 14, cha: 12, wil: 14 }
appearance: elderly grey-muzzled badger in a soot-black pit-captain's coat with brass buttons, leather cap with a candle-bracket, brass whistle on a chain
```

### Sister Wick (id: sister_wick)

A hedge-priest mouse, small and brisk, who came out to Ashen Hollow eleven days
ago because she heard there were people who needed mending and has not left. She
sleeps four hours a night in the ore-shed and is quietly, comprehensively
furious about the whole situation in a way that expresses itself entirely as
excellent organisation. She sells what she can spare and gives away what she
cannot, and will not discuss the difference.

Her candles are not ordinary candles. She makes them herself, from tallow, with
a great deal of muttering over the wick.

```hints
tier: T2
level: 5
hostile: false
role: merchant
gives_quest: wick_and_wax
stats: { wil: 15, cha: 14, foc: 13 }
shop: [potion @48 x6, lesser_heal_scroll @90 x3, cure_wounds_scroll @260 x2, bless_scroll @110 x2, apple @5, torch @9 x20, pit_tea @30 x8]
appearance: small brown mouse hedge-priest in undyed homespun robe with a rope belt, satchel of bandages, a lit candle stub tucked behind one ear
```

### Tobin Ashfoot (id: tobin_ashfoot)

Nineteen, fox, apprenticed to the bellwright's shop in a ferry-town two days off,
and out here because his master is dead of age and he is the only person left
alive who has read the casting books. He has cast exactly four things in his
life, all of them doorbells, one of them badly. He talks too fast and too much
and cannot be discouraged, and under the noise he is the single most useful
person in the camp and knows it, which is the only thing keeping him upright.

He will teach you to re-cast the bell. He is *very* excited about this.

```hints
tier: T2
level: 4
hostile: false
role: questgiver
gives_quest: the_stolen_tongue
stats: { foc: 15, agi: 13, cha: 13 }
appearance: young rust-red fox in a scorched leather apron over a too-big shirt, sleeves rolled, soot on the nose, brass calipers hanging from a belt loop
```

### Hettie Marl (id: hettie_marl)

Day-shift, twenty-two years down the delve, otter, and the only member of either
shift still breathing. When you find her she has been behind a rock-fall in the
dark for three weeks on sump-water and two candles and she is perfectly calm
about it, because panicking would have used air. Get her out and she will set up
in the camp with what she managed to carry and sell it to you, at prices that
are frankly insulting, and grin at you the whole time.

She knows the works better than the map does.

```hints
tier: T2
level: 7
hostile: false
role: merchant
stats: { con: 15, agi: 14, wil: 15 }
shop: [torch @8 x30, pickaxe @210 x2, canvas_backpack @140, small_pouch @95 x3, potion @54 x4, miners_draught @40 x6, iron_ore @18 x10]
appearance: lean otter miner in a patched canvas jacket and knee-boots, hair cropped short, candle-bracket helmet, coil of rope over one shoulder
```

### Grandam Bellow (id: grandam_bellow)

The bellwright who cast the Hollow Bell, dead these two hundred years and not
noticeably inconvenienced by it. She is *in* the bell — she poured herself into
the mould with the metal, on purpose, because a bell that must ring for two
centuries needs someone in it who cares whether it does. She is dry, unbothered,
and has been waiting three weeks for somebody competent to turn up.

She will tell you what is at the bottom of the delve and what it costs to put it
back to sleep, and she will let you decide, and she will not tell you which
choice she thinks is right. (She thinks you should sing it down. She will not
say so.)

Speak to her at the cracked bell in the Undercroft, and again at the re-cast one.

```hints
tier: T3
level: 12
hostile: false
role: questgiver
gives_quest: the_deeplistener
dialog: true
stats: { wil: 18, foc: 17, cha: 15 }
behavior: { detect: 3, disengage: 3, step: 6.0 }
appearance: translucent pale-blue badger matriarch in a bellwright's long apron, hands folded, edges dissolving into bell-shaped ripples of light, feet not quite touching the floor
```

---

## Creatures

### Tallow Drip (id: tallow_drip)

A hand-sized lump of animate candle-fat that humps along the floor leaving a
smear. Individually pathetic. Cinderjack makes them the way a wound makes pus,
and they come in threes, and they are *warm*.

```hints
tier: T1
level: 3
hostile: true
damage: 1d4
damage_type: fire
hp: 1d6+14+constitution
behavior: { detect: 6, disengage: 12, step: 0.7 }
drops: [tallow_wax x1 @0.5]
appearance: fist-sized blob of dirty yellow candle-wax with a guttering flame on top, trailing a glistening smear, two dim orange eye-points
```

### Sump Crawler (id: sump_crawler)

Long, pale, blind, and entirely too many legs. It lives in the flooded low
workings and has never seen a light it did not resent. It does not chase far
from water.

```hints
tier: T2
level: 5
hostile: true
damage: 1d8
damage_type: poison
hp: 2d8+38+constitution*2
armor: 1
behavior: { detect: 7, disengage: 10, step: 0.85 }
drops: [copper_coin x1d6, green_herb x1 @0.25, poison_flask x1 @0.15]
appearance: pale segmented centipede-thing the length of an arm, wet translucent shell, no eyes, dozens of thin white legs, dripping
```

### Wax Wight (id: wax_wight)

A foundry-hand who fell asleep in the tallow store and did not get up. Slow,
silent, and still wearing the apron. It burns from the inside; where it walks the
floor scorches. There is very little left of the person, but it still turns
towards the furnace when it does not know where else to go.

```hints
tier: T3
level: 7
hostile: true
damage: 1d10
damage_type: fire
hp: 2d10+52+constitution*2
armor: 2
behavior: { detect: 8, disengage: 14, step: 1.0 }
drops: [tallow_wax x1d3, copper_coin x2d6, silver_coin x1 @0.4, tallow_candle x1 @0.3]
appearance: humanoid figure of dark yellowed wax in a scorched foundry apron, face half-melted and featureless, orange glow visible through the chest, drips as it moves
```

### Seam Shrike (id: seam_shrike)

A bird that nested too long in the singing seam. Its wing-feathers have gone to
splinters of bell-bronze and it throws them, and it screams on exactly the note
the seam hums, which is the worst part.

```hints
tier: T3
level: 10
hostile: true
attack: ranged
range: 7
damage: 2d6
damage_type: pierce
hp: 2d10+70+constitution*3
behavior: { detect: 10, disengage: 16, step: 0.75 }
drops: [bell_bronze_ore x1 @0.35, silver_coin x1d4, arrow x1d8 @0.4]
appearance: crow-sized shrike with pale crystalline flight feathers that catch light like struck metal, hooked beak, eyes like chips of bronze, faint humming aura
```

### Hollow Spawn (id: hollow_spawn)

Not children of the Deeplistener — *pieces* of its dreaming, which is a different
and worse thing. Blind, mole-clawed, roughly person-shaped, and they only appear
near the bottom. They dig out of the walls.

```hints
tier: T4
level: 12
hostile: true
damage: 2d8
damage_type: earth
hp: 3d10+100+constitution*4
armor: 3
behavior: { detect: 9, disengage: 18, step: 0.9 }
drops: [gold_coin x1 @0.3, silver_coin x2d6, bell_bronze_ore x1 @0.2, deep_iron_shard x1 @0.25]
appearance: hunched eyeless humanoid of packed black earth and root, enormous mole-claws, no face, faint blue bell-glow leaking from cracks in its hide
```

---

## Bosses

### Cinderjack, the Tallow King (id: cinderjack)

Once a tallow-drip that got at the foundry candle-store and did not stop. Now the
size of a cart, mostly molten, wearing the Hollow Bell's iron clapper on its head
because it found it and it is shiny and *therefore* it is a crown. It is genuinely
delighted about this. It bows to you when you come in. Then it sets you on fire.

When hurt it sloughs off pieces of itself, and the pieces get up.

```hints
tier: T3
level: 8
hostile: true
damage: 2d8+4
damage_type: fire
hp: 4d12+215+constitution*4
armor: 3
crit_range: 19
bab_track: full
behavior: { detect: 11, disengage: 30, step: 0.85 }
drops: [bell_clapper x1, tallow_crown x1, gold_coin x2, silver_coin x4d6, tallow_wax x1d6, greater_heal_potion x1 @0.5]
appearance: cart-sized mound of guttering yellow tallow with a rough grinning face melted into the front, arms of dripping wax, wearing a heavy iron bell-clapper tilted on its head like a crown, ringed with small flames
```

### Knell, the Seam-Singer (id: knell)

The seam has been listening for a very long time. Knell is what happens when
listening becomes answering. It has no body to speak of — a suspended cloud of
bell-bronze shards, turning slowly, holding a chord. It fights politely at first,
one shard at a time. When you have hurt it enough it stops being polite and
sings the whole chord at once, and the room rings.

```hints
tier: T3
level: 11
hostile: true
attack: ranged
range: 8
damage: 2d8+3
damage_type: frost
hp: 4d12+290+constitution*4
armor: 2
behavior: { detect: 12, disengage: 26, step: 0.7 }
drops: [knells_shard x1, singing_pick x1, gold_coin x3, silver_coin x5d6, bell_bronze_ore x1d4, frost_lance_scroll x1 @0.5]
appearance: floating cluster of pale crystalline bell-bronze shards suspended in a slowly rotating sphere about a person's height, no body, cold blue light in the gaps between shards, faint constant hum
```

### The Deeplistener (id: deeplistener)

Enormous. Blind. Older than the shaft, older than the hamlet, probably older than
the moor. It has been asleep down here since before anyone was counting, and the
bell was what kept it under, and for three weeks it has been awake and it hurts
and it does not know why and it cannot find the sound. It is not attacking you.
It is *searching*, and you are in the way, and it is the size of a house.

It can be killed. It does not have to be.

```hints
tier: T4
level: 14
hostile: true
damage: 2d12+6
damage_type: earth
hp: 5d12+455+constitution*5
armor: 4
crit_range: 19
bab_track: full
behavior: { detect: 14, disengage: 40, step: 1.1 }
drops: [deeplisteners_ear x1, gold_coin x8, silver_coin x6d6, deep_iron_shard x1d4, bellwrights_hammer x1 @0.5]
appearance: colossal eyeless mole-beast filling half the chamber, hide of packed black earth and pale roots, vast digging claws, blunt questing snout, ancient bell-shaped scars glowing faint blue along its flanks
```

---

## Items

### Sack of Tallow Wax (id: tallow_wax)

Rendered fat, grey-white, smelling faintly of the foundry. Sister Wick wants it
for candles. Cinderjack's leavings are unfortunately the best source in the delve.

```hints
kind: pickup
weight: 1.5
stack: 20
appearance: greasy grey-white lump of rendered tallow wrapped in waxed cloth and string
```

### Bell-Bronze Ore (id: bell_bronze_ore)

Pale, faintly warm, and it hums against your palm. Struck, it rings for an
absurdly long time. Cut from the singing seam with a pickaxe.

```hints
kind: pickup
weight: 3.0
stack: 20
appearance: fist-sized chunk of pale crystalline ore shot through with bronze veins, faintly luminous
```

### Deep Iron Shard (id: deep_iron_shard)

Black, dense, and colder than the room. It came out of whatever the Undercroft
was before the badgers broke into it. Tobin wants three for the bell's crown.

```hints
kind: pickup
weight: 2.0
stack: 10
appearance: jagged shard of near-black metal with an oily blue sheen, frost beading on its surface
```

### The Bell's Tongue (id: bell_clapper)

Two hundred years of dawns are in this lump of iron. It is heavier than it has
any business being. Recovered from Cinderjack's head.

```hints
kind: pickup
weight: 12.0
stack: 1
appearance: heavy pitted iron bell-clapper the length of a forearm, bulbous striking end polished mirror-bright by centuries of use, wax-spattered
```

### Pit Tea (id: pit_tea)

Stewed to within an inch of its life, three sugars, served in a tin cup you will
be expected to return. Ashen Hollow runs on this.

```hints
kind: consumable
weight: 0.3
effect: { regen_multiplier: 2.5, regen_duration_seconds: 90 }
stack: 10
appearance: dented tin cup of very dark stewed tea, steam rising, a chip out of the rim
```

### Miner's Draught (id: miners_draught)

Wick's own. Tastes of pine tar and regret; puts you back on your feet.

```hints
kind: consumable
weight: 0.4
effect: { restore_health: 45 }
stack: 10
appearance: squat brown glass bottle with a waxed cork and a hand-inked paper label
```

### Tallow Candle (id: tallow_candle)

Wick makes them with a great deal of muttering over the wick. Burns steadier and
longer than it ought to in bad air.

```hints
kind: consumable
weight: 0.2
effect: { regen_multiplier: 1.6, regen_duration_seconds: 120 }
stack: 20
appearance: stubby hand-dipped tallow candle, uneven and yellowed, with a long black wick
```

### The Tallow Crown (id: tallow_crown)

Cinderjack's crown, which was never a crown. Scrape the wax off and it is an
honest iron helm, and it has kept the heat off *something* for a long time.

```hints
kind: equipment
slot: helmet
tier: T3
weight: 3.5
stats: { con: 2, wil: 1 }
appearance: battered iron half-helm thick with runnels of hardened yellow wax, small dents all over the crown
```

### Knell's Shard (id: knells_shard)

A single shard of the Seam-Singer, still holding its note. Wearing it, you can
hear the delve breathing. Focus-users find it clarifying.

```hints
kind: equipment
slot: amulet
tier: T3
weight: 0.4
stats: { foc: 3, max_mana: 25 }
appearance: palm-length sliver of pale crystalline bell-bronze on a fine silver chain, faintly luminous, humming
```

### The Bellwright's Hammer (id: bellwrights_hammer)

Grandam Bellow's own casting hammer, lost in the Undercroft when she went down to
hang the bell and did not come back up. Two hundred years under the Deeplistener
has done something to it. It rings when it lands.

```hints
kind: equipment
slot: weapon
tier: T4
weight: 6.0
damage: 2d6
damage_type: blunt
stats: { str: 2 }
appearance: long-hafted bellwright's hammer, ash shaft bound in worn leather, heavy bronze head green with age and inscribed with a ring of tiny bell-marks
```

### The Singing Pick (id: singing_pick)

A miner's pick with a head of seam-bronze. It hums in the hand and cuts stone
like the stone has agreed to it. Also a perfectly serviceable weapon, which is
more than can be said for an ordinary pickaxe.

```hints
kind: equipment
slot: weapon
tier: T3
weight: 4.5
damage: 1d8
damage_type: pierce
stats: { agi: 1 }
appearance: miner's pick with a pale crystalline bell-bronze head and a scarred ash haft, faint bronze shimmer along the striking edge
```

### The Deeplistener's Ear (id: deeplisteners_ear)

A disc of black stone from the beast's hide, worn smooth by two centuries of
turning over in its sleep. Hold it and you hear things a long way off, through
rock.

```hints
kind: equipment
slot: amulet
tier: T4
weight: 0.6
stats: { wil: 2, foc: 2, con: 1 }
appearance: palm-sized disc of polished black stone on a leather thong, concentric ridges like a fingerprint, cool to the touch, faint blue glow deep inside
```

### The Hollow Bell, Re-Cast (id: hollow_bell_charm)

Not the great bell — a hand-bell, cast from the same pour, the size of an apple.
Tobin made two and kept neither. Ring it when you are lost.

```hints
kind: equipment
slot: ring
tier: T4
weight: 0.3
stats: { wil: 2, cha: 2, max_health: 20 }
appearance: small perfect hand-bell of pale bell-bronze worn on a finger-ring, worn mirror-bright, no clapper visible
```

### Greater Heal Potion (id: greater_heal_potion)

Wick's best, and she only has so many.

```hints
kind: consumable
weight: 0.5
effect: { restore_health: 80 }
stack: 5
appearance: heavy cut-glass flask of deep red liquid with a silver-wired stopper
```

---

## Quests

### Down the Shaft (id: down_the_shaft)

Marten will not let anyone at the cage-lift until the haulage-way is passable,
and the haulage-way is not passable, because it is full of things with teeth. He
is blunt about it: he does not know you, he does not much like the look of you,
and eleven people are already dead of somebody being brave in the dark. Clear
the vermin out of the upper works and he will start talking to you like a
person.

```hints
giver: marten_coalbright
kind: kill
objective: kill 8 sump_crawler
reward: [potion x2, silver_coin x30, torch x10]
persuade_bonus: [miners_draught x2]
```

### The Lost Shift (id: the_lost_shift)

Marten has not said her name out loud in three weeks. He says it now: **Hettie
Marl**, day-shift, twenty-two years down, and there was a fall in the western
gallery and no body was ever found, and he would like — he says this looking at
the map, not at you — he would like to know either way.

She is alive. Find her, and tell her which way is out.

```hints
giver: marten_coalbright
kind: talk
objective: find hettie_marl in the winding works and speak to her
reward: [silver_coin x40, pit_tea x3]
```

### Wick and Wax (id: wick_and_wax)

Sister Wick has run out of candles, and candles are not optional this far under
the moor. She needs tallow. There is, she observes with enormous restraint, a
great deal of tallow walking about in the foundry at the moment.

```hints
giver: sister_wick
kind: fetch
objective: bring 6 tallow_wax to sister_wick
reward: [tallow_candle x6, cure_wounds_scroll x1, silver_coin x24]
persuade_bonus: [greater_heal_potion x1]
```

### The Stolen Tongue (id: the_stolen_tongue)

Tobin can re-cast the bell. He is certain of this. He has read the books twice.
What he cannot do is cast a *clapper* — the tongue is two hundred years old and
half of what makes the bell the bell is what those two centuries did to that
lump of iron, and you cannot pour that. It has to be the original.

The original is currently a hat.

```hints
giver: tobin_ashfoot
kind: kill
objective: kill 1 cinderjack
reward: [gold_coin x3, empower_weapon_scroll x1]
```

### The Singing Seam (id: the_singing_seam)

With the tongue back, Tobin needs metal — and not just any metal, it has to come
out of the same seam, because a bell cast from a different pour will not hold the
same note and the whole point is the note. He also wants three shards of the
black deep-iron for the crown, which is not in any book; he has simply decided
it will work, and he is right, and he cannot tell you how he knows.

Cut the ore from the singing seam. Bring it up. He will teach you the casting.

```hints
giver: tobin_ashfoot
kind: fetch
objective: bring 5 bell_bronze_ore and 3 deep_iron_shard to tobin_ashfoot
reward: [gold_coin x4, knells_shard x1]
give_recipe: recast_hollow_bell
```

### The Deeplistener (id: the_deeplistener)

Grandam Bellow has been dead for two hundred years and waiting for three weeks
and she is *extremely* ready to have this conversation.

She tells you what is down there and what it is not: not a demon, not a curse,
not anything anyone did wrong. Just something very old that lives under the moor
and sleeps, and needs a sound to sleep to, and has not had one for three weeks.
The badgers did not trap it. They found it already sleeping and worked out, over
a few bad generations, what kept it that way — and then rang the bell every dawn
for two hundred years and wrote it in the ledger next to the price of candles.

You can kill it. It is in pain and it is enormous and it will not stop, and
killing it is a real answer and she will not think less of you.

Or you can hang the re-cast bell, and ring it, and let it go back down.

```hints
giver: grandam_bellow
kind: kill
objective: kill 1 deeplistener, or ring the re-cast bell in the undercroft
reward: [gold_coin x25, deeplisteners_ear x1, hollow_bell_charm x1]
```

---

## Recipes

### Re-Cast the Hollow Bell (id: recast_hollow_bell)

Tobin's casting, out of the old books and one thing he made up. Fire the foundry
furnace, pour the seam-bronze, set the deep-iron in the crown, hang the tongue.
He will talk you through every step at a volume that is frankly dangerous in a
mine.

```hints
inputs: [bell_bronze_ore x5, deep_iron_shard x3, bell_clapper x1]
outputs: [recast_hollow_bell_item x1]
station: foundry_furnace
xp: 900
```

### Pit Tea (id: brew_pit_tea)

Water, leaves, three sugars, and twenty minutes longer than any reasonable person
would leave it.

```hints
inputs: [green_herb x2]
outputs: [pit_tea x2]
station: campfire
xp: 25
```

---

## Quest items

### The Re-Cast Hollow Bell (id: recast_hollow_bell_item)

Pale bronze, waist-high, with a crown of black deep-iron and a two-hundred-year-old
iron tongue. It weighs what a bell weighs. Carry it down.

```hints
kind: pickup
weight: 40.0
stack: 1
appearance: waist-high bell of pale bell-bronze, freshly cast and still bright, black iron band around the crown, heavy pitted clapper hanging within
```
