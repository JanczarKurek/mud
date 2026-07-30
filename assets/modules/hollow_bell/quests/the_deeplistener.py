# The Hollow Bell — "The Deeplistener". The finale, with two endings.
#
# ENDING A (kill): the ObjectKilled subscription fires and the quest is ready
#   to hand in. A real answer. Grandam Bellow will not think less of you.
#
# ENDING B (ring): the player hangs the re-cast bell and rings it, and the
#   thing goes back down. Driven from the Yarn side by
#   `<<quest_command "hollow_bell/the_deeplistener" "ring">>`, because
#   `ObjectKilled` is the only QuestEvent the engine emits — there is no
#   on-interact or on-region hook to subscribe to. The dialog option is gated
#   on actually carrying the bell, so this cannot be triggered early.
#
# `ring` despawns the Deeplistener rather than killing it: no corpse, no loot
# roll, no XP-on-kill. The quest reward compensates, and the difference is the
# point — you do not get its ear if you did not take it.

import mud_quest_api as q

subscribes_to = ["ObjectKilled"]

TARGET = "hollow_bell/deeplistener"

state = {"slain": False, "rung": False}


def on_start(state):
    state["slain"] = False
    state["rung"] = False
    q.set_var("hollow_bell_finale_started", True)
    q.set_var("hollow_bell_finale_ready", False)
    q.set_var("hollow_bell_finale_rang", False)
    q.log("hollow_bell/the_deeplistener: started")


def on_event(ev, state):
    if ev["kind"] != "ObjectKilled":
        return
    if ev["type_id"] != TARGET:
        return
    state["slain"] = True
    q.set_var("hollow_bell_finale_ready", True)
    q.log("hollow_bell/the_deeplistener: slain")


def _find_deeplistener():
    """Live Deeplistener object ids. `world.objects()` dicts key the runtime
    id as `id` (see `object_to_dict`), not `object_id`."""
    found = []
    for obj in q.objects():
        if obj["type_id"] == TARGET:
            found.append(obj["id"])
    return found


def on_command(name, args, state):
    if name == "ring":
        if state["rung"]:
            return
        state["rung"] = True
        q.set_var("hollow_bell_finale_rang", True)
        q.set_var("hollow_bell_finale_ready", True)
        # It stops searching, and turns, and goes back down.
        for object_id in _find_deeplistener():
            q.despawn(object_id)
        q.log("hollow_bell/the_deeplistener: rung — the Deeplistener sleeps")
        return

    if name == "complete":
        q.set_var("hollow_bell_finale_done", True)
        q.complete_quest("hollow_bell/the_deeplistener")
